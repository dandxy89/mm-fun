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

/// Fixed window rate limiter with hard resets at window boundaries
///
/// The fixed window algorithm divides time into fixed-size windows and allows
/// a maximum number of requests within each window. When a window expires,
/// the counter resets to the limit.
pub struct FixedWindow {
    /// Current request count in this window
    count: AtomicU32,

    /// Window start timestamp in nanoseconds
    window_start: AtomicU64,

    /// Maximum requests allowed per window
    limit: u32,

    /// Window duration in nanoseconds
    window_nanos: u64,

    /// Time source for consistent measurements
    time_source: TimeSource,
}

impl FixedWindow {
    /// Create a new fixed window rate limiter
    pub fn new(limit: u32, window: Duration) -> Self {
        assert!(limit > 0, "Limit must be greater than 0");
        assert!(!window.is_zero(), "Window duration must be greater than 0");

        let time_source = TimeSource::new();
        let now = time_source.now_nanos();

        Self { count: AtomicU32::new(0), window_start: AtomicU64::new(now), limit, window_nanos: window.as_nanos() as u64, time_source }
    }

    /// Create a fixed window limiter with per-second limit
    pub fn per_second(limit: u32) -> Self {
        Self::new(limit, Duration::from_secs(1))
    }

    /// Create a fixed window limiter with per-minute limit
    pub fn per_minute(limit: u32) -> Self {
        Self::new(limit, Duration::from_secs(60))
    }

    /// Create a fixed window limiter with per-hour limit
    pub fn per_hour(limit: u32) -> Self {
        Self::new(limit, Duration::from_secs(3600))
    }

    /// Create a builder for configuring a fixed window limiter
    pub fn builder() -> FixedWindowBuilder {
        FixedWindowBuilder::new()
    }

    /// Check if we need to reset the window and do so if necessary
    #[inline(always)]
    fn check_and_reset_window(&self) -> bool {
        let now = self.time_source.now_nanos();
        let window_start = self.window_start.load(Ordering::Relaxed);

        // Check if we're still in the current window
        let elapsed = now.saturating_sub(window_start);

        if elapsed < self.window_nanos {
            // Still in current window
            return false;
        }

        // Window has expired, try to reset
        // Calculate the new window start (aligned to window boundaries)
        let windows_elapsed = elapsed / self.window_nanos;
        let new_window_start = window_start + (windows_elapsed * self.window_nanos);

        // Try to update window_start using CAS
        // If successful, we own the right to reset the counter
        match self.window_start.compare_exchange(window_start, new_window_start, Ordering::Release, Ordering::Relaxed) {
            Ok(_) => {
                // Successfully claimed window reset, reset counter
                self.count.store(0, Ordering::Release);
                true
            }
            Err(_) => {
                // Another thread already reset the window
                false
            }
        }
    }
}

impl RateLimiter for FixedWindow {
    #[inline]
    fn try_acquire(&self, weight: u32) -> Result<()> {
        if weight == 0 {
            return Ok(());
        }

        // Check and reset window if necessary
        self.check_and_reset_window();

        // Try to increment counter using CAS loop
        loop {
            let current = self.count.load(Ordering::Acquire);

            // Check if adding weight would exceed limit
            let new_count = current.saturating_add(weight);
            if new_count > self.limit {
                return Err(RateLimitError::Exceeded);
            }

            // Try to atomically update counter
            match self.count.compare_exchange_weak(current, new_count, Ordering::Release, Ordering::Relaxed) {
                Ok(_) => return Ok(()),
                Err(_) => {
                    // CAS failed, check if window was reset in the meantime
                    self.check_and_reset_window();
                    continue;
                }
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
                        // Calculate time until next window
                        let now = self.time_source.now_nanos();
                        let window_start = self.window_start.load(Ordering::Relaxed);
                        let elapsed = now.saturating_sub(window_start);
                        let remaining = self.window_nanos.saturating_sub(elapsed);

                        if remaining > 0 {
                            // Wait until next window (with a small buffer)
                            let wait_nanos = remaining.min(self.window_nanos);
                            let wait_micros = (wait_nanos / 1000).max(1);

                            tokio::time::sleep(Duration::from_micros(wait_micros)).await;

                            // Force window check after waiting
                            self.check_and_reset_window();
                        } else {
                            // Window should have already reset, use exponential backoff
                            let delay = backoff_micros.min(1000);
                            tokio::time::sleep(Duration::from_micros(delay)).await;
                            backoff_micros = (backoff_micros * 2).min(1000);
                        }
                    }
                }
            }
        })
    }

    fn available(&self) -> u32 {
        self.check_and_reset_window();
        let current = self.count.load(Ordering::Relaxed);
        self.limit.saturating_sub(current)
    }

    fn capacity(&self) -> u32 {
        self.limit
    }

    fn reset(&self) {
        let now = self.time_source.now_nanos();
        self.count.store(0, Ordering::Release);
        self.window_start.store(now, Ordering::Release);
    }
}

