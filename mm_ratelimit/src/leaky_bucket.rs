use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::error::RateLimitError;
use crate::error::Result;
use crate::limiter::RateLimiter;
use crate::time::TimeSource;

/// Leaky bucket rate limiter using lock-free atomic operations
///
/// The leaky bucket algorithm allows tokens to "leak" (refill) continuously
/// at a constant rate. Tokens accumulate up to the bucket's capacity, and
/// requests consume tokens. This provides smooth rate limiting with burst capacity.
pub struct LeakyBucket {
    /// Current number of available tokens (scaled by TOKEN_SCALE)
    tokens: AtomicU32,

    /// Last refill timestamp in nanoseconds
    last_refill: AtomicU64,

    /// Maximum number of tokens (capacity)
    capacity: u32,

    /// Rate of token generation per nanosecond (scaled by RATE_SCALE)
    /// This is pre-computed as: (tokens_per_second * RATE_SCALE) / 1_000_000_000
    rate_per_nano: u64,

    /// Time source for consistent time measurements
    time_source: TimeSource,
}

// Scaling factors for fixed-point arithmetic to maintain precision
const TOKEN_SCALE: u32 = 1000;
const RATE_SCALE: u64 = 1_000_000_000;

impl LeakyBucket {
    /// Create a new leaky bucket rate limiter
    pub fn new(capacity: u32, rate: f64) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        assert!(rate > 0.0, "Rate must be greater than 0");

        let time_source = TimeSource::new();
        let now = time_source.now_nanos();

        // Pre-compute rate per nanosecond with scaling for precision
        // rate_per_nano = (rate * RATE_SCALE * TOKEN_SCALE) / 1_000_000_000
        // This ensures tokens_to_add_scaled will be in TOKEN_SCALE units
        let rate_per_nano = ((rate * RATE_SCALE as f64 * TOKEN_SCALE as f64) / 1_000_000_000.0) as u64;

        Self { tokens: AtomicU32::new(capacity * TOKEN_SCALE), last_refill: AtomicU64::new(now), capacity, rate_per_nano, time_source }
    }

    /// Create a builder for configuring a leaky bucket
    pub fn builder() -> LeakyBucketBuilder {
        LeakyBucketBuilder::new()
    }

    /// Refill tokens based on elapsed time since last refill
    ///
    /// This method is called internally before each acquisition attempt.
    /// It calculates how many tokens should be added based on the elapsed
    /// time and the refill rate.
    #[inline(always)]
    fn refill(&self) {
        let now = self.time_source.now_nanos();
        let last = self.last_refill.load(Ordering::Relaxed);

        // Calculate elapsed time
        let elapsed = now.saturating_sub(last);
        if elapsed == 0 {
            return;
        }

        // Calculate tokens to add using fixed-point arithmetic
        // tokens_to_add = (elapsed * rate_per_nano) / RATE_SCALE
        let tokens_to_add_scaled = (elapsed * self.rate_per_nano) / RATE_SCALE;

        if tokens_to_add_scaled == 0 {
            return;
        }

        // Try to update timestamp using CAS
        // If successful, we own the right to add tokens
        if self.last_refill.compare_exchange(last, now, Ordering::Release, Ordering::Relaxed).is_ok() {
            // Add tokens up to capacity using CAS loop
            loop {
                let current = self.tokens.load(Ordering::Acquire);
                let capacity_scaled = self.capacity * TOKEN_SCALE;

                // Calculate new token count, capped at capacity
                let new_tokens = (current.saturating_add(tokens_to_add_scaled as u32)).min(capacity_scaled);

                if current == new_tokens {
                    // Already at capacity
                    break;
                }

                // Try to update tokens
                match self.tokens.compare_exchange_weak(current, new_tokens, Ordering::Release, Ordering::Relaxed) {
                    Ok(_) => break,
                    Err(_) => continue, // Retry on contention
                }
            }
        }
    }
}

impl RateLimiter for LeakyBucket {
    #[inline]
    fn try_acquire(&self, weight: u32) -> Result<()> {
        if weight == 0 {
            return Ok(());
        }

        // Refill tokens based on elapsed time
        self.refill();

        let required_tokens = weight * TOKEN_SCALE;

        // Try to acquire tokens using CAS loop
        loop {
            let current = self.tokens.load(Ordering::Acquire);

            if current < required_tokens {
                return Err(RateLimitError::Exceeded);
            }

            // Try to atomically decrement tokens
            match self.tokens.compare_exchange_weak(current, current - required_tokens, Ordering::Release, Ordering::Relaxed) {
                Ok(_) => return Ok(()),
                Err(_) => continue, // CAS failed due to contention, retry
            }
        }
    }

