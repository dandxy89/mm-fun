use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use mm_aeron::Publisher;
use mm_app::aeron_config;
use mm_app::cli;
use mm_app::shutdown_handler;
use mm_app::time_utils;
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
        if let Some(bid_array) = bid.as_array() {
            if bid_array.len() >= 2 {
                let price = bid_array[0].as_str().ok_or("Invalid bid price")?.parse::<f64>()?;
                let size = bid_array[1].as_str().ok_or("Invalid bid size")?.parse::<f64>()?;
                batch.add_bid(to_fixed_point(price), to_fixed_point(size));
            }
        }
    }

    // Add all asks
    for ask in asks {
        if let Some(ask_array) = ask.as_array() {
            if ask_array.len() >= 2 {
                let price = ask_array[0].as_str().ok_or("Invalid ask price")?.parse::<f64>()?;
                let size = ask_array[1].as_str().ok_or("Invalid ask size")?.parse::<f64>()?;
                batch.add_ask(to_fixed_point(price), to_fixed_point(size));
            }
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
    let (tx, rx) = bounded::<Bytes>(aeron_config::DEFAULT_CHANNEL_CAPACITY);

    // Note: Aeron publishers will be created inside their threads to avoid Send issues

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
    let affinity_mgr1 = affinity_manager;
    let processing_handle1 = {
        let affinity_mgr = affinity_mgr1;
        ingestor1.start_processing_thread(move |data| {
            // Pin this thread to dedicated core (first time only)
            static PINNED: AtomicBool = AtomicBool::new(false);
            if !PINNED.swap(true, Ordering::Relaxed) {
                affinity_mgr.pin_parser_thread(0);
                debug!("Parser thread 1 pinned to core 0");
            }

            match std::str::from_utf8(data) {
                Ok(json_str) => {
                    // Parse JSON and send as batch message
                    if let Err(err) = parse_and_send_batch(json_str, &tx_clone1) {
                        warn!("Failed to parse message on conn1: {err}");
                    } else {
                        msg_count1.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    warn!("Received invalid UTF-8 data on conn1: {e}");
                }
            }
        })
    };

    // Start processing thread for connection 2
    let tx_clone2 = tx.clone();
    let msg_count2 = Arc::clone(&msg_count_conn2);
    let processing_handle2 = {
        // Note: Need separate AffinityManager since first one was moved into thread 1 closure
        let affinity_mgr = AffinityManager::new();
        ingestor2.start_processing_thread(move |data| {
            // Pin this thread to a different core
            static PINNED: AtomicBool = AtomicBool::new(false);
            if !PINNED.swap(true, Ordering::Relaxed) {
                affinity_mgr.pin_parser_thread(1);
                debug!("Parser thread 2 pinned to core 1");
            }

            match std::str::from_utf8(data) {
                Ok(json_str) => {
                    // Parse JSON and send as batch message
                    if let Err(err) = parse_and_send_batch(json_str, &tx_clone2) {
                        warn!("Failed to parse message on conn2: {err}");
                    } else {
                        msg_count2.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    warn!("Received invalid UTF-8 data on conn2: {e}");
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
        // Create publisher inside thread to avoid Send issues
        let mut pub_instance = Publisher::new();
        if let Err(err) = pub_instance.add_publication(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID) {
            error!("Failed to add market data publisher: {err}");
            return;
        }
        info!("Market data publisher added on stream {}", aeron_config::MARKET_DATA_STREAM_ID);

        while let Ok(msg_bytes) = rx_clone.recv() {
            match pub_instance.publish(msg_bytes) {
                Ok(_) => {}
                Err(mm_aeron::AeronError::BackPressure) => {
                    // Log back-pressure but don't crash - this is expected under high load
                    warn!("Back-pressure detected on market data publisher");
                    // Brief sleep to allow subscriber to catch up
                    std::thread::sleep(Duration::from_micros(10));
                }
                Err(e) => {
                    error!("Failed to publish market data: {e}");
                    // Consider: increment error counter for monitoring
                }
            }
        }
    });

    // Spawn state publisher thread (synchronous)
    let running_clone3 = Arc::clone(&running);
    let msg_count1_clone = Arc::clone(&msg_count_conn1);
    let msg_count2_clone = Arc::clone(&msg_count_conn2);
    let state_handle = std::thread::spawn(move || {
        // Create state publisher inside thread to avoid Send issues
        let mut state_pub = Publisher::new();
        if let Err(err) = state_pub.add_publication(aeron_config::STATE_CHANNEL, aeron_config::STATE_STREAM_ID) {
            error!("Failed to add state publisher: {err}");
            return;
        }
        info!("State publisher added on stream {}", aeron_config::STATE_STREAM_ID);

        let mut last_count1 = 0u64;
        let mut last_count2 = 0u64;

        while running_clone3.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(aeron_config::STATE_UPDATE_INTERVAL_MS));
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
            let bytes_array = state_msg1.to_bytes();
            if let Err(e) = state_pub.publish(Bytes::copy_from_slice(&bytes_array)) {
                warn!("Failed to publish state for connection 1: {e}");
            }

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
            let bytes_array = state_msg2.to_bytes();
            if let Err(e) = state_pub.publish(Bytes::copy_from_slice(&bytes_array)) {
                warn!("Failed to publish state for connection 2: {e}");
            }
        }
    });

    // Spawn heartbeat publisher thread
    let running_clone4 = Arc::clone(&running);
    let heartbeat_handle = std::thread::spawn(move || {
        let mut heartbeat_pub = Publisher::new();
        if let Err(err) = heartbeat_pub.add_publication(aeron_config::HEARTBEAT_CHANNEL, aeron_config::HEARTBEAT_STREAM_ID) {
            error!("Failed to add heartbeat publisher: {err}");
            return;
        }
        info!("Heartbeat publisher added on stream {}", aeron_config::HEARTBEAT_STREAM_ID);

        let mut sequence = 0u64;

        while running_clone4.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(aeron_config::HEARTBEAT_INTERVAL_MS));

            let timestamp = time_utils::unix_timestamp_ms();

            let heartbeat = HeartbeatMessage::new(timestamp, sequence);
            let bytes_array = heartbeat.to_bytes();
            if let Err(e) = heartbeat_pub.publish(Bytes::copy_from_slice(&bytes_array)) {
                warn!("Failed to publish heartbeat: {e}");
            }

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
