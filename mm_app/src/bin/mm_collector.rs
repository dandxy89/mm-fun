use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use mm_app::cli;
use mm_app::shutdown_handler;
use mm_app::time_utils;
use mm_app::zmq_config;
use mm_binary::CollectorState;
use mm_binary::CollectorStateMessage;
use mm_binary::CompressedString;
use mm_binary::Exchange;
use mm_binary::HeartbeatMessage;
use mm_binary::OrderBookBatchMessage;
use mm_binary::messages::UpdateType;
use mm_binary::to_fixed_point;
use mm_ws::AffinityManager;
use mm_ws::BinanceIngestor;
use mm_zmq::Publisher;
use simd_json::prelude::ValueAsArray;
use simd_json::prelude::ValueAsScalar;
use simd_json::prelude::ValueObjectAccess;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Parse JSON orderbook update and send as a single batch message
fn parse_and_send_batch(json_str: &str, tx: &Sender<Bytes>) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = json_str.as_bytes().to_vec();
    let parsed = simd_json::to_borrowed_value(&mut bytes)?;

    // Extract fields from Binance format
    let symbol_str = parsed["s"].as_str().ok_or("Missing symbol")?;
    let timestamp = parsed["E"].as_u64().ok_or("Missing timestamp")?;

    // Encode symbol
    let (symbol, encoding) = CompressedString::from_str(symbol_str)?;

    // Determine update type based on presence of 'u' field (update ID)
    let update_type = if parsed.get("u").is_some() { UpdateType::Update } else { UpdateType::Snapshot };

    // Get all bid and ask levels
    let bids = parsed["b"].as_array().ok_or("Missing bids")?;
    let asks = parsed["a"].as_array().ok_or("Missing asks")?;

    // Create batch message
    let mut batch = OrderBookBatchMessage::new(Exchange::Binance, update_type, symbol, encoding, timestamp);

    // Add all bids
    for bid in bids {
        if let Some(bid_array) = bid.as_array()
            && bid_array.len() >= 2
        {
            let price = bid_array[0].as_str().ok_or("Invalid bid price")?.parse::<f64>()?;
            let size = bid_array[1].as_str().ok_or("Invalid bid size")?.parse::<f64>()?;
            batch.add_bid(to_fixed_point(price), to_fixed_point(size));
        }
    }

    // Add all asks
    for ask in asks {
        if let Some(ask_array) = ask.as_array()
            && ask_array.len() >= 2
        {
            let price = ask_array[0].as_str().ok_or("Invalid ask price")?.parse::<f64>()?;
            let size = ask_array[1].as_str().ok_or("Invalid ask size")?.parse::<f64>()?;
            batch.add_ask(to_fixed_point(price), to_fixed_point(size));
        }
    }

    // Send single batch message containing all levels
    let msg_bytes = Bytes::from(batch.to_bytes());
    tx.send(msg_bytes)?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: Keep guard alive for entire application lifetime
    let _guard = mm_app::tracing_setup::init_with_stdout("mm_collector", "./logs", tracing::Level::INFO);

    // Get symbol from command line or use default
    let symbol = cli::get_symbol_lowercase("btcusdt");
    info!("Starting dual WS collector for {symbol}");

    // Initialise affinity manager for CPU pinning
    let affinity_manager = AffinityManager::new();

    // Create bounded channels for lock-free communication
    // Processing thread -> Publisher thread
    let (tx, rx) = bounded::<Bytes>(zmq_config::DEFAULT_CHANNEL_CAPACITY);

    // Initialise ZeroMQ publisher
    let mut publisher = Publisher::new();
    publisher.bind(zmq_config::MARKET_DATA_PUBLISH_ADDR)?;
    info!("ZMQ publisher bound to {}", zmq_config::MARKET_DATA_PUBLISH_ADDR);

    // Create state publisher
    let mut state_publisher = Publisher::new();
    state_publisher.bind(zmq_config::STATE_PUBLISH_ADDR)?;
    info!("State publisher bound to {}", zmq_config::STATE_PUBLISH_ADDR);

    // Set up running flag
    let running = Arc::new(AtomicBool::new(true));

    // Message counters for each connection (shared with state publisher)
    let msg_count_conn1 = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let msg_count_conn2 = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Create two Binance ingestors (racing connections)
    let mut ingestor1 = BinanceIngestor::new(&symbol)?;
    let mut ingestor2 = BinanceIngestor::new(&symbol)?;

    ingestor1.connect()?;
    ingestor2.connect()?;
    info!("WebSocket connections established for {symbol}");

    let ingestor1_running = Arc::clone(&ingestor1.running);
    let ingestor2_running = Arc::clone(&ingestor2.running);
    ingestor1_running.store(true, Ordering::Relaxed);
    ingestor2_running.store(true, Ordering::Relaxed);

    // Start processing thread for connection 1
    let tx_clone1 = tx.clone();
    let msg_count1 = Arc::clone(&msg_count_conn1);
    let processing_handle1 = {
        let affinity_mgr = affinity_manager;
        ingestor1.start_processing_thread(move |data| {
            // Pin this thread to dedicated core (first time only)
            static PINNED: AtomicBool = AtomicBool::new(false);
            if !PINNED.swap(true, Ordering::Relaxed) {
                affinity_mgr.pin_parser_thread(0);
                debug!("Parser thread 1 pinned to core 0");
            }

            if let Ok(json_str) = std::str::from_utf8(data) {
                // Parse JSON and send as batch message
                if let Err(err) = parse_and_send_batch(json_str, &tx_clone1) {
                    warn!("Failed to parse message on conn1: {err}");
                } else {
                    msg_count1.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
    };

    // Start processing thread for connection 2
    let tx_clone2 = tx.clone();
    let msg_count2 = Arc::clone(&msg_count_conn2);
    let processing_handle2 = {
        let affinity_mgr = AffinityManager::new();
        ingestor2.start_processing_thread(move |data| {
            // Pin this thread to a different core
            static PINNED: AtomicBool = AtomicBool::new(false);
            if !PINNED.swap(true, Ordering::Relaxed) {
                affinity_mgr.pin_parser_thread(1);
                debug!("Parser thread 2 pinned to core 1");
            }

            if let Ok(json_str) = std::str::from_utf8(data) {
                // Parse JSON and send as batch message
                if let Err(err) = parse_and_send_batch(json_str, &tx_clone2) {
                    warn!("Failed to parse message on conn2: {err}");
                } else {
                    msg_count2.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
    };

    // Set up Ctrl+C handler
    shutdown_handler::setup_multi(vec![Arc::clone(&running), Arc::clone(&ingestor1_running), Arc::clone(&ingestor2_running)])?;

    // Start WebSocket ingestion threads
    let running_clone1 = Arc::clone(&running);
    let ingestion_handle1 = std::thread::spawn(move || {
        match ingestor1.run() {
            Ok(_) => info!("Ingestion thread 1 exited cleanly"),
            Err(err) => error!("Ingestion thread 1 error: {err}"),
        }
        running_clone1.store(false, Ordering::Relaxed);
    });

    let running_clone2 = Arc::clone(&running);
    let ingestion_handle2 = std::thread::spawn(move || {
        match ingestor2.run() {
            Ok(_) => info!("Ingestion thread 2 exited cleanly"),
            Err(err) => error!("Ingestion thread 2 error: {err}"),
        }
        running_clone2.store(false, Ordering::Relaxed);
    });

    // Spawn dedicated publisher thread (synchronous)
    let rx_clone = rx.clone();
    let publisher_handle = std::thread::spawn(move || {
        let mut pub_instance = publisher;
        while let Ok(msg_bytes) = rx_clone.recv() {
            let _ = pub_instance.publish_with_topic(zmq_config::MARKET_DATA_TOPIC, msg_bytes);
        }
    });

    // Spawn state publisher thread (synchronous)
    let running_clone3 = Arc::clone(&running);
    let msg_count1_clone = Arc::clone(&msg_count_conn1);
    let msg_count2_clone = Arc::clone(&msg_count_conn2);
    let state_handle = std::thread::spawn(move || {
        let mut state_pub = state_publisher;
        let mut last_count1 = 0u64;
        let mut last_count2 = 0u64;

        while running_clone3.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(zmq_config::STATE_UPDATE_INTERVAL_MS));
            let timestamp = time_utils::unix_timestamp_ms();

            // Publish state for connection 1
            let count1 = msg_count1_clone.load(Ordering::Relaxed);
            let state1 = if count1 > last_count1 {
                CollectorState::Receiving
            } else if count1 > 0 {
                CollectorState::Connected
            } else {
                CollectorState::Connecting
            };
            last_count1 = count1;

            let state_msg1 = CollectorStateMessage::new(1, state1, timestamp, count1);
            let _ = state_pub.publish_with_topic(zmq_config::COLLECTOR_STATE_TOPIC, Bytes::from(state_msg1.to_bytes().to_vec()));

            // Publish state for connection 2
            let count2 = msg_count2_clone.load(Ordering::Relaxed);
            let state2 = if count2 > last_count2 {
                CollectorState::Receiving
            } else if count2 > 0 {
                CollectorState::Connected
            } else {
                CollectorState::Connecting
            };
            last_count2 = count2;

            let state_msg2 = CollectorStateMessage::new(2, state2, timestamp, count2);
            let _ = state_pub.publish_with_topic(zmq_config::COLLECTOR_STATE_TOPIC, Bytes::from(state_msg2.to_bytes().to_vec()));
        }
    });

    // Spawn heartbeat publisher thread
    let running_clone4 = Arc::clone(&running);
    let heartbeat_handle = std::thread::spawn(move || {
        let mut heartbeat_pub = Publisher::new();
        if let Err(err) = heartbeat_pub.bind(zmq_config::HEARTBEAT_PUBLISH_ADDR) {
            error!("Failed to bind heartbeat publisher: {err}");
            return;
        }
        info!("Heartbeat publisher bound to {}", zmq_config::HEARTBEAT_PUBLISH_ADDR);

        let mut sequence = 0u64;

        while running_clone4.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(zmq_config::HEARTBEAT_INTERVAL_MS));

            let timestamp = time_utils::unix_timestamp_ms();

            let heartbeat = HeartbeatMessage::new(timestamp, sequence);
            let _ = heartbeat_pub.publish_with_topic(zmq_config::HEARTBEAT_TOPIC, Bytes::from(heartbeat.to_bytes().to_vec()));

            sequence = sequence.wrapping_add(1);
        }

        info!("Heartbeat publisher thread exiting");
    });

    // Run until Ctrl+C (pure sync - just sleep)
    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }

    // Stop the ingestors
    ingestor1_running.store(false, Ordering::Relaxed);
    ingestor2_running.store(false, Ordering::Relaxed);

    // Wait for threads to finish
    let _ = ingestion_handle1.join();
    let _ = ingestion_handle2.join();
    let _ = processing_handle1.join();
    let _ = processing_handle2.join();

    // Close channel and wait for publisher to finish
    drop(tx); // Close sender
    let _ = publisher_handle.join();
    let _ = state_handle.join();
    let _ = heartbeat_handle.join();

    info!("Shutdown complete");
    Ok(())
}
