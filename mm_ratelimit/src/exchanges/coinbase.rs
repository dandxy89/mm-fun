//! Coinbase exchange rate limit presets
//!
//! Coinbase enforces different rate limits for public and private endpoints.
//!
//! Reference: https://docs.cloud.coinbase.com/exchange/docs/rate-limits

use crate::FixedWindow;

/// Coinbase public API limits
///
/// Public endpoints (market data):
/// - 10 requests per second
///
/// # Example
/// ```
/// use mm_ratelimit::exchanges::coinbase;
/// use mm_ratelimit::RateLimiter;
///
/// let limiter = coinbase::public_limits();
///
/// if limiter.try_acquire(1).is_ok() {
///     // Make public API request
/// }
/// ```
pub fn public_limits() -> FixedWindow {
    FixedWindow::per_second(10)
}

/// Coinbase private API limits (authenticated)
///
/// Private endpoints (trading, account):
/// - 15 requests per second for most accounts
///
/// # Example
/// ```
/// use mm_ratelimit::exchanges::coinbase;
/// use mm_ratelimit::RateLimiter;
///
/// let limiter = coinbase::private_limits();
///
/// if limiter.try_acquire(1).is_ok() {
///     // Make authenticated API request
/// }
/// ```
pub fn private_limits() -> FixedWindow {
    FixedWindow::per_second(15)
}

/// Coinbase Advanced Trade API limits
///
/// Advanced Trade endpoints:
/// - 30 requests per second for retail
/// - Higher limits for institutional accounts
pub fn advanced_trade_limits() -> FixedWindow {
    FixedWindow::per_second(30)
}

/// Conservative Coinbase limits (safe for all accounts)
///
/// - 8 requests per second for public
/// - 12 requests per second for private
pub fn public_limits_conservative() -> FixedWindow {
    FixedWindow::per_second(8)
}

pub fn private_limits_conservative() -> FixedWindow {
    FixedWindow::per_second(12)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimiter;

    #[test]
    fn test_public_limits() {
        let limiter = public_limits();
        assert_eq!(limiter.capacity(), 10);
        assert!(limiter.try_acquire(1).is_ok());
    }

    #[test]
    fn test_private_limits() {
        let limiter = private_limits();
        assert_eq!(limiter.capacity(), 15);
        assert!(limiter.try_acquire(1).is_ok());
    }

    #[test]
    fn test_advanced_trade_limits() {
        let limiter = advanced_trade_limits();
        assert_eq!(limiter.capacity(), 30);
    }

    #[test]
    fn test_conservative_limits() {
        let pub_limiter = public_limits_conservative();
        assert_eq!(pub_limiter.capacity(), 8);

        let priv_limiter = private_limits_conservative();
        assert_eq!(priv_limiter.capacity(), 12);
    }
}
