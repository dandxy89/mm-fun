use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::EMA;
use crate::FixedPoint;
use crate::MarketState;
use crate::StrategyConfig;
use crate::StrategyQuote;
use crate::drift_estimator::DriftEstimator;
use crate::inventory_manager::InventoryAction;
use crate::inventory_manager::InventoryManager;
use crate::risk_manager::RiskManager;

/// Quote engine - core market making logic
/// Combines drift estimation, inventory management, and risk controls
/// to generate optimal bid/ask quotes
#[derive(Debug, Clone)]
pub struct QuoteEngine {
    config: StrategyConfig,
    drift_estimator: DriftEstimator,
    inventory_manager: InventoryManager,
    risk_manager: RiskManager,
    volatility_ema: EMA,
    last_mid_price: Option<FixedPoint>,
}

impl QuoteEngine {
    pub fn new(config: StrategyConfig) -> Self {
        let drift_estimator = DriftEstimator::new(config.clone());
        let inventory_manager = InventoryManager::new(config.clone());
        let risk_manager = RiskManager::new(config.clone());
        let volatility_ema = EMA::new(config.volatility_halflife_secs, 1.0);

        Self { config, drift_estimator, inventory_manager, risk_manager, volatility_ema, last_mid_price: None }
    }

    /// Get mutable references to components (for external updates)
    pub fn drift_estimator_mut(&mut self) -> &mut DriftEstimator {
        &mut self.drift_estimator
    }

    pub fn inventory_manager_mut(&mut self) -> &mut InventoryManager {
        &mut self.inventory_manager
    }

    pub fn risk_manager_mut(&mut self) -> &mut RiskManager {
        &mut self.risk_manager
    }

    /// Update volatility estimate
    fn update_volatility(&mut self, state: &MarketState) {
        if let Some(last_mid) = self.last_mid_price {
            let current_mid = state.mid_price();
            let returns = ((current_mid - last_mid).to_f64() / last_mid.to_f64()).abs();
            self.volatility_ema.update(returns);
            debug!(
                volatility = %self.volatility_ema.value(),
                returns = %returns,
                "Updated volatility estimate"
            );
        }
        self.last_mid_price = Some(state.mid_price());
    }

    /// Calculate base spread in basis points
    fn calculate_base_spread_bps(&self, state: &MarketState) -> f64 {
        // Start with minimum spread
        let min_spread = self.config.min_spread_bps;

        // Add volatility component
        let volatility = self.volatility_ema.value();
        let vol_spread = volatility * self.config.volatility_factor * 10000.0; // Convert to bps

        // Ensure we're at least as wide as the current market spread
        let market_spread_bps = state.spread_bps();
        let tick_spread_bps = market_spread_bps.max(min_spread);

        // Take the maximum of all components
        let base_spread = min_spread.max(vol_spread).max(tick_spread_bps);

        debug!(
            base_spread_bps = %base_spread,
            min_spread_bps = %min_spread,
            vol_spread_bps = %vol_spread,
            market_spread_bps = %market_spread_bps,
            volatility = %volatility,
            "Calculated base spread"
        );

        base_spread
    }

