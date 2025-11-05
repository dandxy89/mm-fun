use std::collections::BTreeMap;
use std::sync::Arc;

use mm_binary::CompressedString;
use mm_binary::Exchange;
use mm_binary::MarketDataMessage;
use mm_binary::messages::UpdateType;
use mm_binary::parse_json_decimal_to_fixed_point;
use simd_json::prelude::ValueAsArray;
use simd_json::prelude::ValueAsScalar;
use simd_json::prelude::ValueObjectAccess;

#[derive(Debug, Clone)]
pub struct OrderBook {
    pub symbol: Arc<str>,
    pub timestamp: u64,
    pub bids: BTreeMap<i64, i64>, // price -> quantity (fixed-point i64)
    pub asks: BTreeMap<i64, i64>,
    pub max_levels: usize,
}

impl OrderBook {
    pub fn new(symbol: &str) -> Self {
        Self { symbol: Arc::from(symbol), timestamp: 0, bids: BTreeMap::new(), asks: BTreeMap::new(), max_levels: 50 }
    }

    pub fn with_max_levels(symbol: &str, max_levels: usize) -> Self {
        Self { symbol: Arc::from(symbol), timestamp: 0, bids: BTreeMap::new(), asks: BTreeMap::new(), max_levels }
    }

    pub fn update_bid(&mut self, price: i64, quantity: i64) {
        if quantity == 0 {
            self.bids.remove(&price);
        } else {
            self.bids.insert(price, quantity);
        }
    }

    pub fn update_ask(&mut self, price: i64, quantity: i64) {
        if quantity == 0 {
            self.asks.remove(&price);
        } else {
            self.asks.insert(price, quantity);
        }
    }

    /// Apply a batch of orderbook updates from OrderBookBatchMessage
    ///
    /// This is a convenience method to avoid duplicate loops in binaries
    pub fn apply_batch(&mut self, batch: &mm_binary::OrderBookBatchMessage) {
        for bid in batch.bids() {
            let price = bid.price;
            let qty = bid.size;
            if price > 0 {
                self.update_bid(price, qty);
            }
        }

        for ask in batch.asks() {
            let price = ask.price;
            let qty = ask.size;
            if price > 0 {
                self.update_ask(price, qty);
            }
        }

        self.timestamp = batch.timestamp();
    }

    pub fn trim_book(&mut self) {
        // Trim bids (remove lowest prices)
        while self.bids.len() > self.max_levels {
            if let Some(&lowest_bid) = self.bids.keys().next() {
                self.bids.remove(&lowest_bid);
            }
        }

        // Trim asks (remove highest prices)
        while self.asks.len() > self.max_levels {
            if let Some(&highest_ask) = self.asks.keys().next_back() {
                self.asks.remove(&highest_ask);
            }
        }
    }

    pub fn best_bid(&self) -> Option<(i64, i64)> {
        self.bids.iter().next_back().map(|(p, q)| (*p, *q))
    }

    pub fn best_ask(&self) -> Option<(i64, i64)> {
        self.asks.iter().next().map(|(p, q)| (*p, *q))
    }

    pub fn mid_price(&self) -> Option<i64> {
        let (bid_price, _) = self.best_bid()?;
        let (ask_price, _) = self.best_ask()?;
        Some((bid_price + ask_price) / 2)
    }

    pub fn spread(&self) -> Option<i64> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid_price, _)), Some((ask_price, _))) => Some(ask_price - bid_price),
            _ => None,
        }
    }

    /// Get top N bid levels
    pub fn top_bids(&self, n: usize) -> Vec<(i64, i64)> {
        self.bids.iter().rev().take(n).map(|(p, q)| (*p, *q)).collect()
    }

    /// Get top N ask levels
    pub fn top_asks(&self, n: usize) -> Vec<(i64, i64)> {
        self.asks.iter().take(n).map(|(p, q)| (*p, *q)).collect()
    }
}