    fn acquire(&self, weight: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut backoff_micros = 1;

            loop {
                match self.try_acquire(weight) {
                    Ok(()) => return Ok(()),
                    Err(_) => {
                        // Exponential backoff up to 10ms
                        let delay = backoff_micros.min(10_000);
                        tokio::time::sleep(Duration::from_micros(delay)).await;
                        backoff_micros = (backoff_micros * 2).min(10_000);
                    }
                }
            }
        })
    }

    fn available(&self) -> u32 {
        self.refill();
        self.tokens.load(Ordering::Relaxed) / TOKEN_SCALE
    }

    fn capacity(&self) -> u32 {
        self.capacity
    }

    fn reset(&self) {
        let now = self.time_source.now_nanos();
        self.tokens.store(self.capacity * TOKEN_SCALE, Ordering::Release);
        self.last_refill.store(now, Ordering::Release);
    }
}

/// Builder for configuring a leaky bucket rate limiter
pub struct LeakyBucketBuilder {
    capacity: Option<u32>,
    rate: Option<f64>,
}

impl LeakyBucketBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { capacity: None, rate: None }
    }

    /// Set the bucket capacity (max tokens)
    pub fn capacity(mut self, capacity: u32) -> Self {
        self.capacity = Some(capacity);
        self
    }

    /// Set the refill rate in tokens per second
    pub fn rate_per_second(mut self, rate: f64) -> Self {
        self.rate = Some(rate);
        self
    }

    /// Set rate in requests per minute
    pub fn rate_per_minute(mut self, rate: f64) -> Self {
        self.rate = Some(rate / 60.0);
        self
    }

    /// Build the leaky bucket
    ///
    /// # Panics
    /// Panics if capacity or rate is not set
    pub fn build(self) -> LeakyBucket {
        let capacity = self.capacity.expect("Capacity must be set");
        let rate = self.rate.expect("Rate must be set");
        LeakyBucket::new(capacity, rate)
    }
}

impl Default for LeakyBucketBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let bucket = LeakyBucket::new(100, 50.0);
        assert_eq!(bucket.capacity(), 100);
        assert_eq!(bucket.available(), 100);
    }

    #[test]
    fn test_try_acquire() {
        let bucket = LeakyBucket::new(10, 50.0);

        // Should succeed
        assert!(bucket.try_acquire(1).is_ok());
        assert_eq!(bucket.available(), 9);

        // Acquire multiple
        assert!(bucket.try_acquire(5).is_ok());
        assert_eq!(bucket.available(), 4);
    }

    #[test]
    fn test_exceeds_limit() {
        let bucket = LeakyBucket::new(5, 100.0);

        // Exhaust tokens
        assert!(bucket.try_acquire(5).is_ok());

        // Should fail
        assert!(matches!(bucket.try_acquire(1), Err(RateLimitError::Exceeded)));
    }

    #[test]
    fn test_refill() {
        let bucket = LeakyBucket::new(100, 100.0); // 1000 tokens/sec for fast refill

        // Exhaust tokens
        assert!(bucket.try_acquire(100).is_ok());
        assert_eq!(bucket.available(), 0);

        // Wait for refill
        std::thread::sleep(Duration::from_millis(200));

        // Should have refilled approximately 20 tokens (1000/sec * 0.05sec)
        let available = bucket.available();
        assert!((15..=25).contains(&available), "Expected ~20, got {available}");
    }

    #[test]
    fn test_builder() {
        let bucket = LeakyBucket::builder().capacity(200).rate_per_second(100.0).build();

        assert_eq!(bucket.capacity(), 200);
        assert_eq!(bucket.available(), 200);
    }

    #[test]
    fn test_builder_per_minute() {
        let bucket = LeakyBucket::builder().capacity(120).rate_per_minute(60.0).build();

        assert_eq!(bucket.capacity(), 120);
    }

    #[test]
    fn test_reset() {
        let bucket = LeakyBucket::new(10, 50.0);

        // Consume tokens
        assert!(bucket.try_acquire(5).is_ok());
        assert_eq!(bucket.available(), 5);

        // Reset
        bucket.reset();
        assert_eq!(bucket.available(), 10);
    }

    #[test]
    fn test_zero_weight() {
        let bucket = LeakyBucket::new(10, 50.0);
        assert!(bucket.try_acquire(0).is_ok());
        assert_eq!(bucket.available(), 10);
    }

    #[tokio::test]
    async fn test_async_acquire() {
        let bucket = LeakyBucket::new(10, 100.0); // 100 tokens/sec

        // Exhaust tokens
        assert!(bucket.try_acquire(10).is_ok());

        // This should wait and eventually succeed (needs ~10ms to refill 1 token)
        let result = tokio::time::timeout(Duration::from_millis(500), bucket.acquire(1)).await;

        assert!(result.is_ok(), "Async acquire timed out");
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;

        let bucket = Arc::new(LeakyBucket::new(1000, 10000.0));
        let mut handles = vec![];

        // Spawn 10 threads each trying to acquire 100 tokens
        for _ in 0..10 {
            let bucket_clone = Arc::clone(&bucket);
            let handle = std::thread::spawn(move || {
                let mut acquired = 0;
                for _ in 0..100 {
                    if bucket_clone.try_acquire(1).is_ok() {
                        acquired += 1;
                    }
                }
                acquired
            });
            handles.push(handle);
        }

        let total: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Should have acquired exactly 1000 tokens (the capacity)
        assert_eq!(total, 1000);
    }
}
