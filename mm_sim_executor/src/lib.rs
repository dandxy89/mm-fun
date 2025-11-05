use std::collections::HashMap;

use mm_orderbook::OrderBook;
use mm_strategy::FixedPoint;
use mm_strategy::OrderSide;
use mm_strategy::Position;
use mm_strategy::StrategyQuote;
use tracing::info;

/// Simulated order in the order book
#[derive(Debug, Clone)]
pub struct SimulatedOrder {
    pub order_id: u64,
    pub side: OrderSide,
    pub price: FixedPoint,
    pub remaining_quantity: FixedPoint,
    pub original_quantity: FixedPoint,
    pub timestamp: u64,
}

/// Fill event from simulation
#[derive(Debug, Clone, Copy)]
pub struct SimulatedFill {
    pub order_id: u64,
    pub side: OrderSide,
    pub price: FixedPoint,
    pub quantity: FixedPoint,
    pub is_maker: bool,
    pub timestamp: u64,
}

/// Configuration for simulation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimulatorConfig {
    /// Latency for order placement (microseconds)
    pub order_placement_latency_us: u64,

    /// Latency for order cancellation (microseconds)
    pub order_cancellation_latency_us: u64,

    /// Fill probability factor (0.0 to 1.0)
    /// Lower = more conservative fills
    pub fill_probability_factor: f64,

    /// Enable queue position tracking
    pub track_queue_position: bool,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            order_placement_latency_us: 10_000,   // 10ms
            order_cancellation_latency_us: 5_000, // 5ms
            fill_probability_factor: 0.8,         // Conservative fills
            track_queue_position: false,          // Simplified for now
        }
    }
}

/// Order book simulator
/// Maintains virtual orders and simulates fills based on market data
pub struct OrderBookSimulator {
    config: SimulatorConfig,
    next_order_id: u64,
    active_orders: HashMap<u64, SimulatedOrder>,
    position: Position,
    fills: Vec<SimulatedFill>,
}

impl OrderBookSimulator {
    pub fn new(config: SimulatorConfig) -> Self {
        Self { config, next_order_id: 1, active_orders: HashMap::new(), position: Position::new(), fills: Vec::new() }
    }

    /// Get current position
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Get fills since last check
    pub fn drain_fills(&mut self) -> Vec<SimulatedFill> {
        std::mem::take(&mut self.fills)
    }

    /// Place orders from strategy quote
    pub fn place_orders_from_quote(&mut self, quote: &StrategyQuote, timestamp: u64) -> Vec<u64> {
        let mut order_ids = Vec::new();

        // Place bid order
        if quote.bid_size > FixedPoint::ZERO {
            let order_id = self.place_order(OrderSide::Bid, quote.bid_price, quote.bid_size, timestamp);
            order_ids.push(order_id);
        }

        // Place ask order
        if quote.ask_size > FixedPoint::ZERO {
            let order_id = self.place_order(OrderSide::Ask, quote.ask_price, quote.ask_size, timestamp);
            order_ids.push(order_id);
        }

        order_ids
    }

    /// Place a single order
    pub fn place_order(&mut self, side: OrderSide, price: FixedPoint, quantity: FixedPoint, timestamp: u64) -> u64 {
        let order_id = self.next_order_id;
        self.next_order_id += 1;

        let active_at_ns = timestamp + self.config.order_placement_latency_us * 1000;

        let order =
            SimulatedOrder { order_id, side, price, remaining_quantity: quantity, original_quantity: quantity, timestamp: active_at_ns };

        self.active_orders.insert(order_id, order);

        info!(
            "Placed order: id={}, side={:?}, price=${:.2}, qty={:.4}, active_at={}ns",
            order_id,
            side,
            price.to_f64(),
            quantity.to_f64(),
            active_at_ns
        );

        order_id
    }

    /// Cancel an order
    pub fn cancel_order(&mut self, order_id: u64) -> bool {
        let was_cancelled = self.active_orders.remove(&order_id).is_some();
        if was_cancelled {
            info!("Cancelled order: id={}", order_id);
        }
        was_cancelled
    }

    /// Cancel all orders
    pub fn cancel_all_orders(&mut self) {
        let count = self.active_orders.len();
        self.active_orders.clear();
        if count > 0 {
            info!("Cancelled all orders: count={}", count);
        }
    }

    /// Update simulation with new market data
    /// Checks if any orders would be filled based on market movement
    pub fn update_market_data(&mut self, orderbook: &OrderBook, timestamp: u64, last_trade_price: Option<FixedPoint>) {
        let best_bid = orderbook.best_bid().map(|(price, _qty)| FixedPoint(price));
        let best_ask = orderbook.best_ask().map(|(price, _qty)| FixedPoint(price));

        // Log market data update
        let spread = if let (Some(bid), Some(ask)) = (best_bid, best_ask) { ask.to_f64() - bid.to_f64() } else { 0.0 };

        // Debug: Log top 3 bids and asks from the orderbook
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let top_bids = orderbook.top_bids(3);
            let top_asks = orderbook.top_asks(3);
            info!(
                "DEBUG - Top 3 bids from orderbook: {:?}",
                top_bids.iter().map(|(p, q)| (FixedPoint(*p).to_f64(), FixedPoint(*q).to_f64())).collect::<Vec<_>>()
            );
            info!(
                "DEBUG - Top 3 asks from orderbook: {:?}",
                top_asks.iter().map(|(p, q)| (FixedPoint(*p).to_f64(), FixedPoint(*q).to_f64())).collect::<Vec<_>>()
            );
        }

        info!(
            "Market update: best_bid=${}, best_ask=${}, spread=${:.2}, active_orders={}",
            best_bid.map_or("None".to_string(), |p| format!("{:.2}", p.to_f64())),
            best_ask.map_or("None".to_string(), |p| format!("{:.2}", p.to_f64())),
            spread,
            self.active_orders.len()
        );

