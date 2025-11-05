pub mod drift_estimator;
pub mod inventory_manager;
pub mod quote_engine;
pub mod risk_manager;

// Re-export commonly used types from mm_types
pub use mm_types::FixedPoint;
pub use mm_types::MarketState;
pub use mm_types::OrderSide;
pub use mm_types::Position;

/// Core strategy parameters
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StrategyConfig {
    /// Minimum spread in basis points (0.01% = 1 bp)
    pub min_spread_bps: f64,

    /// Volatility multiplier for spread calculation
    pub volatility_factor: f64,

    /// Inventory skew factor (how much to adjust quotes per unit of inventory)
    pub inventory_skew_factor: f64,

    /// Maximum position size (in base currency units)
    pub max_position_size: f64,

    /// Maximum order size per quote
    pub max_order_size: f64,

    /// Target inventory (usually 0 for market-neutral)
    pub target_inventory: f64,

    /// Base quote size
    pub base_quote_size: f64,

    /// Risk aversion parameter (higher = more conservative)
    pub risk_aversion: f64,

    /// EMA half-life for drift estimation (in seconds)
    pub drift_halflife_secs: f64,

    /// EMA half-life for volatility estimation (in seconds)
    pub volatility_halflife_secs: f64,

    /// Trade flow window for OFI calculation (in seconds)
    pub trade_flow_window_secs: f64,

    /// Minimum confidence score to publish quotes
    pub min_confidence: f64,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            min_spread_bps: 5.0,             // 0.05% minimum spread
            volatility_factor: 2.0,          // 2x volatility for spread
            inventory_skew_factor: 0.0001,   // 1bp per BTC for BTCUSDT
            max_position_size: 10.0,         // 10 BTC max
            max_order_size: 1.0,             // 1 BTC per order
            target_inventory: 0.0,           // Market neutral
            base_quote_size: 0.1,            // 0.1 BTC base size
            risk_aversion: 1.0,              // Neutral risk aversion
            drift_halflife_secs: 60.0,       // 1 minute half-life for drift
            volatility_halflife_secs: 300.0, // 5 minute half-life for volatility
            trade_flow_window_secs: 10.0,    // 10 second window for OFI
            min_confidence: 0.5,             // 50% minimum confidence
        }
    }
}

/// Quote output from the strategy
#[derive(Debug, Clone, Copy)]
pub struct StrategyQuote {
    pub timestamp: u64,
    pub bid_price: FixedPoint,
    pub bid_size: FixedPoint,
    pub ask_price: FixedPoint,
    pub ask_size: FixedPoint,
    pub fair_value: FixedPoint,
    pub inventory: FixedPoint,
    pub confidence: f64,
}

/// Helper function to calculate EMA alpha from half-life
pub fn ema_alpha_from_halflife(halflife_secs: f64, dt_secs: f64) -> f64 {
    // alpha = 1 - exp(-ln(2) * dt / halflife)
    1.0 - (-std::f64::consts::LN_2 * dt_secs / halflife_secs).exp()
}

/// Exponential moving average calculator
#[derive(Debug, Clone)]
pub struct EMA {
    value: f64,
    alpha: f64,
    initialized: bool,
}

impl EMA {
    pub fn new(halflife_secs: f64, dt_secs: f64) -> Self {
        let alpha = ema_alpha_from_halflife(halflife_secs, dt_secs);
        Self { value: 0.0, alpha, initialized: false }
    }

    pub fn update(&mut self, new_value: f64) {
        if !self.initialized {
            self.value = new_value;
            self.initialized = true;
        } else {
            self.value = self.alpha * new_value + (1.0 - self.alpha) * self.value;
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_state_mid_price() {
        let state = MarketState {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: None,
            last_trade_size: None,
        };

        let mid = state.mid_price();
        assert!((mid.to_f64() - 100.5).abs() < 0.001);
    }

    #[test]
    fn test_market_state_micro_price() {
        let state = MarketState {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(20.0),
            last_trade_price: None,
            last_trade_size: None,
        };

        // micro = (100 * 20 + 101 * 10) / 30 = 3010 / 30 = 100.333...
        let micro = state.micro_price();
        assert!((micro.to_f64() - 100.333).abs() < 0.01);
    }

    #[test]
    fn test_position_buy_fill() {
        let mut pos = Position::new();
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));

        assert_eq!(pos.quantity.to_f64(), 1.0);
        assert_eq!(pos.avg_entry_price.to_f64(), 100.0);
    }

