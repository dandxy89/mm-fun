use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::FixedPoint;
use crate::Position;
use crate::StrategyConfig;
use crate::StrategyQuote;

/// Risk manager for enforcing position limits and circuit breakers
#[derive(Debug, Clone)]
pub struct RiskManager {
    config: StrategyConfig,
    max_daily_loss: Option<FixedPoint>,
    daily_realized_pnl: FixedPoint,
    is_killed: bool,
    kill_reason: Option<String>,
}

impl RiskManager {
    pub fn new(config: StrategyConfig) -> Self {
        Self {
            config,
            max_daily_loss: Some(FixedPoint::from_f64(-1000.0)), // Default $1000 daily loss limit
            daily_realized_pnl: FixedPoint::ZERO,
            is_killed: false,
            kill_reason: None,
        }
    }

    /// Set maximum daily loss (None = unlimited)
    pub fn set_max_daily_loss(&mut self, max_loss: Option<f64>) {
        self.max_daily_loss = max_loss.map(FixedPoint::from_f64);
    }

    /// Update daily PnL (call this on each fill)
    pub fn update_daily_pnl(&mut self, pnl_change: FixedPoint) {
        self.daily_realized_pnl += pnl_change;
        info!(
            pnl_change = %pnl_change.to_f64(),
            daily_pnl = %self.daily_realized_pnl.to_f64(),
            "Updated daily PnL"
        );
    }

    /// Reset daily PnL (call at start of trading day)
    pub fn reset_daily_pnl(&mut self) {
        info!("Resetting daily PnL");
        self.daily_realized_pnl = FixedPoint::ZERO;
    }

    /// Check if position limit is exceeded
    pub fn check_position_limit(&self, position: &Position) -> RiskCheckResult {
        let abs_position = position.quantity.to_f64().abs();

        if abs_position > self.config.max_position_size {
            error!(
                position = %abs_position,
                limit = %self.config.max_position_size,
                "Position limit exceeded"
            );
            return RiskCheckResult::Reject {
                reason: format!("Position limit exceeded: {} > {}", abs_position, self.config.max_position_size),
            };
        }

        debug!(
            position = %abs_position,
            limit = %self.config.max_position_size,
            "Position limit check passed"
        );
        RiskCheckResult::Accept
    }

    /// Check if order size is within limits
    pub fn check_order_size(&self, order_size: FixedPoint) -> RiskCheckResult {
        let abs_size = order_size.to_f64().abs();

        if abs_size > self.config.max_order_size {
            warn!(
                order_size = %abs_size,
                limit = %self.config.max_order_size,
                "Order size exceeds limit"
            );
            return RiskCheckResult::Reject { reason: format!("Order size exceeds limit: {} > {}", abs_size, self.config.max_order_size) };
        }

        if abs_size <= 0.0 {
            warn!("Order size must be positive");
            return RiskCheckResult::Reject { reason: "Order size must be positive".to_string() };
        }

        RiskCheckResult::Accept
    }

    /// Check if daily loss limit is breached
    pub fn check_daily_loss(&self) -> RiskCheckResult {
        if let Some(max_loss) = self.max_daily_loss {
            if self.daily_realized_pnl < max_loss {
                error!(
                    daily_pnl = %self.daily_realized_pnl.to_f64(),
                    limit = %max_loss.to_f64(),
                    "Daily loss limit breached"
                );
                return RiskCheckResult::Reject {
                    reason: format!("Daily loss limit breached: {} < {}", self.daily_realized_pnl.to_f64(), max_loss.to_f64()),
                };
            }
        }

        debug!(
            daily_pnl = %self.daily_realized_pnl.to_f64(),
            "Daily loss check passed"
        );
        RiskCheckResult::Accept
    }