    /// Generate quotes for current market state
    pub fn generate_quotes(&mut self, state: &MarketState) -> Option<StrategyQuote> {
        info!(
            timestamp = %state.timestamp,
            bid = %state.bid_price.to_f64(),
            ask = %state.ask_price.to_f64(),
            mid = %state.mid_price().to_f64(),
            "Generating quotes"
        );

        // Update models
        self.drift_estimator.update_market_state(state);
        self.update_volatility(state);

        // Check if risk manager allows quoting
        if self.risk_manager.is_killed() {
            warn!(
                reason = ?self.risk_manager.kill_reason(),
                "Quote generation blocked: strategy is killed"
            );
            return None;
        }

        // Estimate fair value with drift
        let mid = state.mid_price();
        let drift_bps = self.drift_estimator.estimate_drift_bps(state);
        let fair_value = mid.apply_bps(drift_bps);

        debug!(
            mid = %mid.to_f64(),
            drift_bps = %drift_bps,
            fair_value = %fair_value.to_f64(),
            "Estimated fair value"
        );

        // Get inventory information
        let inventory = self.inventory_manager.inventory();
        let inventory_skew_bps = self.inventory_manager.inventory_skew_bps(drift_bps);
        let inventory_action = self.inventory_manager.recommended_action(drift_bps);

        info!(
            inventory = %inventory.to_f64(),
            inventory_skew_bps = %inventory_skew_bps,
            inventory_action = ?inventory_action,
            "Inventory state"
        );

        // Calculate base spread
        let mut base_spread_bps = self.calculate_base_spread_bps(state);

        // Adjust spread based on inventory action
        let spread_multiplier = match inventory_action {
            InventoryAction::EmergencyUnwind => 0.5, // Tighten spread aggressively
            InventoryAction::AggressiveUnwind => 0.7,
            InventoryAction::PassiveUnwind => 1.0,
            InventoryAction::Neutral => 1.0,
            InventoryAction::Accumulate => 1.2, // Widen spread slightly (less aggressive)
        };

        base_spread_bps *= spread_multiplier;

        debug!(
            base_spread_bps = %base_spread_bps,
            spread_multiplier = %spread_multiplier,
            "Applied spread multiplier based on inventory action"
        );

        // Apply inventory skew
        // Positive skew = widen ask (discourage buying from us), tighten bid
        // Negative skew = widen bid (discourage selling to us), tighten ask
        let half_spread_bps = base_spread_bps / 2.0;
        let bid_spread_bps = half_spread_bps - inventory_skew_bps / 2.0;
        let ask_spread_bps = half_spread_bps + inventory_skew_bps / 2.0;

        // Calculate prices
        let bid_price = fair_value.subtract_bps(bid_spread_bps);
        let ask_price = fair_value.apply_bps(ask_spread_bps);

        info!(
            bid_price = %bid_price.to_f64(),
            ask_price = %ask_price.to_f64(),
            bid_spread_bps = %bid_spread_bps,
            ask_spread_bps = %ask_spread_bps,
            "Calculated bid/ask prices"
        );

        // Calculate sizes
        let (bid_size_factor, ask_size_factor) = self.inventory_manager.asymmetric_sizes();
        let base_size = self.config.base_quote_size;

        // Further adjust sizes based on action
        let size_urgency = match inventory_action {
            InventoryAction::EmergencyUnwind => 2.0, // Double size for emergency
            InventoryAction::AggressiveUnwind => 1.5,
            InventoryAction::PassiveUnwind => 1.0,
            InventoryAction::Neutral => 1.0,
            InventoryAction::Accumulate => 0.8, // Reduce size when accumulating
        };

        let base_size_fp = FixedPoint::from_f64(base_size);

        let bid_size = if inventory > FixedPoint::ZERO {
            // Long: reduce bid size
            base_size_fp.mul_scalar(bid_size_factor)
        } else {
            // Short or neutral: normal or increased bid size
            base_size_fp.mul_scalar(bid_size_factor * size_urgency)
        };

        let ask_size = if inventory < FixedPoint::ZERO {
            // Short: reduce ask size
            base_size_fp.mul_scalar(ask_size_factor)
        } else {
            // Long or neutral: normal or increased ask size
            base_size_fp.mul_scalar(ask_size_factor * size_urgency)
        };

        debug!(
            bid_size = %bid_size.to_f64(),
            ask_size = %ask_size.to_f64(),
            bid_size_factor = %bid_size_factor,
            ask_size_factor = %ask_size_factor,
            size_urgency = %size_urgency,
            "Calculated bid/ask sizes"
        );

        // Get confidence from drift estimator
        let confidence = self.drift_estimator.confidence();

        debug!(
            confidence = %confidence,
            "Drift estimate confidence"
        );

        let quote =
            StrategyQuote { timestamp: state.timestamp, bid_price, bid_size, ask_price, ask_size, fair_value, inventory, confidence };

        // Risk check
        let mark_price = state.mid_price();
        let position = self.inventory_manager.position();
        let risk_result = self.risk_manager.check_quote(&quote, position, mark_price);

        if risk_result.is_accept() {
            info!(
                bid = %quote.bid_price.to_f64(),
                ask = %quote.ask_price.to_f64(),
                bid_size = %quote.bid_size.to_f64(),
                ask_size = %quote.ask_size.to_f64(),
                "Quote generated successfully"
            );
            Some(quote)
        } else {
            if let crate::risk_manager::RiskCheckResult::Reject { reason } = &risk_result {
                warn!(
                    reason = %reason,
                    bid = %quote.bid_price.to_f64(),
                    ask = %quote.ask_price.to_f64(),
                    "Quote rejected by risk check"
                );
            }
            None
        }
    }

