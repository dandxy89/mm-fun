//! Binance exchange rate limit presets
//!
//! Binance enforces multiple simultaneous rate limits:
//! - **RAW_REQUESTS**: Maximum number of requests (regardless of weight)
//! - **REQUEST_WEIGHT**: Weighted requests based on endpoint cost
//! - **ORDERS**: Order placement limits
//!
//! Reference: https://binance-docs.github.io/apidocs/spot/en/#limits

use std::time::Duration;

use crate::FixedWindow;
use crate::LeakyBucket;
use crate::MultiLimiter;

/// Binance Spot API rate limits (default/conservative)
///
/// Limits:
/// - 6000 requests per 5 minutes (raw requests)
/// - 1_200 weight per minute
/// - 50 requests per second (burst protection)
///
pub fn spot_limits() -> MultiLimiter {
    MultiLimiter::builder()
        // Raw requests: 6000 per 5 minutes = 20 per second average
        .with_limiter(LeakyBucket::builder().capacity(6000).rate_per_second(20.0).build())
        // Request weight: 1_200 per minute = 20 per second
        .with_limiter(LeakyBucket::builder().capacity(1_200).rate_per_minute(1_200.0).build())
        // Burst protection: 50 raw requests per second
        .with_limiter(FixedWindow::per_second(50))
        .build()
}

/// Aggressive Binance Spot limits for low-latency trading
///
/// Uses higher limits suitable for verified accounts and market making:
/// - 1_200 weight per minute
/// - 100 requests per second burst
pub fn spot_limits_aggressive() -> MultiLimiter {
    MultiLimiter::builder()
        // Request weight: 1_200 per minute
        .with_limiter(LeakyBucket::builder().capacity(1_200).rate_per_minute(1_200.0).build())
        // Higher burst for market making
        .with_limiter(FixedWindow::per_second(100))
        .build()
}

/// Conservative Binance limits (safe for all accounts)
///
/// - 800 weight per minute (66% of limit)
/// - 30 requests per second
pub fn spot_limits_conservative() -> MultiLimiter {
    MultiLimiter::builder()
        // 66% of weight limit for safety margin
        .with_limiter(LeakyBucket::builder().capacity(800).rate_per_minute(800.0).build())
        // Conservative burst limit
        .with_limiter(FixedWindow::per_second(30))
        .build()
}

/// Binance order placement limits
///
/// Separate limits for order placement:
/// - 100 orders per 10 seconds
/// - 200,000 orders per day
///
pub fn order_limits() -> MultiLimiter {
    MultiLimiter::builder()
        // 100 orders per 10 seconds
        .with_limiter(FixedWindow::new(100, Duration::from_secs(10)))
        // 200,000 orders per day (conservative: use 180,000)
        .with_limiter(LeakyBucket::builder().capacity(180000).rate_per_second(2.08).build()) // 180000/86400
        .build()
}

/// Binance WebSocket connection limits
///
/// Limits for WebSocket stream subscriptions:
/// - 5 connections per IP
/// - 300 subscriptions per connection
pub fn websocket_limits() -> FixedWindow {
    // Connection limit: 5 per IP (not really rate-limited by time)
    FixedWindow::per_hour(5)
}

/// Custom weight-based limiter for Binance
///
/// Create a custom limiter with specific weight capacity and rate.
pub fn custom_weight_limit(weight_per_minute: u32) -> LeakyBucket {
    LeakyBucket::builder().capacity(weight_per_minute).rate_per_minute(weight_per_minute as f64).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimiter;

    #[test]
    fn test_spot_limits() {
        let limiter = spot_limits();

        // Should be able to acquire small weights
        assert!(limiter.try_acquire(1).is_ok());
        assert!(limiter.try_acquire(10).is_ok());

        // Capacity should be limited by most restrictive limiter
        assert!(limiter.capacity() > 0);
    }

    #[test]
    fn test_spot_limits_aggressive() {
        let limiter = spot_limits_aggressive();

        assert!(limiter.try_acquire(1).is_ok());
        assert!(limiter.capacity() >= 100);
    }

    #[test]
    fn test_spot_limits_conservative() {
        let limiter = spot_limits_conservative();

        assert!(limiter.try_acquire(1).is_ok());

        // Should have lower capacity than aggressive
        let capacity = limiter.capacity();
        assert!(capacity <= 800, "Expected <= 800, got {}", capacity);
    }

    #[test]
    fn test_order_limits() {
        let limiter = order_limits();

        // Should be able to place orders
        assert!(limiter.try_acquire(1).is_ok());

        // Capacity limited by 10-second window
        assert!(limiter.capacity() >= 10);
    }

    #[test]
    fn test_custom_weight_limit() {
        let limiter = custom_weight_limit(2400);

        assert_eq!(limiter.capacity(), 2400);
        assert!(limiter.try_acquire(100).is_ok());
        assert_eq!(limiter.available(), 2300);
    }

    #[test]
    fn test_websocket_limits() {
        let limiter = websocket_limits();

        assert_eq!(limiter.capacity(), 5);
    }
}
