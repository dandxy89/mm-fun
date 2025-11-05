use tracing::debug;
use tracing::info;

use crate::FixedPoint;
use crate::Position;
use crate::StrategyConfig;

/// Inventory manager for opportunistic market making
/// Adjusts quotes based on current inventory and market conditions
#[derive(Debug, Clone)]
pub struct InventoryManager {
    position: Position,
    config: StrategyConfig,
}

impl InventoryManager {
    pub fn new(config: StrategyConfig) -> Self {
        Self { position: Position::new(), config }
    }

    /// Get current position
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Get current inventory
    pub fn inventory(&self) -> FixedPoint {
        self.position.quantity
    }

    /// Update position with fill (called from simulation or execution layer)
    pub fn update_position(&mut self, position: Position) {
        let old_qty = self.position.quantity.to_f64();
        let new_qty = position.quantity.to_f64();

        info!(
            old_position = %old_qty,
            new_position = %new_qty,
            change = %(new_qty - old_qty),
            avg_entry = %position.avg_entry_price.to_f64(),
            realized_pnl = %position.realized_pnl.to_f64(),
            "Position updated"
        );

        self.position = position;
    }

    /// Calculate inventory skew adjustment in basis points
    /// Positive skew = widen ask, tighten bid (encourage selling)
    /// Negative skew = widen bid, tighten ask (encourage buying)
    pub fn inventory_skew_bps(&self, market_drift_bps: f64) -> f64 {
        let inventory = self.position.quantity.to_f64();
        let target = self.config.target_inventory;

        let inventory_deviation = inventory - target;

        // Base skew from inventory
        let base_skew = inventory_deviation * self.config.inventory_skew_factor * 10000.0; // Convert to bps

        // Opportunistic adjustment: amplify skew if drift is favorable
        // If we're long and drift is positive (rising market), reduce urgency to sell
        // If we're long and drift is negative (falling market), increase urgency to sell
        let final_skew = if inventory_deviation.abs() > 0.01 && market_drift_bps.abs() > 0.1 {
            let sign = inventory_deviation.signum();
            // If inventory and drift have opposite signs, increase skew (adverse)
            // If inventory and drift have same signs, reduce skew (favorable)
            if sign * market_drift_bps < 0.0 {
                debug!(
                    inventory_deviation = %inventory_deviation,
                    market_drift_bps = %market_drift_bps,
                    "Adverse market conditions - increasing skew urgency by 50%"
                );
                // Adverse: increase skew by 50%
                base_skew * 1.5
            } else {
                debug!(
                    inventory_deviation = %inventory_deviation,
                    market_drift_bps = %market_drift_bps,
                    "Favorable market conditions - reducing skew urgency by 30%"
                );
                // Favorable: reduce skew by 30%
                base_skew * 0.7
            }
        } else {
            base_skew
        };

        debug!(
            inventory = %inventory,
            target = %target,
            base_skew_bps = %base_skew,
            final_skew_bps = %final_skew,
            "Calculated inventory skew"
        );

        final_skew
    }

    /// Calculate urgency factor (0 to 1)
    /// Higher urgency when approaching position limits
    pub fn urgency(&self) -> f64 {
        let inventory = self.position.quantity.to_f64().abs();
        let max_position = self.config.max_position_size;

        let utilization = inventory / max_position;

        if utilization < 0.5 {
            0.0 // No urgency
        } else if utilization < 0.8 {
            (utilization - 0.5) / 0.3 // Linear ramp from 0.5 to 0.8
        } else {
            1.0 + (utilization - 0.8) / 0.2 // > 1.0 for extreme positions
        }
    }

    /// Calculate size adjustment factor based on inventory
    /// Reduce size as inventory grows to limit risk
    pub fn size_factor(&self) -> f64 {
        let utilization = self.position.quantity.to_f64().abs() / self.config.max_position_size;

        if utilization < 0.5 {
            1.0 // Full size
        } else if utilization < 0.8 {
            1.0 - (utilization - 0.5) * 0.5 / 0.3 // Reduce to 50%
        } else {
            0.5 - (utilization - 0.8) * 0.5 / 0.2 // Reduce to 0%
        }
    }

    /// Calculate asymmetric sizes for bid/ask based on inventory
    /// When long: reduce ask size, keep bid size (to unwind)
    /// When short: reduce bid size, keep ask size (to unwind)
    pub fn asymmetric_sizes(&self) -> (f64, f64) {
        let inventory = self.position.quantity.to_f64();
        let _base_size = self.config.base_quote_size;
        let size_factor = self.size_factor();

        let (bid_factor, ask_factor) = if inventory > 0.0 {
            // Long position: eager to sell, reluctant to buy
            let bid_factor = size_factor;
            let ask_factor = 1.0; // Full size on ask
            (bid_factor, ask_factor)
        } else if inventory < 0.0 {
            // Short position: eager to buy, reluctant to sell
            let bid_factor = 1.0; // Full size on bid
            let ask_factor = size_factor;
            (bid_factor, ask_factor)
        } else {
            // Neutral: symmetric sizes
            (1.0, 1.0)
        };

        debug!(
            inventory = %inventory,
            bid_factor = %bid_factor,
            ask_factor = %ask_factor,
            size_factor = %size_factor,
            "Calculated asymmetric sizes"
        );

        (bid_factor, ask_factor)
    }

