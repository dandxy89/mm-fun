//! Bybit exchange rate limit presets
//!
//! Bybit enforces different rate limits based on endpoint type and account level.
//!
//! Reference: https://bybit-exchange.github.io/docs/v5/rate-limit

use crate::FixedWindow;
use crate::LeakyBucket;
use crate::MultiLimiter;

/// Bybit public API limits
///
/// Public endpoints (market data):
/// - 120 requests per minute
/// - 10 requests per second burst
///
pub fn public_limits() -> MultiLimiter {
    MultiLimiter::builder()
        .with_limiter(LeakyBucket::builder().capacity(120).rate_per_minute(120.0).build())
        .with_limiter(FixedWindow::per_second(10))
        .build()
}

/// Bybit private API limits (default account)
///
/// Private endpoints (trading, account):
/// - 120 requests per minute base
/// - 10 requests per second burst
pub fn private_limits() -> MultiLimiter {
    MultiLimiter::builder()
        .with_limiter(LeakyBucket::builder().capacity(120).rate_per_minute(120.0).build())
        .with_limiter(FixedWindow::per_second(10))
        .build()
}

/// Bybit private API limits for VIP accounts
///
/// Higher limits for VIP accounts:
/// - 600 requests per minute
/// - 50 requests per second burst
pub fn private_limits_vip() -> MultiLimiter {
    MultiLimiter::builder()
        .with_limiter(LeakyBucket::builder().capacity(600).rate_per_minute(600.0).build())
        .with_limiter(FixedWindow::per_second(50))
        .build()
}

/// Bybit order placement limits
///
/// Specific limits for order endpoints:
/// - 100 orders per second for market making
pub fn order_limits() -> FixedWindow {
    FixedWindow::per_second(100)
}

/// Conservative Bybit limits
///
/// - 100 requests per minute (83% of limit)
/// - 8 requests per second
pub fn public_limits_conservative() -> MultiLimiter {
    MultiLimiter::builder()
        .with_limiter(LeakyBucket::builder().capacity(100).rate_per_minute(100.0).build())
        .with_limiter(FixedWindow::per_second(8))
        .build()
}

/// Bybit WebSocket connection limits
///
/// WebSocket limits:
/// - 10 connections per IP
/// - 500 subscriptions per connection
pub fn websocket_limits() -> FixedWindow {
    FixedWindow::per_second(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimiter;

    #[test]
    fn test_public_limits() {
        let limiter = public_limits();
        assert!(limiter.try_acquire(1).is_ok());
        assert!(limiter.capacity() >= 10);
    }

    #[test]
    fn test_private_limits() {
        let limiter = private_limits();
        assert!(limiter.try_acquire(1).is_ok());
    }

    #[test]
    fn test_private_limits_vip() {
        let limiter = private_limits_vip();
        assert!(limiter.try_acquire(1).is_ok());
        assert!(limiter.capacity() >= 50);
    }

    #[test]
    fn test_order_limits() {
        let limiter = order_limits();
        assert_eq!(limiter.capacity(), 100);
    }

    #[test]
    fn test_conservative_limits() {
        let limiter = public_limits_conservative();
        assert!(limiter.try_acquire(1).is_ok());
    }

    #[test]
    fn test_websocket_limits() {
        let limiter = websocket_limits();
        assert_eq!(limiter.capacity(), 10);
    }
}
