use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use mm_aeron::Publisher;
use mm_aeron::Subscriber;
use mm_app::aeron_config;
use mm_app::shutdown_handler;
use mm_backtest::BacktestConfig;
use mm_backtest::HistoricalDataStream;
use mm_backtest::loader;
use mm_backtest::metrics::PerformanceTracker;
use mm_backtest::replay::DataReplayEngine;
use mm_binary::OrderBookBatchMessage;
use mm_binary::from_fixed_point;
use mm_binary::messages::OrderFillMessage;
use mm_binary::messages::PositionMessage;
use mm_orderbook::OrderBook;
use mm_strategy::FixedPoint;
use mm_strategy::Position;
use time::OffsetDateTime;
use time::macros::datetime;
use tracing::error;
use tracing::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let _guard = mm_app::tracing_setup::init_with_stdout("mm_backtest", "./logs", tracing::Level::INFO);

    info!("=== Market Making Backtest ===");

    // Load backtest configuration
    let config = BacktestConfig {
        symbol: "BTCUSDT".to_string(),
        start_time: datetime!(2025-01-01 0:00 UTC),
        end_time: datetime!(2025-01-02 0:00 UTC),
        replay_speed: 100.0, // 100x speed
        initial_capital: 10000.0,
        data_dir: "./data".to_string(),
    };

    info!("Configuration:");
    info!("  Symbol: {}", config.symbol);
    info!("  Period: {} to {}", config.start_time, config.end_time);
    info!("  Replay speed: {}x", config.replay_speed);
    info!("  Initial capital: ${:.2}", config.initial_capital);
    info!("  Data directory: {}", config.data_dir);

    // Load historical data
    info!("Loading historical data...");
    let events = loader::load_historical_data(&config.data_dir, &config.symbol, config.start_time, config.end_time)?;

    if events.is_empty() {
        error!("No data loaded. Ensure CSV files exist in {}/", config.data_dir);
        error!("Required files:");
        error!("  - {}_orderbook.csv", config.symbol.to_lowercase());
        error!("  - {}_trades.csv", config.symbol.to_lowercase());
        return Err("No historical data available".into());
    }

    info!("Loaded {} events", events.len());

    // Create data stream (will be moved to replay thread)
    let data_stream = HistoricalDataStream::new(events, config.replay_speed);

    // Set up subscribers to collect results
    let mut fill_subscriber = Subscriber::new();
    fill_subscriber.add_subscription(aeron_config::ORDER_FILLS_CHANNEL, aeron_config::ORDER_FILLS_STREAM_ID)?;

    let mut position_subscriber = Subscriber::new();
    position_subscriber.add_subscription(aeron_config::POSITION_CHANNEL, aeron_config::POSITION_STREAM_ID)?;

    // Initialize performance tracker
    let mut performance = PerformanceTracker::new(config.initial_capital);
    let mut position = Position { quantity: FixedPoint::ZERO, avg_entry_price: FixedPoint::ZERO, realized_pnl: FixedPoint::ZERO };

    // Initialize orderbook for mark price tracking
    let mut orderbook = OrderBook::new(&config.symbol);
    let mut last_mark_price = FixedPoint::from_f64(50000.0); // Default

    // Set up shutdown handler
    let running = Arc::new(AtomicBool::new(true));
    shutdown_handler::setup(Arc::clone(&running))?;

    // Start replay in separate thread (create publishers inside thread to avoid Send issues)
    let replay_running = Arc::clone(&running);
    let replay_handle = thread::spawn(move || {
        // Create publishers inside thread
        let mut orderbook_publisher = Publisher::new();
        if let Err(err) = orderbook_publisher.add_publication(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID) {
            error!("Failed to create orderbook publisher: {err}");
            return;
        }

        let mut trade_publisher = Publisher::new();
        if let Err(err) = trade_publisher.add_publication(aeron_config::TRADE_DATA_CHANNEL, aeron_config::TRADE_DATA_STREAM_ID) {
            error!("Failed to create trade publisher: {err}");
            return;
        }

        // Create replay engine
        let mut replay_engine = DataReplayEngine::new(data_stream, orderbook_publisher, trade_publisher);

        while replay_running.load(Ordering::Relaxed) && !replay_engine.is_complete() {
            match replay_engine.tick() {
                Ok(_) => {}
                Err(err) => {
                    error!("Replay error: {err}");
                    break;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        info!("Replay thread completed");
    });

    // Main collection loop
    info!("Starting backtest...");
    let start_time = std::time::Instant::now();
    let mut last_progress_log = std::time::Instant::now();

    // Subscriber for market data (to track orderbook)
    let mut market_subscriber = Subscriber::new();
    market_subscriber.add_subscription(aeron_config::MARKET_DATA_CHANNEL, aeron_config::MARKET_DATA_STREAM_ID)?;

    while running.load(Ordering::Relaxed) {
        // Update orderbook
        if let Ok(data) = market_subscriber.receive() {
            if let Ok(batch) = OrderBookBatchMessage::from_bytes(&data) {
                for bid in batch.bids() {
                    let price = bid.price;
                    let qty = bid.size;
                    if price > 0 {
                        orderbook.update_bid(price, qty);
                    }
                }
                for ask in batch.asks() {
                    let price = ask.price;
                    let qty = ask.size;
                    if price > 0 {
                        orderbook.update_ask(price, qty);
                    }
                }

                // Update mark price
                if let Some(mid) = orderbook.mid_price() {
                    last_mark_price = FixedPoint(mid);
                }
            }
        }

        // Collect fills
        if let Ok(data) = fill_subscriber.receive() {
            if let Ok(fill_msg) = OrderFillMessage::from_bytes(&data) {
                let side = if fill_msg.side == 0 { mm_strategy::OrderSide::Bid } else { mm_strategy::OrderSide::Ask };

                let price = from_fixed_point(fill_msg.fill_price);
                let quantity = from_fixed_point(fill_msg.fill_quantity);

                // Calculate PnL change
                let prev_realized = position.realized_pnl.to_f64();

                // Update position
                position.apply_fill(side, FixedPoint::from_f64(price), FixedPoint::from_f64(quantity));

                let pnl_change = position.realized_pnl.to_f64() - prev_realized;

                // Record in tracker
                performance.record_fill(fill_msg.timestamp, side, price, quantity, pnl_change);

                info!(
                    "Fill: {} {:.4} @ ${:.2} | Position: {:.4} | Realized PnL: ${:.2}",
                    if matches!(side, mm_strategy::OrderSide::Bid) { "BUY" } else { "SELL" },
                    quantity,
                    price,
                    position.quantity.to_f64(),
                    position.realized_pnl.to_f64()
                );
            }
        }

        // Collect position updates
        if let Ok(data) = position_subscriber.receive() {
            if let Ok(pos_msg) = PositionMessage::from_bytes(&data) {
                let timestamp = pos_msg.timestamp;
                let quantity = from_fixed_point(pos_msg.quantity);

                let unrealized_pnl = from_fixed_point(pos_msg.unrealized_pnl);
                let realized_pnl = from_fixed_point(pos_msg.realized_pnl);
                let total_pnl = realized_pnl + unrealized_pnl;
                let equity = config.initial_capital + total_pnl;

                performance.update_equity(timestamp, equity);
                performance.update_position(timestamp, quantity);
            }
        }

        // Log progress periodically
        if last_progress_log.elapsed() > Duration::from_secs(5) {
            let unrealized = position.unrealized_pnl(last_mark_price).to_f64();
            let realized = position.realized_pnl.to_f64();
            let total_pnl = realized + unrealized;
            let equity = config.initial_capital + total_pnl;

            info!(
                "Progress: Position={:.4}, Realized=${:.2}, Unrealized=${:.2}, Equity=${:.2}",
                position.quantity.to_f64(),
                realized,
                unrealized,
                equity
            );

            last_progress_log = std::time::Instant::now();
        }

        // Small sleep to avoid busy-waiting
        thread::sleep(Duration::from_millis(10));

        // Check if replay is done
        if !replay_handle.is_finished() {
            continue;
        } else {
            break;
        }
    }

    // Wait for replay thread
    let _ = replay_handle.join();

    let elapsed = start_time.elapsed();
    info!("Backtest completed in {:.2}s", elapsed.as_secs_f64());

    // Calculate final metrics
    info!("\n=== Calculating Performance Metrics ===");
    let metrics = performance.calculate_metrics(&position, last_mark_price);

    // Print results
    print_metrics(&metrics);

    // Save results to JSON
    let now = OffsetDateTime::now_utc();
    let format = time::format_description::parse("[year][month][day]_[hour][minute][second]").unwrap();
    let timestamp = now.format(&format).unwrap();
    let results_path = format!("backtest_results_{}.json", timestamp);
    let json = serde_json::to_string_pretty(&metrics)?;
    std::fs::write(&results_path, json)?;
    info!("Results saved to {results_path}");

    Ok(())
}

fn print_metrics(metrics: &mm_backtest::metrics::BacktestMetrics) {
    info!("\n╔═══════════════════════════════════════════╗");
    info!("║        BACKTEST PERFORMANCE SUMMARY        ║");
    info!("╠═══════════════════════════════════════════╣");
    info!("║ Duration: {:.2} seconds", metrics.duration_seconds);
    info!("║");
    info!("║ Trading Activity:");
    info!("║   Total Trades: {}", metrics.total_trades);
    info!("║   Buy Trades: {}", metrics.buy_trades);
    info!("║   Sell Trades: {}", metrics.sell_trades);
    info!("║   Total Volume: {:.4}", metrics.total_volume);
    info!("║");
    info!("║ PnL Metrics:");
    info!("║   Initial Capital: ${:.2}", metrics.initial_capital);
    info!("║   Final Capital: ${:.2}", metrics.final_capital);
    info!("║   Total PnL: ${:.2} ({:.2}%)", metrics.total_pnl, metrics.total_pnl_pct);
    info!("║   Realized PnL: ${:.2}", metrics.realized_pnl);
    info!("║   Unrealized PnL: ${:.2}", metrics.unrealized_pnl);
    info!("║");
    info!("║ Performance:");
    info!("║   Sharpe Ratio: {:.2}", metrics.sharpe_ratio);
    info!("║   Max Drawdown: ${:.2} ({:.2}%)", metrics.max_drawdown, metrics.max_drawdown_pct);
    info!("║   Win Rate: {:.2}%", metrics.win_rate);
    info!("║   Profit Factor: {:.2}", metrics.profit_factor);
    info!("║");
    info!("║ Position:");
    info!("║   Max Long: {:.4}", metrics.max_long_position);
    info!("║   Max Short: {:.4}", metrics.max_short_position);
    info!("║   Avg Size: {:.4}", metrics.avg_position_size);
    info!("║   Time in Market: {:.1}%", metrics.time_in_market_pct);
    info!("║");
    info!("║ Quoting:");
    info!("║   Total Quotes: {}", metrics.total_quotes);
    info!("║   Avg Spread: {:.2} bps", metrics.avg_spread_bps);
    info!("╚═══════════════════════════════════════════╝\n");
}
