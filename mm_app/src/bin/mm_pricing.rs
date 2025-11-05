use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use mm_aeron::Publisher;
use mm_aeron::Subscriber;
use mm_app::aeron_config;
use mm_app::cli;
use mm_app::monitoring;
use mm_app::shutdown_handler;
use mm_binary::CompressedString;
use mm_binary::OrderBookBatchMessage;
use mm_binary::from_fixed_point;
use mm_binary::messages::PricingOutputMessage;
use mm_binary::messages::TradeMessage;
use mm_binary::to_fixed_point;
use mm_orderbook::OrderBook;
use mm_strategy::FixedPoint;
use mm_strategy::MarketState;
use mm_strategy::StrategyConfig;
use mm_strategy::drift_estimator::DriftEstimator;
use mm_strategy::drift_estimator::Trade;
use tracing::debug;
use tracing::info;
use tracing::warn;

const HEARTBEAT_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const MIDPOINT_LOG_INTERVAL: Duration = Duration::from_secs(1);
const PRICING_PUBLISH_INTERVAL: Duration = Duration::from_millis(500);

fn publish_pricing_output(
    orderbook: &OrderBook,
    drift_estimator: &DriftEstimator,
    symbol: &str,
    publisher: &mut Publisher,
    last_trade_price: Option<FixedPoint>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get best bid and ask (now returns i64 fixed-point)
    let (bid_price_i64, bid_volume_i64) = orderbook.best_bid().ok_or("No best bid")?;
    let (ask_price_i64, ask_volume_i64) = orderbook.best_ask().ok_or("No best ask")?;

    // Build market state
    let state = MarketState {
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos() as u64,
        bid_price: FixedPoint(bid_price_i64),
        ask_price: FixedPoint(ask_price_i64),
        bid_volume: FixedPoint(bid_volume_i64),
        ask_volume: FixedPoint(ask_volume_i64),
        last_trade_price,
        last_trade_size: None,
    };

    // Calculate micro-price (more predictive than mid)
    let micro = state.micro_price();

    // Estimate drift in basis points
    let drift_bps = drift_estimator.estimate_drift_bps(&state);

    // Calculate fair value: micro-price + drift adjustment
    let drift_adjustment = micro * FixedPoint::from_f64(drift_bps / 10000.0);
    let fair_value = micro + drift_adjustment;

    // Calculate volatility from drift estimator
    let volatility = drift_estimator.current_volatility();

    // Calculate confidence based on orderbook depth and spread
    let spread_bps = state.spread_bps();
    let total_depth_fp = state.bid_volume + state.ask_volume;
    let total_depth = total_depth_fp.to_f64();

    // Higher confidence with tighter spreads and deeper books
    let confidence = if spread_bps < 5.0 && total_depth > 1.0 {
        0.9
    } else if spread_bps < 10.0 && total_depth > 0.5 {
        0.7
    } else if spread_bps < 20.0 && total_depth > 0.1 {
        0.5
    } else {
        0.3
    };

    // Encode symbol
    let (symbol_compressed, encoding) = CompressedString::from_str(symbol)?;

    // Create pricing output message
    let pricing_msg = PricingOutputMessage::new(
        0, // strategy_id
        symbol_compressed,
        encoding,
        state.timestamp,
        fair_value.to_i64(),
        to_fixed_point(confidence),
        to_fixed_point(volatility),
    );

    // Publish message
    let bytes = Bytes::from(pricing_msg.to_bytes().to_vec());
    publisher.publish(bytes)?;

    debug!(
        "Published pricing: fair_value=${:.2}, drift={:.2}bps, volatility={:.4}, confidence={:.2}",
        fair_value.to_f64(),
        drift_bps,
        volatility,
        confidence
    );

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: Keep guard alive for entire application lifetime
    let _guard = mm_app::tracing_setup::init_with_stdout("mm_pricing", "./logs", tracing::Level::INFO);

    // Get symbol from command line or use default
    let symbol = cli::get_symbol_uppercase("BTCUSDT");
    info!("Starting pricing app for {symbol}");

    // Initialize drift estimator with default config
    let mut drift_estimator = DriftEstimator::new(StrategyConfig::default());

    // Connect to Aeron for market data and trades (synchronous)
    // IMPORTANT: Need separate subscribers for each stream!
    let mut market_data_subscriber = Subscriber::new();
    market_data_subscriber.add_subscription(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID)?;
    info!("Subscribed to market data on stream {}", aeron_config::MARKET_DATA_STREAM_ID);

    let mut trade_subscriber = Subscriber::new();
    trade_subscriber.add_subscription(aeron_config::TRADE_DATA_CHANNEL, aeron_config::TRADE_DATA_STREAM_ID)?;
    info!("Subscribed to trade data on stream {}", aeron_config::TRADE_DATA_STREAM_ID);

    // Create publisher for pricing output
    let mut pricing_publisher = Publisher::new();
    pricing_publisher.add_publication(aeron_config::PRICING_OUTPUT_CHANNEL, aeron_config::PRICING_OUTPUT_STREAM_ID)?;
    info!("Publishing pricing output on stream {}", aeron_config::PRICING_OUTPUT_STREAM_ID);

    // Fetch full orderbook snapshot via HTTP
    info!("Fetching initial orderbook snapshot for {symbol}");
    let snapshot = mm_app::orderbook_helpers::fetch_orderbook_snapshot(&symbol, 100)?;
    info!("Received snapshot with {} bids, {} asks", snapshot.bids.len(), snapshot.asks.len());

    // Initialise orderbook with snapshot data
    let mut orderbook = OrderBook::new(&symbol);
    for (price_fixed, qty_fixed) in &snapshot.bids {
        orderbook.update_bid(*price_fixed, *qty_fixed);
    }
    for (price_fixed, qty_fixed) in &snapshot.asks {
        orderbook.update_ask(*price_fixed, *qty_fixed);
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
    let mut trade_count = 0u64;
    let mut last_heartbeat_check = std::time::Instant::now();
    let mut last_midpoint_log = std::time::Instant::now();
    let mut last_pricing_publish = std::time::Instant::now();
    let mut orderbook_synchronised = false;
    let mut last_trade_price: Option<FixedPoint> = None;

    while running.load(Ordering::Relaxed) {
        // Periodically check for stale heartbeats
        if last_heartbeat_check.elapsed() > HEARTBEAT_CHECK_INTERVAL {
            monitoring::is_heartbeat_stale(&last_heartbeat_timestamp, aeron_config::HEARTBEAT_TIMEOUT_MS);
            last_heartbeat_check = Instant::now();
        }
        // Check if we should publish pricing output
        if orderbook_synchronised && last_pricing_publish.elapsed() > PRICING_PUBLISH_INTERVAL {
            match publish_pricing_output(&orderbook, &drift_estimator, &symbol, &mut pricing_publisher, last_trade_price) {
                Ok(_) => {}
                Err(err) if err.to_string().contains("back pressure") => {
                    // Silently ignore back-pressure (no subscribers, fire-and-forget stream)
                }
                Err(err) => {
                    warn!("Failed to publish pricing output: {err}");
                }
            }
            last_pricing_publish = std::time::Instant::now();
        }

        // Try to receive from trade subscriber first (non-blocking)
        if let Ok(Some(data)) = trade_subscriber.try_receive() {
            // Parse trade message (trade stream only has trade messages)
            match TradeMessage::from_bytes(&data) {
                Ok(trade_msg) => {
                    trade_count += 1;

                    let trade = Trade {
                        timestamp: trade_msg.timestamp,
                        price: FixedPoint(trade_msg.price),
                        quantity: FixedPoint(trade_msg.quantity),
                        side: trade_msg.trade_side(),
                        is_aggressor: trade_msg.is_aggressor != 0,
                    };

                    last_trade_price = Some(FixedPoint(trade_msg.price));
                    drift_estimator.add_trade(trade);

                    if trade_count.is_multiple_of(100) {
                        debug!("Processed {} trades", trade_count);
                    }
                }
                Err(err) => {
                    warn!(
                        "Failed to parse trade message: {err} (msg len: {} bytes, first byte: 0x{:02x})",
                        data.len(),
                        data.first().unwrap_or(&0)
                    );
                }
            }
            continue;
        }

        // Receive market data message from Aeron (blocking with timeout)
        let data = match market_data_subscriber.receive() {
            Ok(d) => d,
            Err(_err) => {
                // No data available, continue loop
                continue;
            }
        };

        // Parse as orderbook batch message (market data stream only has orderbook messages)
        let batch = match OrderBookBatchMessage::from_bytes(&data) {
            Ok(m) => m,
            Err(err) => {
                warn!("Failed to deserialize message: {err} (msg len: {} bytes)", data.len());
                // Debug: print first 64 bytes in hex
                if data.len() >= 64 {
                    let hex: String = data[..64].iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                    debug!("First 64 bytes: {}", hex);
                }
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

        // Apply batch update to orderbook
        orderbook.apply_batch(&batch);
        orderbook.trim_book();

        // Log once when orderbook is synchronised (has both bids and asks from live updates)
        if !orderbook_synchronised && orderbook.best_bid().is_some() && orderbook.best_ask().is_some() {
            info!("Orderbook synchronised for {symbol}");
            orderbook_synchronised = true;
        }

        // Log mid-point every 5 seconds
        if orderbook_synchronised && last_midpoint_log.elapsed() > MIDPOINT_LOG_INTERVAL {
            if let (Some((bid_price, _)), Some((ask_price, _))) = (orderbook.best_bid(), orderbook.best_ask()) {
                let spread_i64 = ask_price - bid_price;
                if spread_i64 >= 0 {
                    let mid_point = from_fixed_point((bid_price + ask_price) / 2);
                    let bid_f64 = from_fixed_point(bid_price);
                    let ask_f64 = from_fixed_point(ask_price);
                    let spread_f64 = from_fixed_point(spread_i64);
                    info!("Mid-point: {:.2} (bid: {:.2}, ask: {:.2}, spread: {:.2})", mid_point, bid_f64, ask_f64, spread_f64);
                    last_midpoint_log = Instant::now();
                }
            }
        }
    }

    // Wait for background threads to finish
    let _ = state_monitor_handle.join();
    let _ = heartbeat_monitor_handle.join();

    info!("Shutdown complete. Processed {} orderbook messages, {} trades", msg_count, trade_count);
    Ok(())
}
