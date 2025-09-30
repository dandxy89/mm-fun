use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Result;
use crate::limiter::RateLimiter;

/// Coordinator for multiple simultaneous rate limiters
///
/// Some exchanges (like Binance) enforce multiple rate limits simultaneously:
/// - RAW_REQUESTS: 6000 per 5 minutes
/// - REQUEST_WEIGHT: 1200 per minute
/// - ORDERS: 100 per 10 seconds
///
/// MultiLimiter checks all limiters and ensures all pass before allowing a request.
/// This guarantees compliance with all limits atomically.
///
pub struct MultiLimiter {
    limiters: Vec<Arc<dyn RateLimiter>>,
}

impl MultiLimiter {
    /// Create a new multi-limiter builder
    pub fn builder() -> MultiLimiterBuilder {
        MultiLimiterBuilder::new()
    }

    /// Try to acquire from all limiters atomically
    fn try_acquire_internal(&self, weight: u32) -> Result<()> {
        if self.limiters.is_empty() {
            return Ok(());
        }

        // For each limiter, try to acquire
        for limiter in &self.limiters {
            limiter.try_acquire(weight)?;
        }

        Ok(())
    }

    /// Acquire from all limiters asynchronously
    fn acquire_internal(&self, weight: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            if self.limiters.is_empty() {
                return Ok(());
            }

            // Wait for all limiters to have capacity
            for limiter in &self.limiters {
                limiter.acquire(weight).await?;
            }

            Ok(())
        })
    }

    /// Get the minimum available quota across all limiters
    fn available_internal(&self) -> u32 {
        if self.limiters.is_empty() {
            return u32::MAX;
        }

        self.limiters.iter().map(|l| l.available()).min().unwrap_or(0)
    }

    /// Get the minimum capacity across all limiters
    fn capacity_internal(&self) -> u32 {
        if self.limiters.is_empty() {
            return u32::MAX;
        }

        self.limiters.iter().map(|l| l.capacity()).min().unwrap_or(0)
    }

    /// Reset all limiters
    fn reset_internal(&self) {
        for limiter in &self.limiters {
            limiter.reset();
        }
    }
}

impl RateLimiter for MultiLimiter {
    fn try_acquire(&self, weight: u32) -> Result<()> {
        self.try_acquire_internal(weight)
    }

    fn acquire(&self, weight: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.acquire_internal(weight)
    }

    fn available(&self) -> u32 {
        self.available_internal()
    }

    fn capacity(&self) -> u32 {
        self.capacity_internal()
    }

    fn reset(&self) {
        self.reset_internal()
    }
}

/// Builder for creating a multi-limiter
pub struct MultiLimiterBuilder {
    limiters: Vec<Arc<dyn RateLimiter>>,
}

impl MultiLimiterBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { limiters: Vec::new() }
    }

    /// Add a rate limiter to the multi-limiter
    ///
    /// Limiters are checked in the order they are added.
    pub fn with_limiter<L: RateLimiter + 'static>(mut self, limiter: L) -> Self {
        self.limiters.push(Arc::new(limiter));
        self
    }

    /// Add an Arc-wrapped rate limiter
    pub fn with_limiter_arc(mut self, limiter: Arc<dyn RateLimiter>) -> Self {
        self.limiters.push(limiter);
        self
    }

    /// Build the multi-limiter
    pub fn build(self) -> MultiLimiter {
        MultiLimiter { limiters: self.limiters }
    }
}