    /// Check if unrealized PnL is within acceptable range
    pub fn check_unrealized_pnl(&self, position: &Position, mark_price: FixedPoint, max_unrealized_loss: f64) -> RiskCheckResult {
        let unrealized = position.unrealized_pnl(mark_price).to_f64();

        if unrealized < -max_unrealized_loss {
            return RiskCheckResult::Reject { reason: format!("Unrealized loss exceeds limit: {} < -{}", unrealized, max_unrealized_loss) };
        }

        RiskCheckResult::Accept
    }

    /// Comprehensive risk check before publishing quotes
    pub fn check_quote(&self, quote: &StrategyQuote, position: &Position, mark_price: FixedPoint) -> RiskCheckResult {
        debug!(
            bid = %quote.bid_price.to_f64(),
            ask = %quote.ask_price.to_f64(),
            "Running comprehensive quote risk checks"
        );

        // Check if killed
        if self.is_killed {
            warn!(
                reason = ?self.kill_reason,
                "Quote rejected: strategy is killed"
            );
            return RiskCheckResult::Reject {
                reason: format!("Strategy killed: {}", self.kill_reason.as_ref().unwrap_or(&"unknown".to_string())),
            };
        }

        // Check daily loss
        if let RiskCheckResult::Reject { reason } = self.check_daily_loss() {
            return RiskCheckResult::Reject { reason };
        }

        // Check position limit
        if let RiskCheckResult::Reject { reason } = self.check_position_limit(position) {
            return RiskCheckResult::Reject { reason };
        }

        // Check bid size
        if let RiskCheckResult::Reject { reason } = self.check_order_size(quote.bid_size) {
            return RiskCheckResult::Reject { reason: format!("Bid: {}", reason) };
        }

        // Check ask size
        if let RiskCheckResult::Reject { reason } = self.check_order_size(quote.ask_size) {
            return RiskCheckResult::Reject { reason: format!("Ask: {}", reason) };
        }

        // Check spread sanity (prevent crossed quotes)
        if quote.bid_price >= quote.ask_price {
            warn!(
                bid = %quote.bid_price.to_f64(),
                ask = %quote.ask_price.to_f64(),
                "Crossed quotes detected"
            );
            return RiskCheckResult::Reject {
                reason: format!("Crossed quotes: bid {} >= ask {}", quote.bid_price.to_f64(), quote.ask_price.to_f64()),
            };
        }

        // Check if prices are reasonable (within 10% of mark)
        let mark = mark_price.to_f64();
        let bid = quote.bid_price.to_f64();
        let ask = quote.ask_price.to_f64();

        if (bid - mark).abs() / mark > 0.1 {
            warn!(
                bid = %bid,
                mark = %mark,
                deviation_pct = %((bid - mark).abs() / mark * 100.0),
                "Bid price too far from mark"
            );
            return RiskCheckResult::Reject { reason: format!("Bid price too far from mark: {} vs {}", bid, mark) };
        }

        if (ask - mark).abs() / mark > 0.1 {
            warn!(
                ask = %ask,
                mark = %mark,
                deviation_pct = %((ask - mark).abs() / mark * 100.0),
                "Ask price too far from mark"
            );
            return RiskCheckResult::Reject { reason: format!("Ask price too far from mark: {} vs {}", ask, mark) };
        }

        // Check confidence
        if quote.confidence < self.config.min_confidence {
            warn!(
                confidence = %quote.confidence,
                min_confidence = %self.config.min_confidence,
                "Confidence too low"
            );
            return RiskCheckResult::Reject {
                reason: format!("Confidence too low: {} < {}", quote.confidence, self.config.min_confidence),
            };
        }

        debug!("All risk checks passed");
        RiskCheckResult::Accept
    }

    /// Kill switch - stops all quoting
    pub fn kill(&mut self, reason: String) {
        error!(
            reason = %reason,
            "KILL SWITCH ACTIVATED"
        );
        self.is_killed = true;
        self.kill_reason = Some(reason);
    }

    /// Resume after kill
    pub fn resume(&mut self) {
        info!("Strategy resumed from kill state");
        self.is_killed = false;
        self.kill_reason = None;
    }

    /// Check if strategy is killed
    pub fn is_killed(&self) -> bool {
        self.is_killed
    }

