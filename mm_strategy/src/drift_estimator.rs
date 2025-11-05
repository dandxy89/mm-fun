use std::collections::VecDeque;

use mm_binary::messages::TradeSide;
use tracing::debug;
use tracing::info;

use crate::EMA;
use crate::FixedPoint;
use crate::MarketState;
use crate::StrategyConfig;

/// Trade for flow analysis
#[derive(Debug, Clone, Copy)]
pub struct Trade {
    pub timestamp: u64,
    pub price: FixedPoint,
    pub quantity: FixedPoint,
    pub side: TradeSide,
    pub is_aggressor: bool,
}

/// Order Flow Imbalance (OFI) calculator
/// Tracks changes in bid/ask volume to predict short-term price movement
#[derive(Debug, Clone)]
pub struct OrderFlowImbalance {
    prev_bid_volume: FixedPoint,
    prev_ask_volume: FixedPoint,
    ofi_ema: EMA,
    initialized: bool,
}

impl OrderFlowImbalance {
    pub fn new(config: &StrategyConfig) -> Self {
        Self {
            prev_bid_volume: FixedPoint::ZERO,
            prev_ask_volume: FixedPoint::ZERO,
            ofi_ema: EMA::new(config.drift_halflife_secs, 0.1), // Assume 100ms updates
            initialized: false,
        }
    }

    /// Update OFI with new market state
    /// OFI = Δbid_volume - Δask_volume
    /// Positive OFI → buying pressure → upward drift
    /// Negative OFI → selling pressure → downward drift
    pub fn update(&mut self, state: &MarketState) {
        if !self.initialized {
            self.prev_bid_volume = state.bid_volume;
            self.prev_ask_volume = state.ask_volume;
            self.initialized = true;
            debug!("OFI initialized");
            return;
        }

        let delta_bid = state.bid_volume - self.prev_bid_volume;
        let delta_ask = state.ask_volume - self.prev_ask_volume;

        let ofi = (delta_bid - delta_ask).to_f64();
        self.ofi_ema.update(ofi);

        debug!(
            delta_bid = %delta_bid.to_f64(),
            delta_ask = %delta_ask.to_f64(),
            ofi = %ofi,
            ofi_ema = %self.ofi_ema.value(),
            "Updated OFI"
        );

        self.prev_bid_volume = state.bid_volume;
        self.prev_ask_volume = state.ask_volume;
    }

    /// Get current OFI value
    pub fn value(&self) -> f64 {
        self.ofi_ema.value()
    }
}

/// Trade flow analyzer
/// Tracks recent trades to estimate market direction and aggression
#[derive(Debug, Clone)]
pub struct TradeFlowAnalyzer {
    trades: VecDeque<Trade>,
    window_secs: f64,
    buy_volume_ema: EMA,
    sell_volume_ema: EMA,
}

impl TradeFlowAnalyzer {
    pub fn new(config: &StrategyConfig) -> Self {
        Self {
            trades: VecDeque::new(),
            window_secs: config.trade_flow_window_secs,
            buy_volume_ema: EMA::new(config.drift_halflife_secs, 0.1),
            sell_volume_ema: EMA::new(config.drift_halflife_secs, 0.1),
        }
    }

    /// Add a new trade
    pub fn add_trade(&mut self, trade: Trade) {
        debug!(
            price = %trade.price.to_f64(),
            qty = %trade.quantity.to_f64(),
            side = ?trade.side,
            is_aggressor = %trade.is_aggressor,
            "Adding trade to flow analyzer"
        );

        self.trades.push_back(trade);

        // Update EMAs
        match trade.side {
            TradeSide::Buy => {
                self.buy_volume_ema.update(trade.quantity.to_f64());
                self.sell_volume_ema.update(0.0);
            }
            TradeSide::Sell => {
                self.sell_volume_ema.update(trade.quantity.to_f64());
                self.buy_volume_ema.update(0.0);
            }
        }

        // Remove old trades outside window
        let mut removed_count = 0;
        while let Some(front) = self.trades.front() {
            let age_secs = (trade.timestamp - front.timestamp) as f64 / 1_000_000_000.0;
            if age_secs > self.window_secs {
                self.trades.pop_front();
                removed_count += 1;
            } else {
                break;
            }
        }

        if removed_count > 0 {
            debug!(
                removed = %removed_count,
                window_size = %self.trades.len(),
                "Removed expired trades"
            );
        }
    }

