use std::future::Future;
use std::pin::Pin;

use crate::error::Result;

/// Core trait for all rate limiting implementations
pub trait RateLimiter: Send + Sync {
    /// Try to acquire a specified number of tokens/weight without blocking
    fn try_acquire(&self, weight: u32) -> Result<()>;

    /// Try to acquire a single token without blocking
    fn try_acquire_one(&self) -> Result<()> {
        self.try_acquire(1)
    }

    /// Asynchronously wait until tokens become available, then acquire them
    fn acquire(&self, weight: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    /// Asynchronously acquire a single token
    fn acquire_one(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.acquire(1)
    }

    /// Get the number of currently available tokens
    fn available(&self) -> u32;

    /// Get the maximum capacity/quota
    fn capacity(&self) -> u32;

    /// Reset the rate limiter to initial state
    fn reset(&self);
}
