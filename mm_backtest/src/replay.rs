use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use mm_aeron::Publisher;
use mm_binary::CompressedString;
use mm_binary::Exchange;
use mm_binary::OrderBookBatchMessage;
use mm_binary::messages::TradeMessage;
use mm_binary::messages::UpdateType;
use mm_binary::to_fixed_point;
use tracing::debug;
use tracing::info;

use crate::BacktestError;
use crate::HistoricalDataStream;
use crate::HistoricalEvent;
use crate::OrderBookUpdate;
use crate::TradeEvent;

/// Replays historical data through Aeron streams
pub struct DataReplayEngine {
    data_stream: HistoricalDataStream,
    orderbook_publisher: Publisher,
    trade_publisher: Publisher,
    start_instant: Instant,
    simulated_start_time: u64,
}

impl DataReplayEngine {
    pub fn new(data_stream: HistoricalDataStream, orderbook_publisher: Publisher, trade_publisher: Publisher) -> Self {
        let simulated_start_time = data_stream.current_time();

        Self { data_stream, orderbook_publisher, trade_publisher, start_instant: Instant::now(), simulated_start_time }
    }

    /// Get current simulated time based on replay speed
    fn current_simulated_time(&self) -> u64 {
        let elapsed_real = self.start_instant.elapsed();
        let elapsed_simulated_ns = (elapsed_real.as_nanos() as f64 * self.data_stream.replay_speed()) as u64;
        self.simulated_start_time + elapsed_simulated_ns
    }

    /// Process next batch of events that should have occurred by now
    pub fn tick(&mut self) -> Result<usize, BacktestError> {
        let current_sim_time = self.current_simulated_time();
        let mut events_processed = 0;

        // Process all events up to current simulated time
        while let Some(event) = self.data_stream.peek_event() {
            if event.timestamp() <= current_sim_time {
                // Take the event
                let event = self.data_stream.next_event().unwrap();

                // Publish it
                self.publish_event(event)?;
                events_processed += 1;
            } else {
                // Future event, wait
                break;
            }
        }

        Ok(events_processed)
    }

    /// Publish a single event
    fn publish_event(&mut self, event: HistoricalEvent) -> Result<(), BacktestError> {
        match event {
            HistoricalEvent::OrderBook(ob) => self.publish_orderbook(ob)?,
            HistoricalEvent::Trade(trade) => self.publish_trade(trade)?,
        }
        Ok(())
    }

    /// Publish orderbook update
    fn publish_orderbook(&mut self, update: OrderBookUpdate) -> Result<(), BacktestError> {
        let (symbol, encoding) =
            CompressedString::from_str(&update.symbol).map_err(|e| BacktestError::Parse(format!("Symbol encoding error: {}", e)))?;

        let mut batch = OrderBookBatchMessage::new(Exchange::Binance, UpdateType::Update, symbol, encoding, update.timestamp);

        // Add bids
        for (price, qty) in &update.bids {
            if *price > 0.0 && *qty >= 0.0 {
                batch.add_bid(to_fixed_point(*price), to_fixed_point(*qty));
            }
        }

        // Add asks
        for (price, qty) in &update.asks {
            if *price > 0.0 && *qty >= 0.0 {
                batch.add_ask(to_fixed_point(*price), to_fixed_point(*qty));
            }
        }

        let bytes = Bytes::from(batch.to_bytes());
        self.orderbook_publisher.publish(bytes).map_err(|e| BacktestError::Parse(format!("Aeron publish error: {}", e)))?;

        Ok(())
    }

    /// Publish trade
    fn publish_trade(&mut self, trade: TradeEvent) -> Result<(), BacktestError> {
        let (symbol, encoding) =
            CompressedString::from_str(&trade.symbol).map_err(|e| BacktestError::Parse(format!("Symbol encoding error: {}", e)))?;

        let trade_msg = TradeMessage::new(
            Exchange::Binance,
            symbol,
            encoding,
            trade.timestamp,
            trade.trade_id,
            to_fixed_point(trade.price),
            to_fixed_point(trade.quantity),
            trade.side,
            trade.is_aggressor,
        );

        let bytes = Bytes::from(trade_msg.to_bytes().to_vec());
        self.trade_publisher.publish(bytes).map_err(|e| BacktestError::Parse(format!("Aeron publish error: {}", e)))?;

        Ok(())
    }

    /// Run replay until completion
    pub fn run(&mut self) -> Result<(), BacktestError> {
        info!("Starting replay with speed {}x", self.data_stream.replay_speed());

        let mut total_events = 0;
        let mut last_log = Instant::now();

        while self.data_stream.has_more() {
            let processed = self.tick()?;
            total_events += processed;

            // Log progress every second
            if last_log.elapsed() > Duration::from_secs(1) {
                let remaining = self.data_stream.remaining_events();
                let progress =
                    if total_events + remaining > 0 { (total_events as f64 / (total_events + remaining) as f64) * 100.0 } else { 0.0 };

                debug!("Replay progress: {:.1}% ({} events processed, {} remaining)", progress, total_events, remaining);
                last_log = Instant::now();
            }

            // Small sleep to avoid busy-waiting
            std::thread::sleep(Duration::from_micros(100));
        }

        info!("Replay complete: {total_events} total events processed");
        Ok(())
    }

    /// Check if replay is complete
    pub fn is_complete(&self) -> bool {
        !self.data_stream.has_more()
    }

    /// Get progress percentage
    pub fn progress(&self) -> f64 {
        if let Some((start, end)) = self.data_stream.time_range() {
            let current = self.current_simulated_time();
            if current >= end {
                return 100.0;
            }
            ((current - start) as f64 / (end - start) as f64) * 100.0
        } else {
            100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use mm_binary::messages::TradeSide;

    use super::*;
    use crate::TradeEvent;

    #[test]
    fn test_simulated_time() {
        let events = vec![HistoricalEvent::Trade(TradeEvent {
            timestamp: 1_000_000_000,
            symbol: "BTCUSDT".to_string(),
            trade_id: 1,
            price: 50000.0,
            quantity: 0.1,
            side: TradeSide::Buy,
            is_aggressor: true,
        })];

        let stream = HistoricalDataStream::new(events, 10.0);
        let ob_pub = Publisher::new();
        let trade_pub = Publisher::new();

        let engine = DataReplayEngine::new(stream, ob_pub, trade_pub);

        // Simulated time should start at first event
        assert_eq!(engine.simulated_start_time, 1_000_000_000);
    }
}