    /// Calculate trade imbalance (-1 to 1)
    /// Positive = more buying, Negative = more selling
    pub fn trade_imbalance(&self) -> f64 {
        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;

        for trade in &self.trades {
            match trade.side {
                TradeSide::Buy => buy_volume += trade.quantity.to_f64(),
                TradeSide::Sell => sell_volume += trade.quantity.to_f64(),
            }
        }

        let total_volume = buy_volume + sell_volume;
        if total_volume == 0.0 {
            return 0.0;
        }

        (buy_volume - sell_volume) / total_volume
    }

    /// Calculate aggressive trade imbalance (only aggressive trades)
    /// More predictive of short-term price movement
    pub fn aggressive_trade_imbalance(&self) -> f64 {
        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;

        for trade in &self.trades {
            if trade.is_aggressor {
                match trade.side {
                    TradeSide::Buy => buy_volume += trade.quantity.to_f64(),
                    TradeSide::Sell => sell_volume += trade.quantity.to_f64(),
                }
            }
        }

        let total_volume = buy_volume + sell_volume;
        if total_volume == 0.0 {
            return 0.0;
        }

        (buy_volume - sell_volume) / total_volume
    }

    /// Calculate volume-weighted average price (VWAP) of recent trades
    pub fn vwap(&self) -> Option<FixedPoint> {
        if self.trades.is_empty() {
            return None;
        }

        let mut total_value = 0.0;
        let mut total_volume = 0.0;

        for trade in &self.trades {
            total_value += trade.price.to_f64() * trade.quantity.to_f64();
            total_volume += trade.quantity.to_f64();
        }

        if total_volume == 0.0 {
            return None;
        }

        Some(FixedPoint::from_f64(total_value / total_volume))
    }
}

/// Complete drift estimator combining multiple signals
#[derive(Debug, Clone)]
pub struct DriftEstimator {
    ofi: OrderFlowImbalance,
    trade_flow: TradeFlowAnalyzer,
    price_change_ema: EMA,
    volatility_ema: EMA,
    last_price: Option<FixedPoint>,
}

impl DriftEstimator {
    pub fn new(config: StrategyConfig) -> Self {
        Self {
            ofi: OrderFlowImbalance::new(&config),
            trade_flow: TradeFlowAnalyzer::new(&config),
            price_change_ema: EMA::new(config.drift_halflife_secs, 0.1),
            volatility_ema: EMA::new(config.volatility_halflife_secs, 1.0),
            last_price: None,
        }
    }

    /// Update with new market state (for OFI calculation)
    pub fn update_market_state(&mut self, state: &MarketState) {
        self.ofi.update(state);

        // Track price changes
        if let Some(last_price) = state.last_trade_price {
            let mid = state.mid_price();
            let price_change = (last_price - mid).to_f64() / mid.to_f64();
            self.price_change_ema.update(price_change);
        }
    }

    /// Update with new trade
    pub fn add_trade(&mut self, trade: Trade) {
        // Update volatility from trade price changes
        if let Some(last) = self.last_price {
            if last.0 > 0 {
                let returns = (trade.price.to_f64() - last.to_f64()) / last.to_f64();
                let abs_returns = returns.abs();
                self.volatility_ema.update(abs_returns);
            }
        }
        self.last_price = Some(trade.price);

        self.trade_flow.add_trade(trade);
    }