    /// Get kill reason
    pub fn kill_reason(&self) -> Option<&str> {
        self.kill_reason.as_deref()
    }

    /// Get current daily PnL
    pub fn daily_pnl(&self) -> FixedPoint {
        self.daily_realized_pnl
    }

    /// Check heartbeat (call this when heartbeat is received)
    /// Returns true if heartbeat is healthy
    pub fn check_heartbeat(&mut self, last_heartbeat_ms: u64, current_time_ms: u64) -> bool {
        let _timeout_ms = crate::StrategyConfig::default().min_confidence as u64 * 1000; // Use a reasonable default
        let age_ms = current_time_ms.saturating_sub(last_heartbeat_ms);

        debug!(
            heartbeat_age_ms = %age_ms,
            "Checking heartbeat"
        );

        if age_ms > 5000 {
            // 5 second timeout
            error!(
                heartbeat_age_ms = %age_ms,
                timeout_ms = 5000,
                "Heartbeat timeout - activating kill switch"
            );
            self.kill(format!("Heartbeat timeout: {}ms", age_ms));
            return false;
        }

        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiskCheckResult {
    Accept,
    Reject { reason: String },
}

impl RiskCheckResult {
    pub fn is_accept(&self) -> bool {
        matches!(self, RiskCheckResult::Accept)
    }

    pub fn is_reject(&self) -> bool {
        !self.is_accept()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_limit() {
        let config = StrategyConfig { max_position_size: 10.0, ..Default::default() };

        let manager = RiskManager::new(config);

        let mut position = Position::new();
        position.quantity = FixedPoint::from_f64(5.0);
        assert!(manager.check_position_limit(&position).is_accept());

        position.quantity = FixedPoint::from_f64(15.0);
        assert!(manager.check_position_limit(&position).is_reject());
    }

    #[test]
    fn test_order_size_limit() {
        let config = StrategyConfig { max_order_size: 1.0, ..Default::default() };

        let manager = RiskManager::new(config);

        assert!(manager.check_order_size(FixedPoint::from_f64(0.5)).is_accept());
        assert!(manager.check_order_size(FixedPoint::from_f64(2.0)).is_reject());
        assert!(manager.check_order_size(FixedPoint::from_f64(0.0)).is_reject());
    }

    #[test]
    fn test_daily_loss_limit() {
        let config = StrategyConfig::default();
        let mut manager = RiskManager::new(config);
        manager.set_max_daily_loss(Some(-100.0));

        assert!(manager.check_daily_loss().is_accept());

        manager.update_daily_pnl(FixedPoint::from_f64(-50.0));
        assert!(manager.check_daily_loss().is_accept());

        manager.update_daily_pnl(FixedPoint::from_f64(-60.0)); // Total: -110
        assert!(manager.check_daily_loss().is_reject());
    }

    #[test]
    fn test_kill_switch() {
        let config = StrategyConfig::default();
        let mut manager = RiskManager::new(config);

        assert!(!manager.is_killed());

        manager.kill("Test kill".to_string());
        assert!(manager.is_killed());
        assert_eq!(manager.kill_reason(), Some("Test kill"));

        manager.resume();
        assert!(!manager.is_killed());
    }

    #[test]
    fn test_crossed_quotes() {
        let config = StrategyConfig { min_confidence: 0.5, ..Default::default() };
        let manager = RiskManager::new(config);

        let quote = StrategyQuote {
            timestamp: 0,
            bid_price: FixedPoint::from_f64(101.0),
            bid_size: FixedPoint::from_f64(1.0),
            ask_price: FixedPoint::from_f64(100.0),
            ask_size: FixedPoint::from_f64(1.0),
            fair_value: FixedPoint::from_f64(100.5),
            inventory: FixedPoint::ZERO,
            confidence: 0.8,
        };

        let position = Position::new();
        let mark = FixedPoint::from_f64(100.5);

        let result = manager.check_quote(&quote, &position, mark);
        assert!(result.is_reject());
    }
}
