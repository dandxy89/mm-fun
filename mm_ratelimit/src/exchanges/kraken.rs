//! Kraken exchange rate limit presets
//!
//! Kraken uses a tiered rate limiting system based on verification level.
//! The rate limit decreases with each API call and increases over time.
//!
//! Reference: https://docs.kraken.com/rest/#section/Rate-Limits

use crate::LeakyBucket;
use crate::MultiLimiter;

/// Kraken public API limits
///
/// Public endpoints:
/// - 1 request per second baseline
/// - Burst up to 15 requests
///
pub fn public_limits() -> LeakyBucket {
    // Kraken allows bursting but refills slowly
    LeakyBucket::builder().capacity(15).rate_per_second(1.0).build()
}

/// Kraken private API limits for Starter tier
///
/// Starter verification tier:
/// - Starts at 15 points
/// - Refills at 0.33 points per second
/// - Different endpoints cost different amounts (1-4 points)
pub fn private_limits_starter() -> LeakyBucket {
    LeakyBucket::builder().capacity(15).rate_per_second(0.33).build()
}

/// Kraken private API limits for Intermediate tier
///
/// Intermediate verification tier:
/// - Starts at 20 points
/// - Refills at 0.5 points per second
pub fn private_limits_intermediate() -> LeakyBucket {
    LeakyBucket::builder().capacity(20).rate_per_second(0.5).build()
}

/// Kraken private API limits for Pro tier
///
/// Pro verification tier:
/// - Starts at 20 points
/// - Refills at 1.0 point per second
pub fn private_limits_pro() -> LeakyBucket {
    LeakyBucket::builder().capacity(20).rate_per_second(1.0).build()
}

/// Kraken WebSocket connection limits
///
/// WebSocket limits:
/// - 50 connections per IP
pub fn websocket_limits() -> LeakyBucket {
    LeakyBucket::builder().capacity(50).rate_per_second(0.1).build()
}

/// Kraken combined limits (public + private for Starter)
///
/// For applications that use both public and private endpoints
pub fn combined_limits_starter() -> MultiLimiter {
    MultiLimiter::builder().with_limiter(public_limits()).with_limiter(private_limits_starter()).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimiter;

    #[test]
    fn test_public_limits() {
        let limiter = public_limits();
        assert_eq!(limiter.capacity(), 15);
        assert!(limiter.try_acquire(1).is_ok());
    }

    #[test]
    fn test_private_limits_tiers() {
        let starter = private_limits_starter();
        assert_eq!(starter.capacity(), 15);

        let intermediate = private_limits_intermediate();
        assert_eq!(intermediate.capacity(), 20);

        let pro = private_limits_pro();
        assert_eq!(pro.capacity(), 20);
    }

    #[test]
    fn test_combined_limits() {
        let limiter = combined_limits_starter();
        assert!(limiter.try_acquire(1).is_ok());
    }

    #[test]
    fn test_websocket_limits() {
        let limiter = websocket_limits();
        assert_eq!(limiter.capacity(), 50);
    }
}