    /// Estimate drift in basis points per second
    /// Combines OFI, trade flow, and micro-price signals
    pub fn estimate_drift_bps(&self, state: &MarketState) -> f64 {
        // 1. Order flow imbalance contribution
        let ofi_value = self.ofi.value();
        let ofi_contribution = ofi_value * 0.1;

        // 2. Trade flow imbalance
        let trade_imbalance = self.trade_flow.aggressive_trade_imbalance();
        let trade_contribution = trade_imbalance * 0.5;

        // 3. Orderbook imbalance
        let ob_imbalance = state.orderbook_imbalance();
        let ob_contribution = ob_imbalance * 0.8;

        // 4. Micro-price vs mid-price
        let micro = state.micro_price();
        let mid = state.mid_price();
        let micro_diff_bps = ((micro - mid).to_f64() / mid.to_f64()) * 10000.0;
        let micro_contribution = micro_diff_bps * 0.05;

        let drift = ofi_contribution + trade_contribution + ob_contribution + micro_contribution;

        info!(
            drift_bps = %drift,
            ofi_value = %ofi_value,
            ofi_contribution = %ofi_contribution,
            trade_imbalance = %trade_imbalance,
            trade_contribution = %trade_contribution,
            ob_imbalance = %ob_imbalance,
            ob_contribution = %ob_contribution,
            micro_diff_bps = %micro_diff_bps,
            micro_contribution = %micro_contribution,
            "Estimated drift from signals"
        );

        drift
    }

    /// Estimate drift as an absolute price adjustment
    pub fn estimate_drift_price(&self, state: &MarketState) -> FixedPoint {
        let drift_bps = self.estimate_drift_bps(state);
        let mid = state.mid_price();
        let drift_fraction = drift_bps / 10000.0;

        FixedPoint::from_f64(mid.to_f64() * drift_fraction)
    }

    /// Get confidence in drift estimate (0 to 1)
    /// Higher confidence when signals agree
    pub fn confidence(&self) -> f64 {
        let mut agreement_count = 0;
        let mut total_signals = 0;

        let ofi = self.ofi.value();
        let trade_imb = self.trade_flow.aggressive_trade_imbalance();

        // Check if OFI and trade flow agree
        let signals_agree = ofi.signum() == trade_imb.signum() && ofi.abs() > 0.1 && trade_imb.abs() > 0.1;
        if signals_agree {
            agreement_count += 1;
        }
        total_signals += 1;

        // More agreement = higher confidence
        let agreement_ratio = agreement_count as f64 / total_signals as f64;

        // Scale by signal strength
        let signal_strength = (ofi.abs() + trade_imb.abs()) / 2.0;
        let confidence = agreement_ratio * signal_strength.min(1.0);
        let final_confidence = confidence.clamp(0.1, 1.0);

        debug!(
            ofi = %ofi,
            trade_imbalance = %trade_imb,
            signals_agree = %signals_agree,
            agreement_ratio = %agreement_ratio,
            signal_strength = %signal_strength,
            confidence = %final_confidence,
            "Calculated drift confidence"
        );

        final_confidence
    }

    /// Get current volatility estimate (as standard deviation of returns)
    pub fn current_volatility(&self) -> f64 {
        self.volatility_ema.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_flow_imbalance() {
        let config = StrategyConfig::default();
        let mut analyzer = TradeFlowAnalyzer::new(&config);

        // Add buy trades
        analyzer.add_trade(Trade {
            timestamp: 1_000_000_000,
            price: FixedPoint::from_f64(100.0),
            quantity: FixedPoint::from_f64(1.0),
            side: TradeSide::Buy,
            is_aggressor: true,
        });

        analyzer.add_trade(Trade {
            timestamp: 1_500_000_000,
            price: FixedPoint::from_f64(100.5),
            quantity: FixedPoint::from_f64(0.5),
            side: TradeSide::Sell,
            is_aggressor: true,
        });

        let imbalance = analyzer.trade_imbalance();
        assert!(imbalance > 0.0); // More buy volume
    }

    #[test]
    fn test_drift_estimator() {
        let config = StrategyConfig::default();
        let mut estimator = DriftEstimator::new(config);

        // First state to initialize OFI
        let state1 = MarketState {
            timestamp: 1_000_000_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };
        estimator.update_market_state(&state1);

        // Second state with increased ask volume (bearish signal)
        // Increasing ask volume = more sellers = bearish
        let state2 = MarketState {
            timestamp: 1_000_100_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0), // Bid volume unchanged
            ask_volume: FixedPoint::from_f64(20.0), // Ask volume increased = bearish OFI
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };
        estimator.update_market_state(&state2);

        let drift = estimator.estimate_drift_bps(&state2);
        // With increased ask volume (OFI negative) and more total ask volume (ob_imbalance positive but bearish),
        // we expect negative drift (downward pressure from selling)
        assert!(drift < 0.0, "Expected negative drift but got: {}", drift);
    }