    /// Check if we can increase position (respect limits)
    pub fn can_increase_position(&self, side: crate::OrderSide, size: FixedPoint) -> bool {
        let current = self.position.quantity.to_f64();
        let delta = match side {
            crate::OrderSide::Bid => size.to_f64(),  // Buying increases
            crate::OrderSide::Ask => -size.to_f64(), // Selling decreases
        };

        let new_position = current + delta;
        new_position.abs() <= self.config.max_position_size
    }

    /// Get recommended action based on inventory and market conditions
    pub fn recommended_action(&self, market_drift_bps: f64) -> InventoryAction {
        let inventory = self.position.quantity.to_f64();
        let urgency = self.urgency();

        debug!(
            inventory = %inventory,
            target = %self.config.target_inventory,
            urgency = %urgency,
            market_drift_bps = %market_drift_bps,
            "Determining inventory action"
        );

        if urgency > 1.0 {
            // Emergency: position limit breached
            info!(
                urgency = %urgency,
                inventory = %inventory,
                "Emergency unwind required - position limit breached"
            );
            return InventoryAction::EmergencyUnwind;
        }

        if urgency > 0.7 {
            // High urgency: aggressively unwind
            info!(
                urgency = %urgency,
                inventory = %inventory,
                "Aggressive unwind required - high urgency"
            );
            return InventoryAction::AggressiveUnwind;
        }

        // Opportunistic logic
        let action = if inventory > self.config.target_inventory + 0.1 {
            // Long position
            if market_drift_bps < -5.0 {
                // Falling market + long = bad, unwind aggressively
                info!("Long position + falling market = aggressive unwind");
                InventoryAction::AggressiveUnwind
            } else if market_drift_bps > 5.0 {
                // Rising market + long = good, accumulate more if possible
                info!("Long position + rising market = accumulate");
                InventoryAction::Accumulate
            } else {
                // Neutral market, passively unwind
                debug!("Long position + neutral market = passive unwind");
                InventoryAction::PassiveUnwind
            }
        } else if inventory < self.config.target_inventory - 0.1 {
            // Short position
            if market_drift_bps > 5.0 {
                // Rising market + short = bad, unwind aggressively
                info!("Short position + rising market = aggressive unwind");
                InventoryAction::AggressiveUnwind
            } else if market_drift_bps < -5.0 {
                // Falling market + short = good, accumulate more if possible
                info!("Short position + falling market = accumulate");
                InventoryAction::Accumulate
            } else {
                // Neutral market, passively unwind
                debug!("Short position + neutral market = passive unwind");
                InventoryAction::PassiveUnwind
            }
        } else {
            // Near target inventory
            if urgency > 0.3 {
                debug!("Near target but some urgency = passive unwind");
                InventoryAction::PassiveUnwind
            } else {
                debug!("Near target, low urgency = neutral");
                InventoryAction::Neutral
            }
        };

        info!(
            action = ?action,
            "Selected inventory action"
        );

        action
    }

