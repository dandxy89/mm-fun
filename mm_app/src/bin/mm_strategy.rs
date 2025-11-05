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
use mm_binary::OrderBookBatchMessage;
use mm_binary::from_fixed_point;
use mm_binary::messages::OrderFillMessage;
use mm_binary::messages::PositionMessage;
use mm_binary::messages::QuoteMessage;
use mm_binary::messages::TradeMessage;
use mm_orderbook::OrderBook;
use mm_strategy::FixedPoint;
use mm_strategy::MarketState;
use mm_strategy::drift_estimator::Trade;
use mm_strategy::quote_engine::QuoteEngine;
use tracing::debug;
use tracing::info;
use tracing::warn;

const HEARTBEAT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let _guard = mm_app::tracing_setup::init_with_stdout("mm_strategy", "./logs", tracing::Level::INFO);

    // Load strategy configuration from file (with fallback to defaults)
    let config_file = config_loader::load_strategy_config_or_default("config/strategy.toml");
    let symbol = cli::get_symbol_uppercase(&config_file.symbol);
    let config = config_file.strategy;
    let quote_publish_interval = Duration::from_millis(config_file.quote_publish_interval_ms.unwrap_or(100));

    info!("Starting market making strategy for {symbol}");
    info!(
        "Strategy config: min_spread={}bps, max_position={}, base_size={}",
        config.min_spread_bps, config.max_position_size, config.base_quote_size
    );

    // Initialize quote engine
    let mut quote_engine = QuoteEngine::new(config);

    // Connect to Aeron - need separate subscribers for each stream!
    let mut market_data_subscriber = Subscriber::new();
    market_data_subscriber.add_subscription(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID)?;
    info!("Subscribed to market data on stream {}", aeron_config::MARKET_DATA_STREAM_ID);

    // Subscribe to trade data (TODO: implement in collector)
    // For now, we'll work without trade data and rely on orderbook imbalance

    // Subscribe to order fills from simulator
    let mut order_fills_subscriber = Subscriber::new();
    order_fills_subscriber.add_subscription(aeron_config::ORDER_FILLS_CHANNEL, aeron_config::ORDER_FILLS_STREAM_ID)?;
    info!("Subscribed to order fills on stream {}", aeron_config::ORDER_FILLS_STREAM_ID);

    // Create publishers
    let mut quote_publisher = Publisher::new();
    quote_publisher.add_publication(aeron_config::STRATEGY_QUOTES_CHANNEL, aeron_config::STRATEGY_QUOTES_STREAM_ID)?;
    info!("Publishing quotes on stream {}", aeron_config::STRATEGY_QUOTES_STREAM_ID);

    let mut position_publisher = Publisher::new();
    position_publisher.add_publication(aeron_config::POSITION_CHANNEL, aeron_config::POSITION_STREAM_ID)?;
    info!("Publishing position updates on stream {}", aeron_config::POSITION_STREAM_ID);

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
    info!("Starting main strategy loop");
    let mut msg_count = 0u64;
    let mut last_heartbeat_check = Instant::now();
    let mut last_quote_publish = Instant::now();
    let mut orderbook_synchronized = false;
    let mut last_trade_price: Option<FixedPoint> = None;

    while running.load(Ordering::Relaxed) {
        // Check heartbeat
        if last_heartbeat_check.elapsed() > HEARTBEAT_CHECK_INTERVAL {
            let heartbeat_stale = monitoring::is_heartbeat_stale(&last_heartbeat_timestamp, aeron_config::HEARTBEAT_TIMEOUT_MS);

            // Kill strategy if heartbeat is stale
            if heartbeat_stale {
                warn!("Heartbeat stale, killing strategy");
                quote_engine.risk_manager_mut().kill("Heartbeat timeout".to_string());
            }

            last_heartbeat_check = Instant::now();
        }

        // Try to receive from order fills subscriber first (non-blocking)
        if let Ok(Some(data)) = order_fills_subscriber.try_receive() {
            // Try to parse as OrderFillMessage
            if let Ok(fill_msg) = OrderFillMessage::from_bytes(&data) {
                info!(
                    "Received fill: {} {} @ {} ({})",
                    if fill_msg.side == 0 { "BUY" } else { "SELL" },
                    from_fixed_point(fill_msg.fill_quantity),
                    from_fixed_point(fill_msg.fill_price),
                    if fill_msg.is_maker != 0 { "maker" } else { "taker" }
                );

                // Update position tracker
                let fill_side = if fill_msg.side == 0 { mm_strategy::OrderSide::Bid } else { mm_strategy::OrderSide::Ask };
                let mut position = *quote_engine.inventory_manager_mut().position();
                position.apply_fill(fill_side, FixedPoint(fill_msg.fill_price), FixedPoint(fill_msg.fill_quantity));
                quote_engine.inventory_manager_mut().update_position(position);

                // Publish position update
                if let Err(err) = publish_position(&position, &orderbook, &mut position_publisher) {
                    warn!("Failed to publish position update: {err}");
                }

                info!(
                    "Position updated: {} @ avg ${:.2} | Realized PnL: ${:.2}",
                    position.quantity.to_f64(),
                    position.avg_entry_price.to_f64(),
                    position.realized_pnl.to_f64()
                );
            } else {
                warn!("Failed to parse order fill message");
            }
            continue;
        }

        // Receive messages from market data subscriber (blocking with timeout)
        let data = match market_data_subscriber.receive() {
            Ok(d) => d,
            Err(_err) => {
                // No data available, check if we should publish quotes
                if orderbook_synchronized && last_quote_publish.elapsed() > quote_publish_interval {
                    publish_quotes(&mut quote_engine, &orderbook, last_trade_price, &mut quote_publisher)?;
                    last_quote_publish = Instant::now();
                }
                continue;
            }
        };

        // Try to parse as OrderBookBatchMessage first
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
                info!("Orderbook synchronized - starting quote generation");
            }

            if msg_count.is_multiple_of(1000) {
                if let (Some(mid_i64), Some(spread_i64)) = (orderbook.mid_price(), orderbook.spread()) {
                    let mid = from_fixed_point(mid_i64);
                    let spread = from_fixed_point(spread_i64);
                    let spread_bps = (spread / mid) * 10000.0;
                    debug!(
                        "Processed {} messages | Mid: ${:.2} | Spread: {:.2}bps | Inventory: {}",
                        msg_count,
                        mid,
                        spread_bps,
                        quote_engine.inventory_manager_mut().inventory().to_f64()
                    );
                }
            }

            continue;
        }

        // Try to parse as TradeMessage (can appear in market data stream)
        if let Ok(trade_msg) = TradeMessage::from_bytes(&data) {
            let trade = Trade {
                timestamp: trade_msg.timestamp,
                price: FixedPoint(trade_msg.price),
                quantity: FixedPoint(trade_msg.quantity),
                side: trade_msg.trade_side(),
                is_aggressor: trade_msg.is_aggressor != 0,
            };

            last_trade_price = Some(FixedPoint(trade_msg.price));
            quote_engine.drift_estimator_mut().add_trade(trade);
            continue;
        }

        // Note: OrderFillMessage is now handled by order_fills_subscriber above
        warn!("Unknown message type received from market data stream");
    }

    info!("Shutting down strategy");
    Ok(())
}

