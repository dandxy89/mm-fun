use std::collections::BTreeMap;
use std::sync::Arc;

use mm_binary::CompressedString;
use mm_binary::Exchange;
use mm_binary::MarketDataMessage;
use mm_binary::messages::UpdateType;
use mm_binary::to_fixed_point;
use ordered_float::OrderedFloat;
use simd_json::prelude::ValueAsArray;
use simd_json::prelude::ValueAsScalar;
use simd_json::prelude::ValueObjectAccess;

#[derive(Debug, Clone)]
pub struct OrderBook {
    pub symbol: Arc<str>,
    pub timestamp: u64,
    pub bids: BTreeMap<OrderedFloat<f64>, f64>, // price -> quantity
    pub asks: BTreeMap<OrderedFloat<f64>, f64>,
    pub max_levels: usize,
}

impl OrderBook {
    pub fn new(symbol: &str) -> Self {
        Self { symbol: Arc::from(symbol), timestamp: 0, bids: BTreeMap::new(), asks: BTreeMap::new(), max_levels: 50 }
    }

    pub fn with_max_levels(symbol: &str, max_levels: usize) -> Self {
        Self { symbol: Arc::from(symbol), timestamp: 0, bids: BTreeMap::new(), asks: BTreeMap::new(), max_levels }
    }

    pub fn update_bid(&mut self, price: f64, quantity: f64) {
        let ordered_price = OrderedFloat(price);
        if quantity == 0.0 {
            self.bids.remove(&ordered_price);
        } else {
            self.bids.insert(ordered_price, quantity);
        }
    }

    pub fn update_ask(&mut self, price: f64, quantity: f64) {
        let ordered_price = OrderedFloat(price);
        if quantity == 0.0 {
            self.asks.remove(&ordered_price);
        } else {
            self.asks.insert(ordered_price, quantity);
        }
    }

    pub fn trim_book(&mut self) {
        // Trim bids
        while self.bids.len() > self.max_levels {
            if let Some((&lowest_bid, _)) = self.bids.iter().next() {
                self.bids.remove(&lowest_bid);
            }
        }

        // Trim asks
        while self.asks.len() > self.max_levels {
            if let Some((&highest_ask, _)) = self.asks.iter().next_back() {
                self.asks.remove(&highest_ask);
            }
        }
    }

    pub fn best_bid(&self) -> Option<(f64, f64)> {
        self.bids.iter().next_back().map(|(p, q)| (p.0, *q))
    }

    pub fn best_ask(&self) -> Option<(f64, f64)> {
        self.asks.iter().next().map(|(p, q)| (p.0, *q))
    }

    pub fn mid_price(&self) -> Option<f64> {
        Some((self.best_bid()?.0 + self.best_ask()?.0) / 2.0)
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid_price, _)), Some((ask_price, _))) => Some(ask_price - bid_price),
            _ => None,
        }
    }

    /// Get top N bid levels
    pub fn top_bids(&self, n: usize) -> Vec<(f64, f64)> {
        self.bids.iter().rev().take(n).map(|(p, q)| (p.0, *q)).collect()
    }

    /// Get top N ask levels
    pub fn top_asks(&self, n: usize) -> Vec<(f64, f64)> {
        self.asks.iter().take(n).map(|(p, q)| (p.0, *q)).collect()
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
        let price = bid_array[0].as_str().ok_or("Invalid bid price")?.parse::<f64>()?;
        let size = bid_array[1].as_str().ok_or("Invalid bid size")?.parse::<f64>()?;
        (to_fixed_point(price), to_fixed_point(size))
    } else {
        (0, 0)
    };

    let (ask_price, ask_size) = if let Some(first_ask) = asks.first() {
        let ask_array = first_ask.as_array().ok_or("Invalid ask format")?;
        let price = ask_array[0].as_str().ok_or("Invalid ask price")?.parse::<f64>()?;
        let size = ask_array[1].as_str().ok_or("Invalid ask size")?.parse::<f64>()?;
        (to_fixed_point(price), to_fixed_point(size))
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
            if let Some(bid_array) = bid.as_array()
                && bid_array.len() >= 2
            {
                let price = bid_array[0].as_str().ok_or("Invalid bid price")?.parse::<f64>()?;
                let quantity = bid_array[1].as_str().ok_or("Invalid bid quantity")?.parse::<f64>()?;
                orderbook.update_bid(price, quantity);
            }
        }
    }

    // Process asks
    if let Some(asks) = parsed["a"].as_array() {
        for ask in asks {
            if let Some(ask_array) = ask.as_array()
                && ask_array.len() >= 2
            {
                let price = ask_array[0].as_str().ok_or("Invalid ask price")?.parse::<f64>()?;
                let quantity = ask_array[1].as_str().ok_or("Invalid ask quantity")?.parse::<f64>()?;
                orderbook.update_ask(price, quantity);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orderbook() {
        let mut ob = OrderBook::new("BTCUSDT");

        ob.update_bid(50_000.0, 1.5);
        ob.update_bid(49999.0, 2.0);
        ob.update_ask(50001.0, 1.0);
        ob.update_ask(50002.0, 0.5);

        assert_eq!(ob.best_bid(), Some((50_000.0, 1.5)));
        assert_eq!(ob.best_ask(), Some((50001.0, 1.0)));
        assert_eq!(ob.mid_price(), Some(50_000.5));
        assert_eq!(ob.spread(), Some(1.0));
    }

    #[test]
    fn test_orderbook_updates() {
        let mut ob = OrderBook::new("BTCUSDT");

        ob.update_bid(50_000.0, 1.5);
        assert_eq!(ob.best_bid(), Some((50_000.0, 1.5)));

        // Update quantity
        ob.update_bid(50_000.0, 2.0);
        assert_eq!(ob.best_bid(), Some((50_000.0, 2.0)));

        // Remove level
        ob.update_bid(50_000.0, 0.0);
        assert_eq!(ob.best_bid(), None);
    }
}