/// Converts Binance JSON orderbook update to binary message
pub fn json_to_binary(json_data: &str) -> Result<MarketDataMessage, Box<dyn std::error::Error>> {
    let mut bytes = json_data.as_bytes().to_vec();
    let parsed = simd_json::to_borrowed_value(&mut bytes)?;

    // Extract fields from Binance format
    let symbol_str = parsed["s"].as_str().ok_or("Missing symbol")?;
    let timestamp = parsed["E"].as_u64().ok_or("Missing timestamp")?;

    // Encode symbol
    let (symbol, encoding) = CompressedString::from_str(symbol_str)?;

    // Get best bid and ask from the update
    let bids = parsed["b"].as_array().ok_or("Missing bids")?;
    let asks = parsed["a"].as_array().ok_or("Missing asks")?;

    let (bid_price, bid_size) = if let Some(first_bid) = bids.first() {
        let bid_array = first_bid.as_array().ok_or("Invalid bid format")?;
        let price_str = bid_array[0].as_str().ok_or("Invalid bid price")?;
        let size_str = bid_array[1].as_str().ok_or("Invalid bid size")?;
        let price = parse_json_decimal_to_fixed_point(price_str.as_bytes())?;
        let size = parse_json_decimal_to_fixed_point(size_str.as_bytes())?;
        (price, size)
    } else {
        (0, 0)
    };

    let (ask_price, ask_size) = if let Some(first_ask) = asks.first() {
        let ask_array = first_ask.as_array().ok_or("Invalid ask format")?;
        let price_str = ask_array[0].as_str().ok_or("Invalid ask price")?;
        let size_str = ask_array[1].as_str().ok_or("Invalid ask size")?;
        let price = parse_json_decimal_to_fixed_point(price_str.as_bytes())?;
        let size = parse_json_decimal_to_fixed_point(size_str.as_bytes())?;
        (price, size)
    } else {
        (0, 0)
    };

    // Determine update type based on presence of 'u' field (update ID)
    let update_type = if parsed.get("u").is_some() { UpdateType::Update } else { UpdateType::Snapshot };

    Ok(MarketDataMessage::new(Exchange::Binance, update_type, symbol, encoding, timestamp, bid_price, ask_price, bid_size, ask_size))
}

/// Process orderbook update from JSON and apply to orderbook
pub fn process_orderbook_update(orderbook: &mut OrderBook, json_data: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = json_data.as_bytes().to_vec();
    let parsed = simd_json::to_borrowed_value(&mut bytes)?;

    // Update timestamp
    if let Some(timestamp) = parsed["E"].as_u64() {
        orderbook.timestamp = timestamp;
    }

    // Process bids
    if let Some(bids) = parsed["b"].as_array() {
        for bid in bids {
            if let Some(bid_array) = bid.as_array() {
                if bid_array.len() >= 2 {
                    let price_str = bid_array[0].as_str().ok_or("Invalid bid price")?;
                    let quantity_str = bid_array[1].as_str().ok_or("Invalid bid quantity")?;
                    let price = parse_json_decimal_to_fixed_point(price_str.as_bytes())?;
                    let quantity = parse_json_decimal_to_fixed_point(quantity_str.as_bytes())?;
                    orderbook.update_bid(price, quantity);
                }
            }
        }
    }

    // Process asks
    if let Some(asks) = parsed["a"].as_array() {
        for ask in asks {
            if let Some(ask_array) = ask.as_array() {
                if ask_array.len() >= 2 {
                    let price_str = ask_array[0].as_str().ok_or("Invalid ask price")?;
                    let quantity_str = ask_array[1].as_str().ok_or("Invalid ask quantity")?;
                    let price = parse_json_decimal_to_fixed_point(price_str.as_bytes())?;
                    let quantity = parse_json_decimal_to_fixed_point(quantity_str.as_bytes())?;
                    orderbook.update_ask(price, quantity);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use mm_binary::FIXED_POINT_MULTIPLIER;

    use super::*;

    #[test]
    fn test_orderbook() {
        let mut ob = OrderBook::new("BTCUSDT");

        // Use fixed-point i64 values (8 decimal places)
        let price_50000 = 50_000 * FIXED_POINT_MULTIPLIER;
        let price_49999 = 49_999 * FIXED_POINT_MULTIPLIER;
        let price_50001 = 50_001 * FIXED_POINT_MULTIPLIER;
        let price_50002 = 50_002 * FIXED_POINT_MULTIPLIER;
        let qty_1_5 = 150_000_000; // 1.5 * 100_000_000
        let qty_2_0 = 200_000_000; // 2.0 * 100_000_000
        let qty_1_0 = 100_000_000; // 1.0 * 100_000_000
        let qty_0_5 = 50_000_000; // 0.5 * 100_000_000

        ob.update_bid(price_50000, qty_1_5);
        ob.update_bid(price_49999, qty_2_0);
        ob.update_ask(price_50001, qty_1_0);
        ob.update_ask(price_50002, qty_0_5);

        assert_eq!(ob.best_bid(), Some((price_50000, qty_1_5)));
        assert_eq!(ob.best_ask(), Some((price_50001, qty_1_0)));
        // Mid price: (50000 + 50001) / 2 = 50000.5
        assert_eq!(ob.mid_price(), Some((price_50000 + price_50001) / 2));
        assert_eq!(ob.spread(), Some(price_50001 - price_50000));
    }

    #[test]
    fn test_orderbook_updates() {
        let mut ob = OrderBook::new("BTCUSDT");

        let price_50000 = 50_000 * FIXED_POINT_MULTIPLIER;
        let qty_1_5 = 150_000_000;
        let qty_2_0 = 200_000_000;

        ob.update_bid(price_50000, qty_1_5);
        assert_eq!(ob.best_bid(), Some((price_50000, qty_1_5)));

        // Update quantity
        ob.update_bid(price_50000, qty_2_0);
        assert_eq!(ob.best_bid(), Some((price_50000, qty_2_0)));

        // Remove level
        ob.update_bid(price_50000, 0);
        assert_eq!(ob.best_bid(), None);
    }
}