fn publish_quotes(
    quote_engine: &mut QuoteEngine,
    orderbook: &OrderBook,
    last_trade_price: Option<FixedPoint>,
    publisher: &mut Publisher,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build market state
    let default_price = 50_000 * mm_binary::FIXED_POINT_MULTIPLIER;
    let (bid_price, bid_volume) = orderbook.best_bid().unwrap_or((default_price, 0));
    let (ask_price, ask_volume) = orderbook.best_ask().unwrap_or((default_price, 0));

    let state = MarketState {
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos() as u64,
        bid_price: FixedPoint(bid_price),
        ask_price: FixedPoint(ask_price),
        bid_volume: FixedPoint(bid_volume),
        ask_volume: FixedPoint(ask_volume),
        last_trade_price,
        last_trade_size: None,
    };

    // Generate quote
    if let Some(quote) = quote_engine.generate_quotes(&state) {
        // Create and publish QuoteMessage
        let (symbol, encoding) = CompressedString::from_str("BTCUSDT")?;

        let quote_msg = QuoteMessage::new(
            0, // strategy_id
            symbol,
            encoding,
            quote.timestamp,
            quote.bid_price.to_i64(),
            quote.bid_size.to_i64(),
            quote.ask_price.to_i64(),
            quote.ask_size.to_i64(),
            quote.fair_value.to_i64(),
            quote.inventory.to_i64(),
            mm_binary::to_fixed_point(quote.confidence),
        );

        let bytes = Bytes::from(quote_msg.to_bytes().to_vec());
        publisher.publish(bytes)?;

        debug!(
            "Published quote: bid ${:.2} x {} | ask ${:.2} x {} | fv ${:.2} | conf {:.2}",
            quote.bid_price.to_f64(),
            quote.bid_size.to_f64(),
            quote.ask_price.to_f64(),
            quote.ask_size.to_f64(),
            quote.fair_value.to_f64(),
            quote.confidence
        );
    }

    Ok(())
}

fn publish_position(
    position: &mm_strategy::Position,
    orderbook: &OrderBook,
    publisher: &mut Publisher,
) -> Result<(), Box<dyn std::error::Error>> {
    let default_mid = 50_000 * mm_binary::FIXED_POINT_MULTIPLIER;
    let mark_price_i64 = orderbook.mid_price().unwrap_or(default_mid);
    let unrealized_pnl = position.unrealized_pnl(FixedPoint(mark_price_i64));

    let (symbol, encoding) = CompressedString::from_str("BTCUSDT")?;

    let pos_msg = PositionMessage::new(
        symbol,
        encoding,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos() as u64,
        position.quantity.to_i64(),
        position.avg_entry_price.to_i64(),
        unrealized_pnl.to_i64(),
        position.realized_pnl.to_i64(),
    );

    let bytes = Bytes::from(pos_msg.to_bytes().to_vec());
    publisher.publish(bytes)?;

    Ok(())
}