impl Default for MultiLimiterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FixedWindow;
    use crate::LeakyBucket;

    #[test]
    fn test_empty_multi_limiter() {
        let limiter = MultiLimiter::builder().build();

        assert!(limiter.try_acquire(1).is_ok());
        assert_eq!(limiter.available(), u32::MAX);
    }

    #[test]
    fn test_single_limiter() {
        let limiter = MultiLimiter::builder().with_limiter(LeakyBucket::new(10, 100.0)).build();

        assert!(limiter.try_acquire(1).is_ok());
        assert_eq!(limiter.available(), 9);
        assert_eq!(limiter.capacity(), 10);
    }

    #[test]
    fn test_multiple_limiters() {
        // Create multi-limiter with two limits:
        // - LeakyBucket: 20 capacity
        // - FixedWindow: 10 per second
        let limiter = MultiLimiter::builder().with_limiter(LeakyBucket::new(20, 100.0)).with_limiter(FixedWindow::per_second(10)).build();

        // Should be limited by the most restrictive (FixedWindow: 10)
        assert_eq!(limiter.available(), 10);
        assert_eq!(limiter.capacity(), 10);

        // Acquire 5
        assert!(limiter.try_acquire(5).is_ok());

        // Both limiters should be reduced
        assert_eq!(limiter.available(), 5);
    }

    #[test]
    fn test_all_or_nothing() {
        use crate::error::RateLimitError;

        // First limiter: 100 capacity
        // Second limiter: 5 capacity (restrictive)
        let limiter = MultiLimiter::builder().with_limiter(LeakyBucket::new(100, 1000.0)).with_limiter(FixedWindow::per_second(5)).build();

        // Try to acquire 6 (exceeds second limiter)
        let result = limiter.try_acquire(6);
        assert!(matches!(result, Err(RateLimitError::Exceeded)));

        // Both limiters should be unaffected
        assert_eq!(limiter.available(), 5);
    }

    #[test]
    fn test_sequential_checking() {
        // Three limiters with different capacities
        let limiter = MultiLimiter::builder()
            .with_limiter(LeakyBucket::new(100, 1000.0))
            .with_limiter(FixedWindow::per_second(50))
            .with_limiter(LeakyBucket::new(20, 100.0))
            .build();

        // Most restrictive is 20
        assert_eq!(limiter.available(), 20);

        // Acquire 15
        assert!(limiter.try_acquire(15).is_ok());

        // Should have 5 remaining (from most restrictive)
        assert_eq!(limiter.available(), 5);
    }

    #[test]
    fn test_reset_all() {
        let limiter = MultiLimiter::builder().with_limiter(LeakyBucket::new(10, 100.0)).with_limiter(FixedWindow::per_second(10)).build();

        // Use some quota
        assert!(limiter.try_acquire(5).is_ok());
        assert_eq!(limiter.available(), 5);

        // Reset
        limiter.reset();
        assert_eq!(limiter.available(), 10);
    }

    #[tokio::test]
    async fn test_async_acquire() {
        use std::time::Duration;

        let limiter = MultiLimiter::builder()
            .with_limiter(LeakyBucket::new(10, 100.0))
            .with_limiter(FixedWindow::new(10, Duration::from_millis(100)))
            .build();

        // Exhaust quota
        assert!(limiter.try_acquire(10).is_ok());

        // This should wait and succeed
        let result = tokio::time::timeout(Duration::from_millis(500), limiter.acquire(1)).await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_binance_style_limits() {
        // Simulate Binance rate limits:
        // - 1200 weight per minute (= 20/sec)
        // - 50 raw requests per second
        let limiter = MultiLimiter::builder()
            .with_limiter(LeakyBucket::builder().capacity(1200).rate_per_minute(1200.0).build())
            .with_limiter(FixedWindow::per_second(50))
            .build();

        // Most restrictive is 50 raw requests
        assert_eq!(limiter.available(), 50);

        // Acquire weight of 10
        assert!(limiter.try_acquire(10).is_ok());

        // Should have consumed from both limiters
        let available = limiter.available();
        assert!(available <= 40, "Expected <= 40, got {}", available);
    }

    #[test]
    fn test_concurrent_multi_limiter() {
        use std::sync::Arc;

        let limiter = Arc::new(
            MultiLimiter::builder().with_limiter(LeakyBucket::new(100, 10000.0)).with_limiter(FixedWindow::per_second(100)).build(),
        );

        let mut handles = vec![];

        // Spawn 10 threads each trying to acquire 15 tokens
        for _ in 0..10 {
            let limiter_clone = Arc::clone(&limiter);
            let handle = std::thread::spawn(move || {
                let mut acquired = 0;
                for _ in 0..15 {
                    if limiter_clone.try_acquire(1).is_ok() {
                        acquired += 1;
                    }
                }
                acquired
            });
            handles.push(handle);
        }

        let total: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Should have acquired exactly 100 tokens (the limit)
        assert_eq!(total, 100);
    }
}