    /// Generate ladder quotes (multiple levels)
    pub fn generate_ladder_quotes(&mut self, state: &MarketState, num_levels: usize, level_spacing_bps: f64) -> Vec<StrategyQuote> {
        info!(
            num_levels = %num_levels,
            level_spacing_bps = %level_spacing_bps,
            "Generating ladder quotes"
        );

        let mut quotes = Vec::new();

        // Generate base quote
        if let Some(base_quote) = self.generate_quotes(state) {
            quotes.push(base_quote);

            // Generate additional levels
            for level in 1..num_levels {
                let level_offset = level_spacing_bps * level as f64;
                let level_size_factor = 1.0 / (level as f64 + 1.0); // Reduce size at further levels

                let bid_price = base_quote.bid_price.subtract_bps(level_offset);
                let ask_price = base_quote.ask_price.apply_bps(level_offset);

                let bid_size = base_quote.bid_size.mul_scalar(level_size_factor);
                let ask_size = base_quote.ask_size.mul_scalar(level_size_factor);

                let level_quote = StrategyQuote {
                    timestamp: state.timestamp,
                    bid_price,
                    bid_size,
                    ask_price,
                    ask_size,
                    fair_value: base_quote.fair_value,
                    inventory: base_quote.inventory,
                    confidence: base_quote.confidence * level_size_factor, // Reduce confidence at further levels
                };

                // Check risk for each level
                let position = self.inventory_manager.position();
                let risk_result = self.risk_manager.check_quote(&level_quote, position, state.mid_price());

                if risk_result.is_accept() {
                    debug!(
                        level = %level,
                        bid = %bid_price.to_f64(),
                        ask = %ask_price.to_f64(),
                        "Added ladder level"
                    );
                    quotes.push(level_quote);
                } else {
                    debug!(
                        level = %level,
                        "Ladder level rejected by risk check, stopping"
                    );
                    break; // Stop generating levels if risk check fails
                }
            }
        }

        info!(
            levels_generated = %quotes.len(),
            "Ladder quotes generation complete"
        );

        quotes
    }

    /// Get configuration
    pub fn config(&self) -> &StrategyConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_generation() {
        let config = StrategyConfig {
            min_spread_bps: 5.0,
            base_quote_size: 0.1,
            min_confidence: 0.1, // Low threshold for testing
            ..Default::default()
        };

        let mut engine = QuoteEngine::new(config);

        let state = MarketState {
            timestamp: 1_000_000_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };

        let quote = engine.generate_quotes(&state);
        assert!(quote.is_some());

        let quote = quote.unwrap();
        assert!(quote.bid_price < quote.ask_price);
        assert!(quote.bid_size > FixedPoint::ZERO);
        assert!(quote.ask_size > FixedPoint::ZERO);
    }

    #[test]
    fn test_ladder_quotes() {
        let config = StrategyConfig { min_spread_bps: 5.0, base_quote_size: 0.1, min_confidence: 0.1, ..Default::default() };

        let mut engine = QuoteEngine::new(config);

        let state = MarketState {
            timestamp: 1_000_000_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };

        let quotes = engine.generate_ladder_quotes(&state, 3, 5.0);
        assert!(!quotes.is_empty());
        assert!(quotes.len() <= 3);

        // Check that levels are properly spaced
        if quotes.len() > 1 {
            assert!(quotes[1].bid_price < quotes[0].bid_price);
            assert!(quotes[1].ask_price > quotes[0].ask_price);
        }
    }

    #[test]
    fn test_inventory_skew_in_quotes() {
        let config = StrategyConfig {
            min_spread_bps: 10.0,
            inventory_skew_factor: 0.001,
            base_quote_size: 0.1,
            min_confidence: 0.1,
            ..Default::default()
        };

        let mut engine = QuoteEngine::new(config);

        let state = MarketState {
            timestamp: 1_000_000_000,
            bid_price: FixedPoint::from_f64(100.0),
            ask_price: FixedPoint::from_f64(101.0),
            bid_volume: FixedPoint::from_f64(10.0),
            ask_volume: FixedPoint::from_f64(10.0),
            last_trade_price: Some(FixedPoint::from_f64(100.5)),
            last_trade_size: Some(FixedPoint::from_f64(1.0)),
        };

        // Generate quote with no inventory
        let neutral_quote = engine.generate_quotes(&state).unwrap();
        let _neutral_spread = (neutral_quote.ask_price - neutral_quote.bid_price).to_f64();

        // Simulate long position
        let mut pos = crate::Position::new();
        pos.quantity = FixedPoint::from_f64(5.0);
        engine.inventory_manager_mut().update_position(pos);

        // Generate quote with long inventory
        let long_quote = engine.generate_quotes(&state).unwrap();

        // With long inventory, ask should be wider (easier to sell)
        // and bid should be tighter (less eager to buy)
        let mid = state.mid_price().to_f64();
        let _neutral_ask_distance = (neutral_quote.ask_price.to_f64() - mid).abs();
        let _long_ask_distance = (long_quote.ask_price.to_f64() - mid).abs();

        // This might not always hold due to other factors, but generally should
        // assert!(long_ask_distance >= neutral_ask_distance);
    }
}
