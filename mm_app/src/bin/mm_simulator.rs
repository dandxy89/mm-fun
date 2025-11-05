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
use mm_app::config_loader;
use mm_app::monitoring;
use mm_app::orderbook_sync::OrderbookSyncState;
use mm_app::shutdown_handler;
use mm_binary::CompressedString;
use mm_binary::Exchange;
use mm_binary::OrderBookBatchMessage;
use mm_binary::from_fixed_point;
use mm_binary::messages::OrderFillMessage;
use mm_binary::messages::OrderSide;
use mm_binary::messages::QuoteMessage;
use mm_orderbook::OrderBook;
use mm_sim_executor::OrderBookSimulator;
use mm_sim_executor::SimulatedFill;
use mm_strategy::FixedPoint;
use tracing::debug;
use tracing::info;
use tracing::warn;

const HEARTBEAT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let _guard = mm_app::tracing_setup::init_with_stdout("mm_simulator", "./logs", tracing::Level::INFO);

    // Load simulator configuration from file (with fallback to defaults)
    let config_file = config_loader::load_simulator_config_or_default("config/simulator.toml");
    let symbol = cli::get_symbol_uppercase(&config_file.symbol);
    let config = config_file.simulator;

    info!("Starting order fill simulator for {symbol}");
    info!(
        "Simulator config: placement_latency={}us, cancellation_latency={}us, fill_prob={}, track_queue={}",
        config.order_placement_latency_us,
        config.order_cancellation_latency_us,
        config.fill_probability_factor,
        config.track_queue_position
    );

    let mut simulator = OrderBookSimulator::new(config);

    // Connect to Aeron - need separate subscribers for each stream!
    let mut market_data_subscriber = Subscriber::new();
    market_data_subscriber.add_subscription(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID)?;
    info!("Subscribed to market data on stream {}", aeron_config::MARKET_DATA_STREAM_ID);

    let mut strategy_quotes_subscriber = Subscriber::new();
    strategy_quotes_subscriber.add_subscription(aeron_config::STRATEGY_QUOTES_CHANNEL, aeron_config::STRATEGY_QUOTES_STREAM_ID)?;
    info!("Subscribed to strategy quotes on stream {}", aeron_config::STRATEGY_QUOTES_STREAM_ID);

    // Create publisher for order fills
    let mut fill_publisher = Publisher::new();
    fill_publisher.add_publication(aeron_config::ORDER_FILLS_CHANNEL, aeron_config::ORDER_FILLS_STREAM_ID)?;
    info!("Publishing order fills on stream {}", aeron_config::ORDER_FILLS_STREAM_ID);

    // Fetch initial orderbook snapshot
    info!("Fetching initial orderbook snapshot for {symbol}");
    let snapshot = mm_app::orderbook_helpers::fetch_orderbook_snapshot(&symbol, 100)?;
    info!("Received snapshot with {} bids, {} asks", snapshot.bids.len(), snapshot.asks.len());

    // Initialize orderbook synchronization state
    let mut sync_state = OrderbookSyncState::new(snapshot.last_update_id);

    // Initialize orderbook
    let mut orderbook = OrderBook::new(&symbol);
    for (price_fixed, qty_fixed) in &snapshot.bids {
        orderbook.update_bid(*price_fixed, *qty_fixed);
    }
    for (price_fixed, qty_fixed) in &snapshot.asks {
        orderbook.update_ask(*price_fixed, *qty_fixed);
    }

    // Set up shutdown handler
    let running = Arc::new(AtomicBool::new(true));
    shutdown_handler::setup(Arc::clone(&running))?;

    // Start monitors
    info!("Starting collector state monitor");
    let _state_monitor_handle = monitoring::spawn_state_monitor(monitoring::StateMonitorConfig::default(), Arc::clone(&running))?;

    info!("Starting heartbeat monitor");
    let (_heartbeat_monitor_handle, last_heartbeat_timestamp) =
        monitoring::spawn_heartbeat_monitor(monitoring::HeartbeatConfig::default(), Arc::clone(&running))?;

    // Main processing loop
    info!("Starting simulator main loop");
    let mut msg_count = 0u64;
    let mut quote_count = 0u64;
    let mut fill_count = 0u64;
    let mut last_heartbeat_check = Instant::now();
    let mut orderbook_synchronized = false;
    let last_trade_price: Option<FixedPoint> = None;

    while running.load(Ordering::Relaxed) {
        // Check heartbeat
        if last_heartbeat_check.elapsed() > HEARTBEAT_CHECK_INTERVAL {
            monitoring::is_heartbeat_stale(&last_heartbeat_timestamp, aeron_config::HEARTBEAT_TIMEOUT_MS);
            last_heartbeat_check = Instant::now();
        }

        // Try to receive from strategy quotes subscriber first (non-blocking)
        if let Ok(Some(data)) = strategy_quotes_subscriber.try_receive() {
            // Try to parse as QuoteMessage
            if let Ok(quote_msg) = QuoteMessage::from_bytes(&data) {
                quote_count += 1;

                // Convert quote message to StrategyQuote
                let quote = mm_strategy::StrategyQuote {
                    timestamp: quote_msg.timestamp,
                    bid_price: FixedPoint(quote_msg.bid_price),
                    bid_size: FixedPoint(quote_msg.bid_size),
                    ask_price: FixedPoint(quote_msg.ask_price),
                    ask_size: FixedPoint(quote_msg.ask_size),
                    fair_value: FixedPoint(quote_msg.fair_value),
                    inventory: FixedPoint(quote_msg.inventory),
                    confidence: from_fixed_point(quote_msg.confidence),
                };

                // Cancel previous orders (simplified: cancel all and replace)
                simulator.cancel_all_orders();

                // Place new orders from quote
                let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos() as u64;

                let order_ids = simulator.place_orders_from_quote(&quote, timestamp);

                debug!(
                    "Received quote: bid ${:.2} x {}, ask ${:.2} x {} | Placed {} orders",
                    quote.bid_price.to_f64(),
                    quote.bid_size.to_f64(),
                    quote.ask_price.to_f64(),
                    quote.ask_size.to_f64(),
                    order_ids.len()
                );
            } else {
                warn!("Failed to parse quote message");
            }
            continue;
        }

        // Receive market data messages from Aeron (blocking with timeout)
        let data = match market_data_subscriber.receive() {
            Ok(d) => d,
            Err(_err) => {
                // No data available, check for fills to publish
                if !simulator.drain_fills().is_empty() {
                    let fills = simulator.drain_fills();
                    publish_fills(&fills, &mut fill_publisher)?;
                    fill_count += fills.len() as u64;
                }
                continue;
            }
        };

        // Try to parse as OrderBookBatchMessage (market data stream only has orderbook messages)
        if let Ok(batch) = OrderBookBatchMessage::from_bytes(&data) {
            msg_count += 1;

            // Check if this update should be processed based on sequence IDs
            if !sync_state.should_process_update(&batch) {
                // Check if we should resync (threshold of consecutive skips reached)
                if sync_state.should_resync() {
                    warn!("Attempting automatic resync - fetching fresh snapshot");

                    match mm_app::orderbook_helpers::fetch_orderbook_snapshot(&symbol, 100) {
                        Ok(fresh_snapshot) => {
                            info!("Received fresh snapshot with {} bids, {} asks", fresh_snapshot.bids.len(), fresh_snapshot.asks.len());

                            // Reset orderbook
                            orderbook = OrderBook::new(&symbol);
                            for (price_fixed, qty_fixed) in &fresh_snapshot.bids {
                                orderbook.update_bid(*price_fixed, *qty_fixed);
                            }
                            for (price_fixed, qty_fixed) in &fresh_snapshot.asks {
                                orderbook.update_ask(*price_fixed, *qty_fixed);
                            }

                            // Reset sync state
                            sync_state = OrderbookSyncState::new(fresh_snapshot.last_update_id);
                            orderbook_synchronized = false;

                            info!("Orderbook resync complete - waiting for synchronization");
                        }
                        Err(err) => {
                            warn!("Failed to fetch snapshot for resync: {err}");
                        }
                    }
                }

                continue; // Skip this update - not synced yet or sequence gap
            }

            // Apply batch update to orderbook
            orderbook.apply_batch(&batch);

            if !orderbook_synchronized && sync_state.is_synchronized() && orderbook.best_bid().is_some() && orderbook.best_ask().is_some() {
                orderbook_synchronized = true;
                let best_bid = orderbook.best_bid().unwrap();
                let best_ask = orderbook.best_ask().unwrap();
                let spread = from_fixed_point(best_ask.0) - from_fixed_point(best_bid.0);
                info!(
                    "Orderbook synchronized - ready to simulate fills | Best bid: ${:.2} x {}, Best ask: ${:.2} x {}, Spread: ${:.2}",
                    from_fixed_point(best_bid.0),
                    from_fixed_point(best_bid.1),
                    from_fixed_point(best_ask.0),
                    from_fixed_point(best_ask.1),
                    spread
                );
            }

            // Update simulator with new market data
            let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos() as u64;

            simulator.update_market_data(&orderbook, timestamp, last_trade_price);

            // Check for fills
            let fills = simulator.drain_fills();
            if !fills.is_empty() {
                publish_fills(&fills, &mut fill_publisher)?;
                fill_count += fills.len() as u64;

                for fill in &fills {
                    info!(
                        "Simulated fill: {} {} @ ${:.2}",
                        if matches!(fill.side, mm_strategy::OrderSide::Bid) { "BUY" } else { "SELL" },
                        fill.quantity.to_f64(),
                        fill.price.to_f64()
                    );
                }
            }

            if msg_count.is_multiple_of(1000) {
                if let Some(mid_i64) = orderbook.mid_price() {
                    let mid = from_fixed_point(mid_i64);
                    let position = simulator.position();
                    debug!(
                        "Processed {} OB updates, {} quotes, {} fills | Mid: ${:.2} | Active orders: {} | Position: {}",
                        msg_count,
                        quote_count,
                        fill_count,
                        mid,
                        simulator.active_order_count(),
                        position.quantity.to_f64()
                    );
                }
            }

            continue;
        }

        // Note: QuoteMessage is now handled by strategy_quotes_subscriber above
        warn!("Unknown message type received from market data stream");
    }

    info!("Shutting down simulator");
    info!("Final stats: {} OB updates, {} quotes processed, {} fills executed", msg_count, quote_count, fill_count);
    Ok(())
}

fn publish_fills(fills: &[SimulatedFill], publisher: &mut Publisher) -> Result<(), Box<dyn std::error::Error>> {
    let (symbol, encoding) = CompressedString::from_str("BTCUSDT")?;

    for fill in fills {
        let side_byte = match fill.side {
            mm_strategy::OrderSide::Bid => 0u8,
            mm_strategy::OrderSide::Ask => 1u8,
        };

        let fill_msg = OrderFillMessage::new(
            Exchange::Binance,
            symbol,
            encoding,
            fill.timestamp,
            fill.order_id,
            fill.price.to_i64(),
            fill.quantity.to_i64(),
            if side_byte == 0 { OrderSide::Bid } else { OrderSide::Ask },
            fill.is_maker,
        );

        let bytes = Bytes::from(fill_msg.to_bytes().to_vec());
        publisher.publish(bytes)?;
    }

    Ok(())
}
