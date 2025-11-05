use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use mm_aeron::Subscriber;
use mm_app::aeron_config;
use mm_app::cli;
use mm_app::monitoring;
use mm_app::shutdown_handler;
use mm_binary::OrderBookBatchMessage;
use mm_binary::from_fixed_point;
use mm_http::binance::BinanceClient;
use mm_orderbook::OrderBook;
use tracing::debug;
use tracing::info;
use tracing::warn;

const HEARTBEAT_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const MIDPOINT_LOG_INTERVAL: Duration = Duration::from_secs(1);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: Keep guard alive for entire application lifetime
    let _guard = mm_app::tracing_setup::init_with_stdout("mm_pricing", "./logs", tracing::Level::INFO);

    // Get symbol from command line or use default
    let symbol = cli::get_symbol_uppercase("BTCUSDT");
    info!("Starting pricing app for {symbol}");

    // Connect to Aeron for market data (synchronous)
    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID)?;
    info!("Subscribed to market data on stream {}", aeron_config::MARKET_DATA_STREAM_ID);

    // Fetch full orderbook snapshot via HTTP
    // We need a minimal tokio runtime ONLY for this one-time HTTP call
    info!("Fetching initial orderbook snapshot for {}", symbol);
    let snapshot = tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let binance_client = BinanceClient::new()?;
        binance_client.orderbook(&symbol, 100).await
    })?;
    info!("Received snapshot with {} bids, {} asks", snapshot.bids.len(), snapshot.asks.len());

    // Initialise orderbook with snapshot data
    let mut orderbook = OrderBook::new(&symbol);
    for (price_fixed, qty_fixed) in &snapshot.bids {
        let price = from_fixed_point(*price_fixed);
        let qty = from_fixed_point(*qty_fixed);
        orderbook.update_bid(price, qty);
    }
    for (price_fixed, qty_fixed) in &snapshot.asks {
        let price = from_fixed_point(*price_fixed);
        let qty = from_fixed_point(*qty_fixed);
        orderbook.update_ask(price, qty);
    }

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    shutdown_handler::setup(Arc::clone(&running))?;

    // Spawn background thread to monitor collector state
    info!("Starting collector state monitor");
    let state_monitor_handle = monitoring::spawn_state_monitor(monitoring::StateMonitorConfig::default(), Arc::clone(&running))?;

    // Spawn background thread to monitor heartbeats
    info!("Starting heartbeat monitor");
    let (heartbeat_monitor_handle, last_heartbeat_timestamp) =
        monitoring::spawn_heartbeat_monitor(monitoring::HeartbeatConfig::default(), Arc::clone(&running))?;

    // Main message processing loop (fully synchronous!)
    info!("Starting main market data processing loop");
    let mut msg_count = 0u64;
    let mut last_heartbeat_check = std::time::Instant::now();
    let mut last_midpoint_log = std::time::Instant::now();
    let mut orderbook_synchronised = false;

    while running.load(Ordering::Relaxed) {
        // Periodically check for stale heartbeats
        if last_heartbeat_check.elapsed() > HEARTBEAT_CHECK_INTERVAL {
            monitoring::is_heartbeat_stale(&last_heartbeat_timestamp, aeron_config::HEARTBEAT_TIMEOUT_MS);
            last_heartbeat_check = Instant::now();
        }
        // Receive binary message from Aeron (synchronous)
        let data = match subscriber.receive() {
            Ok(d) => d,
            Err(err) => {
                warn!("Aeron receive error: {err}");
                continue;
            }
        };

        // Deserialize batch message
        let batch = match OrderBookBatchMessage::from_bytes(&data) {
            Ok(m) => m,
            Err(err) => {
                warn!("Failed to deserialize batch message: {err}");
                continue;
            }
        };

        msg_count += 1;
        if msg_count.is_multiple_of(1_000) {
            debug!("Processed {msg_count} batch messages");
        }

        // Verify symbol matches (skip messages for other symbols)
        let msg_symbol = batch.symbol().decode(batch.encoding());
        if msg_symbol != symbol {
            continue;
        }

        // Update orderbook with all bids from the batch
        for bid in batch.bids() {
            let price = from_fixed_point(bid.price);
            let qty = from_fixed_point(bid.size);
            if price > 0.0 {
                orderbook.update_bid(price, qty);
            }
        }

        // Update orderbook with all asks from the batch
        for ask in batch.asks() {
            let price = from_fixed_point(ask.price);
            let qty = from_fixed_point(ask.size);
            if price > 0.0 {
                orderbook.update_ask(price, qty);
            }
        }

        orderbook.timestamp = batch.timestamp();
        orderbook.trim_book();

        // Log once when orderbook is synchronised (has both bids and asks from live updates)
        if !orderbook_synchronised && orderbook.best_bid().is_some() && orderbook.best_ask().is_some() {
            info!("Orderbook synchronised for {}", symbol);
            orderbook_synchronised = true;
        }

        // Log mid-point every 5 seconds
        if orderbook_synchronised && last_midpoint_log.elapsed() > MIDPOINT_LOG_INTERVAL {
            if let (Some((bid_price, _)), Some((ask_price, _))) = (orderbook.best_bid(), orderbook.best_ask()) {
                let spread = ask_price - bid_price;
                if spread >= 0.0 {
                    let mid_point = (bid_price + ask_price) / 2.0;
                    info!("Mid-point: {:.2} (bid: {:.2}, ask: {:.2}, spread: {:.2})", mid_point, bid_price, ask_price, spread);
                    last_midpoint_log = Instant::now();
                }
            }
        }
    }

    // Wait for background threads to finish
    let _ = state_monitor_handle.join();
    let _ = heartbeat_monitor_handle.join();

    info!("Shutdown complete. Processed {msg_count} total messages");
    Ok(())
}