/// Builder for configuring a fixed window rate limiter
pub struct FixedWindowBuilder {
    limit: Option<u32>,
    window: Option<Duration>,
}

impl FixedWindowBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { limit: None, window: None }
    }

    /// Set the limit (max requests per window)
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set the window duration
    pub fn window(mut self, window: Duration) -> Self {
        self.window = Some(window);
        self
    }

    /// Set window to 1 second
    pub fn per_second(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self.window = Some(Duration::from_secs(1));
        self
    }

    /// Set window to 1 minute
    pub fn per_minute(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self.window = Some(Duration::from_secs(60));
        self
    }

    /// Set window to 1 hour
    pub fn per_hour(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self.window = Some(Duration::from_secs(3600));
        self
    }

    /// Build the fixed window limiter
    ///
    /// # Panics
    /// Panics if limit or window is not set
    pub fn build(self) -> FixedWindow {
        let limit = self.limit.expect("Limit must be set");
        let window = self.window.expect("Window must be set");
        FixedWindow::new(limit, window)
    }
}

impl Default for FixedWindowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let limiter = FixedWindow::per_second(100);
        assert_eq!(limiter.capacity(), 100);
        assert_eq!(limiter.available(), 100);
    }

    #[test]
    fn test_try_acquire() {
        let limiter = FixedWindow::per_second(10);

        assert!(limiter.try_acquire(1).is_ok());
        assert_eq!(limiter.available(), 9);

        assert!(limiter.try_acquire(5).is_ok());
        assert_eq!(limiter.available(), 4);
    }

    #[test]
    fn test_exceeds_limit() {
        let limiter = FixedWindow::per_second(5);

        // Use all quota
        assert!(limiter.try_acquire(5).is_ok());

        // Should fail
        assert!(matches!(limiter.try_acquire(1), Err(RateLimitError::Exceeded)));
    }

    #[test]
    fn test_window_reset() {
        let limiter = FixedWindow::new(10, Duration::from_millis(50));

        // Use all quota
        assert!(limiter.try_acquire(10).is_ok());
        assert_eq!(limiter.available(), 0);

        // Wait for window to reset
        std::thread::sleep(Duration::from_millis(60));

        // Should be available again
        assert!(limiter.try_acquire(1).is_ok());
        assert_eq!(limiter.available(), 9);
    }

    #[test]
    fn test_builder() {
        let limiter = FixedWindow::builder().per_minute(1000).build();

        assert_eq!(limiter.capacity(), 1000);
        assert_eq!(limiter.available(), 1000);
    }

    #[test]
    fn test_reset() {
        let limiter = FixedWindow::per_second(10);

        assert!(limiter.try_acquire(5).is_ok());
        assert_eq!(limiter.available(), 5);

        limiter.reset();
        assert_eq!(limiter.available(), 10);
    }

    #[test]
    fn test_zero_weight() {
        let limiter = FixedWindow::per_second(10);
        assert!(limiter.try_acquire(0).is_ok());
        assert_eq!(limiter.available(), 10);
    }

    #[tokio::test]
    async fn test_async_acquire() {
        let limiter = FixedWindow::new(5, Duration::from_millis(50));

        // Exhaust quota
        assert!(limiter.try_acquire(5).is_ok());

        // This should wait for next window
        let result = tokio::time::timeout(Duration::from_millis(100), limiter.acquire(1)).await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;

        let limiter = Arc::new(FixedWindow::per_second(1000));
        let mut handles = vec![];

        // Spawn 10 threads each trying to acquire 150 tokens
        for _ in 0..10 {
            let limiter_clone = Arc::clone(&limiter);
            let handle = std::thread::spawn(move || {
                let mut acquired = 0;
                for _ in 0..150 {
                    if limiter_clone.try_acquire(1).is_ok() {
                        acquired += 1;
                    }
                }
                acquired
            });
            handles.push(handle);
        }

        let total: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Should have acquired exactly 1000 tokens (the limit)
        assert_eq!(total, 1000);
    }

    #[test]
    fn test_per_minute() {
        let limiter = FixedWindow::per_minute(60);
        assert_eq!(limiter.capacity(), 60);
    }

    #[test]
    fn test_per_hour() {
        let limiter = FixedWindow::per_hour(3600);
        assert_eq!(limiter.capacity(), 3600);
    }
}