    /// Calculate PnL metrics for monitoring
    pub fn pnl_metrics(&self, mark_price: FixedPoint) -> PnLMetrics {
        PnLMetrics {
            realized_pnl: self.position.realized_pnl,
            unrealized_pnl: self.position.unrealized_pnl(mark_price),
            total_pnl: self.position.realized_pnl + self.position.unrealized_pnl(mark_price),
            inventory: self.position.quantity,
            avg_entry_price: self.position.avg_entry_price,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryAction {
    /// Emergency unwind (position limit breached)
    EmergencyUnwind,

    /// Aggressively unwind position
    AggressiveUnwind,

    /// Passively unwind position over time
    PassiveUnwind,

    /// Neutral (near target inventory)
    Neutral,

    /// Accumulate more inventory (opportunistic)
    Accumulate,
}

#[derive(Debug, Clone, Copy)]
pub struct PnLMetrics {
    pub realized_pnl: FixedPoint,
    pub unrealized_pnl: FixedPoint,
    pub total_pnl: FixedPoint,
    pub inventory: FixedPoint,
    pub avg_entry_price: FixedPoint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_skew() {
        let config = StrategyConfig { inventory_skew_factor: 0.0001, target_inventory: 0.0, ..Default::default() };

        let mut manager = InventoryManager::new(config);

        // Simulate long position
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(5.0);
        pos.avg_entry_price = FixedPoint::from_f64(100.0);
        manager.update_position(pos);

        // With positive inventory and no drift, should get positive skew (widen ask)
        let skew = manager.inventory_skew_bps(0.0);
        assert!(skew > 0.0);

        // With positive inventory and negative drift (adverse), skew should increase
        let adverse_skew = manager.inventory_skew_bps(-10.0);
        assert!(adverse_skew > skew);

        // With positive inventory and positive drift (favorable), skew should decrease
        let favorable_skew = manager.inventory_skew_bps(10.0);
        assert!(favorable_skew < skew);
    }

    #[test]
    fn test_urgency() {
        let config = StrategyConfig { max_position_size: 10.0, ..Default::default() };

        let mut manager = InventoryManager::new(config);

        // Small position: no urgency
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(2.0);
        manager.update_position(pos);
        assert_eq!(manager.urgency(), 0.0);

        // Medium position: some urgency
        pos.quantity = FixedPoint::from_f64(6.0);
        manager.update_position(pos);
        assert!(manager.urgency() > 0.0 && manager.urgency() < 1.0);

        // Large position: high urgency
        pos.quantity = FixedPoint::from_f64(9.0);
        manager.update_position(pos);
        assert!(manager.urgency() >= 1.0);
    }

    #[test]
    fn test_recommended_action() {
        let config = StrategyConfig { max_position_size: 10.0, target_inventory: 0.0, ..Default::default() };

        let mut manager = InventoryManager::new(config);

        // Long position + falling market = aggressive unwind
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(5.0);
        manager.update_position(pos);
        let action = manager.recommended_action(-10.0);
        assert_eq!(action, InventoryAction::AggressiveUnwind);

        // Long position + rising market = accumulate
        let action = manager.recommended_action(10.0);
        assert_eq!(action, InventoryAction::Accumulate);
    }

    #[test]
    fn test_size_factor_reduction() {
        let config = StrategyConfig { max_position_size: 10.0, ..Default::default() };
        let mut manager = InventoryManager::new(config);

        // Small position: full size
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(2.0);
        manager.update_position(pos);
        assert_eq!(manager.size_factor(), 1.0);

        // Medium position: reduced size
        pos.quantity = FixedPoint::from_f64(6.0);
        manager.update_position(pos);
        let factor = manager.size_factor();
        assert!(factor > 0.5 && factor < 1.0);

        // Large position: minimal size
        pos.quantity = FixedPoint::from_f64(9.0);
        manager.update_position(pos);
        assert!(manager.size_factor() < 0.5);
    }

    #[test]
    fn test_asymmetric_sizing() {
        let config = StrategyConfig { max_position_size: 10.0, base_quote_size: 1.0, ..Default::default() };
        let mut manager = InventoryManager::new(config);

        // Long position: should reduce bid size, keep ask size
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(6.0);
        manager.update_position(pos);
        let (bid_factor, ask_factor) = manager.asymmetric_sizes();
        assert!(bid_factor < ask_factor);

        // Short position: should reduce ask size, keep bid size
        pos.quantity = FixedPoint::from_f64(-6.0);
        manager.update_position(pos);
        let (bid_factor, ask_factor) = manager.asymmetric_sizes();
        assert!(bid_factor > ask_factor);
    }

    #[test]
    fn test_can_increase_position() {
        let config = StrategyConfig { max_position_size: 10.0, ..Default::default() };
        let mut manager = InventoryManager::new(config);

        // Room to grow
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(5.0);
        manager.update_position(pos);
        assert!(manager.can_increase_position(crate::OrderSide::Bid, FixedPoint::from_f64(1.0))); // Can buy more
        assert!(manager.can_increase_position(crate::OrderSide::Ask, FixedPoint::from_f64(1.0))); // Can sell

        // At limit
        pos.quantity = FixedPoint::from_f64(10.0);
        manager.update_position(pos);
        assert!(!manager.can_increase_position(crate::OrderSide::Bid, FixedPoint::from_f64(1.0))); // Cannot buy more
        assert!(manager.can_increase_position(crate::OrderSide::Ask, FixedPoint::from_f64(1.0))); // Can still sell
    }

    #[test]
    fn test_emergency_unwind_action() {
        let config = StrategyConfig { max_position_size: 10.0, ..Default::default() };
        let mut manager = InventoryManager::new(config);

        // Position > limit = emergency unwind
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(11.0); // Over limit
        manager.update_position(pos);
        let action = manager.recommended_action(0.0);
        assert_eq!(action, InventoryAction::EmergencyUnwind);
    }

    #[test]
    fn test_negative_inventory() {
        let config = StrategyConfig { inventory_skew_factor: 0.0001, target_inventory: 0.0, ..Default::default() };
        let mut manager = InventoryManager::new(config);

        // Short position should have negative skew (widen bid)
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(-5.0);
        manager.update_position(pos);
        let skew = manager.inventory_skew_bps(0.0);
        assert!(skew < 0.0);
    }

    #[test]
    fn test_target_inventory_offset() {
        let config = StrategyConfig {
            inventory_skew_factor: 0.0001,
            target_inventory: 2.0, // Target long 2 units
            ..Default::default()
        };
        let mut manager = InventoryManager::new(config);

        // At target: minimal skew
        let mut pos = Position::new();
        pos.quantity = FixedPoint::from_f64(2.0);
        manager.update_position(pos);
        let skew = manager.inventory_skew_bps(0.0);
        assert!(skew.abs() < 0.1); // Near zero

        // Above target: positive skew
        pos.quantity = FixedPoint::from_f64(5.0);
        manager.update_position(pos);
        let skew = manager.inventory_skew_bps(0.0);
        assert!(skew > 0.0);
    }
}