        let mut orders_to_remove = Vec::new();

        for (order_id, order) in &mut self.active_orders {
            // Order not yet active due to latency
            if timestamp < order.timestamp {
                info!(
                    "Order {} not yet active (current={}ns, active_at={}ns, pending={}ns)",
                    order_id,
                    timestamp,
                    order.timestamp,
                    order.timestamp - timestamp
                );
                continue;
            }

            // Check if order would be filled
            let should_fill = match order.side {
                OrderSide::Bid => {
                    // Our bid gets filled if market trades at or below our price
                    if let Some(trade_price) = last_trade_price {
                        trade_price.0 <= order.price.0
                    } else if let Some(ask) = best_ask {
                        // Or if best ask crosses our bid
                        ask.0 <= order.price.0
                    } else {
                        false
                    }
                }
                OrderSide::Ask => {
                    // Our ask gets filled if market trades at or above our price
                    if let Some(trade_price) = last_trade_price {
                        trade_price.0 >= order.price.0
                    } else if let Some(bid) = best_bid {
                        // Or if best bid crosses our ask
                        bid.0 >= order.price.0
                    } else {
                        false
                    }
                }
            };

            if should_fill {
                // Determine fill quantity (could be partial)
                let fill_quantity = Self::calculate_fill_quantity(self.config.fill_probability_factor, order, orderbook);

                if fill_quantity > FixedPoint::ZERO {
                    // Create fill event
                    let fill = SimulatedFill {
                        order_id: *order_id,
                        side: order.side,
                        price: order.price,
                        quantity: fill_quantity,
                        is_maker: true, // All our orders are maker orders in this simulation
                        timestamp,
                    };

                    self.fills.push(fill);

                    // Update position
                    self.position.apply_fill(order.side, order.price, fill_quantity);

                    info!(
                        "Position updated: qty={:.4}, avg_entry=${:.2}, realized_pnl=${:.2}",
                        self.position.quantity.to_f64(),
                        self.position.avg_entry_price.to_f64(),
                        self.position.realized_pnl.to_f64()
                    );

                    // Update remaining quantity
                    order.remaining_quantity = FixedPoint(order.remaining_quantity.0 - fill_quantity.0);

                    let is_full_fill = order.remaining_quantity.0 <= 0;
                    let fill_type = if is_full_fill { "Full" } else { "Partial" };

                    info!(
                        "{} fill: order_id={}, side={:?}, price=${:.2}, qty={:.4}, remaining={:.4}",
                        fill_type,
                        order_id,
                        order.side,
                        order.price.to_f64(),
                        fill_quantity.to_f64(),
                        order.remaining_quantity.to_f64()
                    );

                    if is_full_fill {
                        orders_to_remove.push(*order_id);
                    }
                }
            }
        }

        // Remove fully filled orders
        for order_id in orders_to_remove {
            self.active_orders.remove(&order_id);
            info!("Removed fully filled order: id={}", order_id);
        }
    }

    /// Calculate fill quantity for an order
    /// In a real simulation, this would consider queue position, order book depth, etc.
    fn calculate_fill_quantity(fill_probability_factor: f64, order: &SimulatedOrder, _orderbook: &OrderBook) -> FixedPoint {
        // Simplified: fill a fraction of remaining quantity based on probability
        let fill_qty = order.remaining_quantity.to_f64() * fill_probability_factor;

        FixedPoint::from_f64(fill_qty)
    }

    /// Get active order count
    pub fn active_order_count(&self) -> usize {
        self.active_orders.len()
    }

    /// Get active orders
    pub fn active_orders(&self) -> &HashMap<u64, SimulatedOrder> {
        &self.active_orders
    }
}

/// Latency simulator
/// Models realistic order placement and cancellation delays
pub struct LatencySimulator {
    placement_latency_us: u64,
    cancellation_latency_us: u64,
}

impl LatencySimulator {
    pub fn new(placement_latency_us: u64, cancellation_latency_us: u64) -> Self {
        Self { placement_latency_us, cancellation_latency_us }
    }

    /// Calculate when order will be active
    pub fn order_active_time(&self, submission_time: u64) -> u64 {
        submission_time + self.placement_latency_us * 1000 // Convert to nanos
    }

    /// Calculate when cancellation will be effective
    pub fn cancellation_effective_time(&self, cancellation_time: u64) -> u64 {
        cancellation_time + self.cancellation_latency_us * 1000 // Convert to nanos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulator_creation() {
        let config = SimulatorConfig::default();
        let simulator = OrderBookSimulator::new(config);

        assert_eq!(simulator.active_order_count(), 0);
        assert_eq!(simulator.position().quantity.0, 0);
    }

    #[test]
    fn test_place_order() {
        let config = SimulatorConfig::default();
        let mut simulator = OrderBookSimulator::new(config);

        let order_id = simulator.place_order(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0), 0);

        assert_eq!(order_id, 1);
        assert_eq!(simulator.active_order_count(), 1);
    }

    #[test]
    fn test_cancel_order() {
        let config = SimulatorConfig::default();
        let mut simulator = OrderBookSimulator::new(config);

        let order_id = simulator.place_order(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0), 0);

        assert!(simulator.cancel_order(order_id));
        assert_eq!(simulator.active_order_count(), 0);
    }

    #[test]
    fn test_latency_simulator() {
        let latency_sim = LatencySimulator::new(10_000, 5_000);

        let submission_time = 1_000_000_000; // 1 second in nanos
        let active_time = latency_sim.order_active_time(submission_time);

        assert_eq!(active_time, submission_time + 10_000_000); // +10ms in nanos
    }
}
