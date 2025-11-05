use std::collections::VecDeque;

use mm_binary::messages::TradeSide;
use mm_strategy::FixedPoint;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;
use time::OffsetDateTime;

pub mod loader;
pub mod metrics;
pub mod replay;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("No data available")]
    NoData,

    #[error("Invalid timestamp")]
    InvalidTimestamp,
}

/// Historical orderbook update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookUpdate {
    pub timestamp: u64, // Nanoseconds since epoch
    pub symbol: String,
    pub bids: Vec<(f64, f64)>, // (price, quantity)
    pub asks: Vec<(f64, f64)>, // (price, quantity)
}

/// Historical trade event
#[derive(Debug, Clone)]
pub struct TradeEvent {
    pub timestamp: u64, // Nanoseconds since epoch
    pub symbol: String,
    pub trade_id: u64,
    pub price: f64,
    pub quantity: f64,
    pub side: TradeSide,
    pub is_aggressor: bool,
}

/// Unified event type for replay
#[derive(Debug, Clone)]
pub enum HistoricalEvent {
    OrderBook(OrderBookUpdate),
    Trade(TradeEvent),
}

impl HistoricalEvent {
    pub fn timestamp(&self) -> u64 {
        match self {
            HistoricalEvent::OrderBook(ob) => ob.timestamp,
            HistoricalEvent::Trade(trade) => trade.timestamp,
        }
    }
}

/// Historical data stream
pub struct HistoricalDataStream {
    events: VecDeque<HistoricalEvent>,
    current_time: u64,
    replay_speed: f64, // 1.0 = realtime, 10.0 = 10x faster
}

impl HistoricalDataStream {
    pub fn new(mut events: Vec<HistoricalEvent>, replay_speed: f64) -> Self {
        // Sort events by timestamp
        events.sort_by_key(|e| e.timestamp());

        let current_time = events.first().map(|e| e.timestamp()).unwrap_or(0);

        Self { events: events.into(), current_time, replay_speed }
    }

    /// Get next event that should be processed at current time
    pub fn next_event(&mut self) -> Option<HistoricalEvent> {
        self.events.pop_front()
    }

    /// Peek at next event without consuming
    pub fn peek_event(&self) -> Option<&HistoricalEvent> {
        self.events.front()
    }

    /// Check if there are more events
    pub fn has_more(&self) -> bool {
        !self.events.is_empty()
    }

    /// Get current replay time
    pub fn current_time(&self) -> u64 {
        self.current_time
    }

    /// Get replay speed multiplier
    pub fn replay_speed(&self) -> f64 {
        self.replay_speed
    }

    /// Get total number of remaining events
    pub fn remaining_events(&self) -> usize {
        self.events.len()
    }

    /// Get time range of remaining events
    pub fn time_range(&self) -> Option<(u64, u64)> {
        if self.events.is_empty() {
            return None;
        }

        let start = self.events.front()?.timestamp();
        let end = self.events.back()?.timestamp();
        Some((start, end))
    }
}

/// Backtest configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub symbol: String,
    #[serde(with = "time::serde::rfc3339")]
    pub start_time: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end_time: OffsetDateTime,
    pub replay_speed: f64,
    pub initial_capital: f64,
    pub data_dir: String,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        let now = OffsetDateTime::now_utc();
        let one_day_ago = now - time::Duration::days(1);
        Self {
            symbol: "BTCUSDT".to_string(),
            start_time: one_day_ago,
            end_time: now,
            replay_speed: 100.0, // 100x realtime
            initial_capital: 10000.0,
            data_dir: "./data".to_string(),
        }
    }
}

/// Fill event from backtest
#[derive(Debug, Clone)]
pub struct BacktestFill {
    pub timestamp: u64,
    pub side: mm_strategy::OrderSide,
    pub price: FixedPoint,
    pub quantity: FixedPoint,
    pub is_maker: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_historical_data_stream() {
        let events = vec![
            HistoricalEvent::Trade(TradeEvent {
                timestamp: 1000,
                symbol: "BTCUSDT".to_string(),
                trade_id: 1,
                price: 50000.0,
                quantity: 0.1,
                side: TradeSide::Buy,
                is_aggressor: true,
            }),
            HistoricalEvent::Trade(TradeEvent {
                timestamp: 2000,
                symbol: "BTCUSDT".to_string(),
                trade_id: 2,
                price: 50001.0,
                quantity: 0.2,
                side: TradeSide::Sell,
                is_aggressor: true,
            }),
        ];

        let mut stream = HistoricalDataStream::new(events, 1.0);
        assert!(stream.has_more());
        assert_eq!(stream.remaining_events(), 2);

        let event1 = stream.next_event().unwrap();
        assert_eq!(event1.timestamp(), 1000);

        let event2 = stream.next_event().unwrap();
        assert_eq!(event2.timestamp(), 2000);

        assert!(!stream.has_more());
    }
}