    #[test]
    fn test_position_sell_fill() {
        let mut pos = Position::new();
        pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));

        assert_eq!(pos.quantity.to_f64(), -1.0);
        assert_eq!(pos.avg_entry_price.to_f64(), 100.0);
    }

    #[test]
    fn test_position_realized_pnl() {
        let mut pos = Position::new();
        // Buy 1 @ 100
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));
        // Sell 1 @ 110
        pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(110.0), FixedPoint::from_f64(1.0));

        assert_eq!(pos.quantity.to_f64(), 0.0);
        assert_eq!(pos.realized_pnl.to_f64(), 10.0);
    }

    #[test]
    fn test_ema() {
        let mut ema = EMA::new(10.0, 1.0);
        ema.update(100.0);
        assert_eq!(ema.value(), 100.0);

        ema.update(110.0);
        assert!(ema.value() > 100.0 && ema.value() < 110.0);
    }

    // ===== Comprehensive FixedPoint Tests =====

    #[test]
    fn test_fixedpoint_conversions() {
        let fp = FixedPoint::from_f64(123.456);
        assert!((fp.to_f64() - 123.456).abs() < 0.001);

        let zero = FixedPoint::ZERO;
        assert_eq!(zero.to_f64(), 0.0);
    }

    #[test]
    fn test_fixedpoint_arithmetic() {
        let a = FixedPoint::from_f64(10.0);
        let b = FixedPoint::from_f64(5.0);

        let sum = a + b;
        assert_eq!(sum.to_f64(), 15.0);

        let diff = a - b;
        assert_eq!(diff.to_f64(), 5.0);

        let prod = a * b;
        assert_eq!(prod.to_f64(), 50.0);

        let quot = a / b;
        assert_eq!(quot.to_f64(), 2.0);
    }

    #[test]
    fn test_fixedpoint_negative() {
        let a = FixedPoint::from_f64(-10.5);
        assert_eq!(a.to_f64(), -10.5);

        let b = FixedPoint::from_f64(5.5);
        let sum = a + b;
        assert_eq!(sum.to_f64(), -5.0);
    }

    #[test]
    fn test_fixedpoint_comparison() {
        let a = FixedPoint::from_f64(10.0);
        let b = FixedPoint::from_f64(5.0);
        let c = FixedPoint::from_f64(10.0);

        assert!(a > b);
        assert!(b < a);
        assert_eq!(a, c);
        assert!(a >= c);
    }

    // ===== MarketState Edge Case Tests =====

    #[test]
    fn test_market_state_zero_volume() {
        let state = MarketState {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::ZERO,
            ask_volume: FixedPoint::ZERO,
            last_trade_price: None,
            last_trade_size: None,
        };

        // With zero volume, micro_price should return mid_price
        let micro = state.micro_price();
        let mid = state.mid_price();
        assert_eq!(micro, mid);

        // Orderbook imbalance should be 0
        assert_eq!(state.orderbook_imbalance(), 0.0);
    }

    #[test]
    fn test_market_state_spread() {
        let state = MarketState {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(102.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: None,
            last_trade_size: None,
        };

        let spread_bps = state.spread_bps();
        // Spread = 2.0, mid = 101.0, bps = (2/101) * 10000 ≈ 198 bps
        assert!((spread_bps - 198.0).abs() < 1.0);
    }

    #[test]
    fn test_market_state_imbalance_extremes() {
        // All ask volume
        let state = MarketState {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::ZERO,
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: None,
            last_trade_size: None,
        };
        assert_eq!(state.orderbook_imbalance(), 1.0);

        // All bid volume
        let state2 = MarketState {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::ZERO,
            last_trade_price: None,
            last_trade_size: None,
        };
        assert_eq!(state2.orderbook_imbalance(), -1.0);
    }

    // ===== Position Tracking Comprehensive Tests =====

    #[test]
    fn test_position_averaging() {
        let mut pos = Position::new();

        // Buy 1 @ 100
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));
        assert_eq!(pos.avg_entry_price.to_f64(), 100.0);

        // Buy 1 @ 110
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(110.0), FixedPoint::from_f64(1.0));
        assert_eq!(pos.quantity.to_f64(), 2.0);
        assert_eq!(pos.avg_entry_price.to_f64(), 105.0); // (100+110)/2
    }

    #[test]
    fn test_position_partial_close() {
        let mut pos = Position::new();

        // Buy 2 @ 100
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(2.0));

        // Sell 1 @ 110
        pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(110.0), FixedPoint::from_f64(1.0));

        assert_eq!(pos.quantity.to_f64(), 1.0);
        assert_eq!(pos.avg_entry_price.to_f64(), 100.0);
        assert_eq!(pos.realized_pnl.to_f64(), 10.0); // (110-100) * 1
    }

    #[test]
    fn test_position_reversal() {
        let mut pos = Position::new();

        // Buy 1 @ 100
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));

        // Sell 2 @ 110 (close long and go short 1)
        pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(110.0), FixedPoint::from_f64(2.0));

        assert_eq!(pos.quantity.to_f64(), -1.0);
        assert_eq!(pos.avg_entry_price.to_f64(), 110.0);
        assert_eq!(pos.realized_pnl.to_f64(), 10.0); // Profit from closing long
    }

    #[test]
    fn test_position_unrealized_pnl() {
        let mut pos = Position::new();

        // Buy 1 @ 100
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));

        let mark_price = FixedPoint::from_f64(105.0);
        let unrealized = pos.unrealized_pnl(mark_price);
        assert_eq!(unrealized.to_f64(), 5.0); // (105-100) * 1

        // Short position
        let mut short_pos = Position::new();
        short_pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));

        let unrealized_short = short_pos.unrealized_pnl(mark_price);
        assert_eq!(unrealized_short.to_f64(), -5.0); // (105-100) * -1
    }

    #[test]
    fn test_position_multiple_trades() {
        let mut pos = Position::new();

        // Simulate active trading
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(100.0), FixedPoint::from_f64(1.0));
        pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(101.0), FixedPoint::from_f64(0.5));
        pos.apply_fill(OrderSide::Bid, FixedPoint::from_f64(99.0), FixedPoint::from_f64(0.5));
        pos.apply_fill(OrderSide::Ask, FixedPoint::from_f64(102.0), FixedPoint::from_f64(1.0));

        // After these trades:
        // Buy 1 @ 100 -> qty=1, avg=100
        // Sell 0.5 @ 101 -> qty=0.5, avg=100, realized=0.5
        // Buy 0.5 @ 99 -> qty=1, avg=99.5
        // Sell 1 @ 102 -> qty=0, realized=0.5 + 2.5 = 3.0

        assert_eq!(pos.quantity.to_f64(), 0.0);
        assert_eq!(pos.realized_pnl.to_f64(), 3.0);
    }
}