    #[test]
    fn test_drift_estimator_bullish() {
        let config = StrategyConfig::default();
        let mut estimator = DriftEstimator::new(config);

        let state1 = MarketState {
            timestamp: 1_000_000_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };
        estimator.update_market_state(&state1);

        // Bullish signal: bid volume increases
        let state2 = MarketState {
            timestamp: 1_000_100_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(20.0), // Bid volume increased = bullish
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };
        estimator.update_market_state(&state2);

        let drift = estimator.estimate_drift_bps(&state2);
        assert!(drift > 0.0, "Expected positive drift for bullish signal but got: {}", drift);
    }

    #[test]
    fn test_ofi_initialization() {
        let config = StrategyConfig::default();
        let mut ofi = OrderFlowImbalance::new(&config);

        let state = MarketState {
            timestamp: 1_000_000_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: None,
            last_trade_size: None,
        };

        // First update should initialize
        ofi.update(&state);
        assert_eq!(ofi.value(), 0.0); // No delta on first update
    }

    #[test]
    fn test_aggressive_trade_filtering() {
        let config = StrategyConfig::default();
        let mut analyzer = TradeFlowAnalyzer::new(&config);

        // Add aggressive buy
        analyzer.add_trade(Trade {
            timestamp: 1_000_000_000,
            price: FixedPoint::from_f64(100.0),
            quantity: FixedPoint::from_f64(1.0),
            side: TradeSide::Buy,
            is_aggressor: true,
        });

        // Add passive sell
        analyzer.add_trade(Trade {
            timestamp: 1_000_100_000,
            price: FixedPoint::from_f64(100.0),
            quantity: FixedPoint::from_f64(1.0),
            side: TradeSide::Sell,
            is_aggressor: false,
        });

        let aggressive_imbalance = analyzer.aggressive_trade_imbalance();
        // Only aggressive trade counts, so should be positive (buy-heavy)
        assert!(aggressive_imbalance > 0.0);
    }

    #[test]
    fn test_drift_confidence() {
        let config = StrategyConfig::default();
        let estimator = DriftEstimator::new(config);

        // Initial confidence should be low (no signals)
        let confidence = estimator.confidence();
        assert!((0.0..=1.0).contains(&confidence));
    }

    #[test]
    fn test_volatility_tracking() {
        let config = StrategyConfig::default();
        let mut estimator = DriftEstimator::new(config);

        // Add trades with price changes
        estimator.add_trade(Trade {
            timestamp: 1_000_000_000,
            price: FixedPoint::from_f64(100.0),
            quantity: FixedPoint::from_f64(1.0),
            side: TradeSide::Buy,
            is_aggressor: true,
        });

        estimator.add_trade(Trade {
            timestamp: 1_100_000_000,
            price: FixedPoint::from_f64(101.0),
            quantity: FixedPoint::from_f64(1.0),
            side: TradeSide::Buy,
            is_aggressor: true,
        });

        let volatility = estimator.current_volatility();
        assert!(volatility >= 0.0); // Volatility should be non-negative
    }

    #[test]
    fn test_trade_window_expiration() {
        let config = StrategyConfig::default();
        let mut analyzer = TradeFlowAnalyzer::new(&config);

        // Add old trade (outside window)
        analyzer.add_trade(Trade {
            timestamp: 1_000_000_000,
            price: FixedPoint::from_f64(100.0),
            quantity: FixedPoint::from_f64(10.0),
            side: TradeSide::Buy,
            is_aggressor: true,
        });

        // Add recent trade (inside window)
        analyzer.add_trade(Trade {
            timestamp: 1_000_000_000 + (config.trade_flow_window_secs * 1_000_000_000.0) as u64 + 1,
            price: FixedPoint::from_f64(100.0),
            quantity: FixedPoint::from_f64(1.0),
            side: TradeSide::Sell,
            is_aggressor: true,
        });

        // Old trade should have expired, so imbalance should be negative (sell)
        let imbalance = analyzer.trade_imbalance();
        assert!(imbalance < 0.0);
    }
}
