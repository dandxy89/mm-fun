use std::fs::File;
use std::path::Path;

use csv::ReaderBuilder;
use mm_binary::messages::TradeSide;
use serde::Deserialize;
use time::OffsetDateTime;
use tracing::info;
use tracing::warn;

use crate::BacktestError;
use crate::HistoricalEvent;
use crate::OrderBookUpdate;
use crate::TradeEvent;

/// CSV format for orderbook data
#[derive(Debug, Deserialize)]
struct OrderBookRow {
    timestamp_ms: u64,
    symbol: String,
    bid_price_1: f64,
    bid_qty_1: f64,
    bid_price_2: Option<f64>,
    bid_qty_2: Option<f64>,
    bid_price_3: Option<f64>,
    bid_qty_3: Option<f64>,
    ask_price_1: f64,
    ask_qty_1: f64,
    ask_price_2: Option<f64>,
    ask_qty_2: Option<f64>,
    ask_price_3: Option<f64>,
    ask_qty_3: Option<f64>,
}

/// CSV format for trade data
#[derive(Debug, Deserialize)]
struct TradeRow {
    timestamp_ms: u64,
    symbol: String,
    trade_id: u64,
    price: f64,
    quantity: f64,
    is_buyer_maker: bool,
}

/// Load orderbook data from CSV file
pub fn load_orderbook_csv<P: AsRef<Path>>(path: P) -> Result<Vec<OrderBookUpdate>, BacktestError> {
    let file = File::open(path.as_ref())?;
    let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut updates = Vec::new();

    for result in reader.deserialize() {
        let row: OrderBookRow = result?;

        let mut bids = vec![(row.bid_price_1, row.bid_qty_1)];
        if let (Some(p), Some(q)) = (row.bid_price_2, row.bid_qty_2) {
            bids.push((p, q));
        }
        if let (Some(p), Some(q)) = (row.bid_price_3, row.bid_qty_3) {
            bids.push((p, q));
        }

        let mut asks = vec![(row.ask_price_1, row.ask_qty_1)];
        if let (Some(p), Some(q)) = (row.ask_price_2, row.ask_qty_2) {
            asks.push((p, q));
        }
        if let (Some(p), Some(q)) = (row.ask_price_3, row.ask_qty_3) {
            asks.push((p, q));
        }

        updates.push(OrderBookUpdate {
            timestamp: row.timestamp_ms * 1_000_000, // Convert ms to ns
            symbol: row.symbol,
            bids,
            asks,
        });
    }

    info!("Loaded {} orderbook updates from CSV", updates.len());
    Ok(updates)
}

/// Load trade data from CSV file
pub fn load_trades_csv<P: AsRef<Path>>(path: P) -> Result<Vec<TradeEvent>, BacktestError> {
    let file = File::open(path.as_ref())?;
    let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut trades = Vec::new();

    for result in reader.deserialize() {
        let row: TradeRow = result?;

        let side = if row.is_buyer_maker {
            TradeSide::Sell // Aggressor sold
        } else {
            TradeSide::Buy // Aggressor bought
        };

        trades.push(TradeEvent {
            timestamp: row.timestamp_ms * 1_000_000, // Convert ms to ns
            symbol: row.symbol,
            trade_id: row.trade_id,
            price: row.price,
            quantity: row.quantity,
            side,
            is_aggressor: true,
        });
    }

    info!("Loaded {} trades from CSV", trades.len());
    Ok(trades)
}

/// Load combined data from directory
pub fn load_historical_data<P: AsRef<Path>>(
    data_dir: P,
    symbol: &str,
    start_time: OffsetDateTime,
    end_time: OffsetDateTime,
) -> Result<Vec<HistoricalEvent>, BacktestError> {
    let mut events = Vec::new();

    let start_ms = start_time.unix_timestamp_nanos() as u64 / 1_000_000;
    let end_ms = end_time.unix_timestamp_nanos() as u64 / 1_000_000;

    // Load orderbook data
    let ob_path = data_dir.as_ref().join(format!("{}_orderbook.csv", symbol.to_lowercase()));
    if ob_path.exists() {
        match load_orderbook_csv(&ob_path) {
            Ok(updates) => {
                for update in updates {
                    let ts_ms = update.timestamp / 1_000_000;
                    if ts_ms >= start_ms && ts_ms <= end_ms {
                        events.push(HistoricalEvent::OrderBook(update));
                    }
                }
            }
            Err(err) => {
                warn!("Failed to load orderbook CSV: {err}");
            }
        }
    } else {
        warn!("Orderbook file not found: {:?}", ob_path);
    }

    // Load trade data
    let trades_path = data_dir.as_ref().join(format!("{}_trades.csv", symbol.to_lowercase()));
    if trades_path.exists() {
        match load_trades_csv(&trades_path) {
            Ok(trades) => {
                for trade in trades {
                    let ts_ms = trade.timestamp / 1_000_000;
                    if ts_ms >= start_ms && ts_ms <= end_ms {
                        events.push(HistoricalEvent::Trade(trade));
                    }
                }
            }
            Err(err) => {
                warn!("Failed to load trades CSV: {err}");
            }
        }
    } else {
        warn!("Trades file not found: {:?}", trades_path);
    }

    if events.is_empty() {
        return Err(BacktestError::NoData);
    }

    // Sort by timestamp
    events.sort_by_key(|e| e.timestamp());

    info!("Loaded {} total events from {} to {}", events.len(), start_time, end_time);

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_formats() {
        // Test that our CSV deserializers work
        let ob_csv = "timestamp_ms,symbol,bid_price_1,bid_qty_1,bid_price_2,bid_qty_2,bid_price_3,bid_qty_3,ask_price_1,ask_qty_1,ask_price_2,ask_qty_2,ask_price_3,ask_qty_3\n\
                      1000,BTCUSDT,50000.0,1.0,49999.0,2.0,49998.0,3.0,50001.0,1.0,50002.0,2.0,50003.0,3.0\n";

        let mut reader = ReaderBuilder::new().has_headers(true).from_reader(ob_csv.as_bytes());

        let row: OrderBookRow = reader.deserialize().next().unwrap().unwrap();
        assert_eq!(row.symbol, "BTCUSDT");
        assert_eq!(row.bid_price_1, 50000.0);
        assert_eq!(row.ask_price_1, 50001.0);
    }
}
