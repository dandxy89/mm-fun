use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use crate::errors::HttpError;
use crate::errors::Result;

/// State of the circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests pass through
    Closed,
    /// Circuit is open, requests are rejected
    Open,
    /// Circuit is half-open, limited requests allowed for testing
    HalfOpen,
}

/// Configuration for circuit breaker behavior
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold to open circuit (e.g., 0.5 = 50% failure rate)
    pub failure_threshold: f64,

    /// Minimum number of requests before circuit can open
    pub minimum_requests: usize,

    /// Duration to keep circuit open before testing recovery
    pub open_timeout: Duration,

    /// Number of successful requests needed to close from half-open
    pub success_threshold: usize,

    /// Time window for tracking statistics
    pub window_duration: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 0.5, // 50% failure rate
            minimum_requests: 10,
            open_timeout: Duration::from_secs(30),
            success_threshold: 3,
            window_duration: Duration::from_secs(60),
        }
    }
}

impl CircuitBreakerConfig {
    /// Configuration for aggressive failure detection (trading systems)
    pub fn aggressive() -> Self {
        Self {
            failure_threshold: 0.3, // 30% failure rate
            minimum_requests: 5,
            open_timeout: Duration::from_secs(10),
            success_threshold: 5,
            window_duration: Duration::from_secs(30),
        }
    }

    /// Configuration for conservative failure detection
    pub fn conservative() -> Self {
        Self {
            failure_threshold: 0.7, // 70% failure rate
            minimum_requests: 20,
            open_timeout: Duration::from_secs(60),
            success_threshold: 2,
            window_duration: Duration::from_secs(120),
        }
    }
}

/// Cicuit breaker for HTTP clients
///
/// Implements the circuit breaker pattern to prevent cascading failures
/// by failing fast when a downstream service is unhealthy.
pub struct CircuitBreaker {
    /// Current state of the circuit
    state: Arc<AtomicUsize>,

    /// Timestamp when circuit was opened (nanos since arbitrary epoch)
    opened_at: Arc<AtomicU64>,

    /// Total number of requests in current window
    total_requests: Arc<AtomicUsize>,

    /// Number of failed requests in current window
    failed_requests: Arc<AtomicUsize>,

    /// Successful requests in half-open state
    half_open_successes: Arc<AtomicUsize>,

    /// Window start timestamp
    window_start: Arc<AtomicU64>,

    /// Configuration
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with default configuration
    pub fn new() -> Self {
        Self::with_config(CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker with custom configuration
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        let now = Self::now_nanos();

        Self {
            state: Arc::new(AtomicUsize::new(CircuitState::Closed as usize)),
            opened_at: Arc::new(AtomicU64::new(0)),
            total_requests: Arc::new(AtomicUsize::new(0)),
            failed_requests: Arc::new(AtomicUsize::new(0)),
            half_open_successes: Arc::new(AtomicUsize::new(0)),
            window_start: Arc::new(AtomicU64::new(now)),
            config,
        }
    }

    /// Check if a request should be allowed through the circuit
    pub fn call<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        // Check if request is allowed
        self.check_allow()?;

        // Execute the function
        match f() {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(err) => {
                self.record_failure();
                Err(err)
            }
        }
    }

    /// Async version of call
    pub async fn call_async<F, Fut, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        // Check if request is allowed
        self.check_allow()?;

        // Execute the async function
        match f().await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(err) => {
                self.record_failure();
                Err(err)
            }
        }
    }

    /// Check if a request should be allowed (without executing)
    pub fn check_allow(&self) -> Result<()> {
        self.reset_window_if_needed();

        let state = self.current_state();

        match state {
            CircuitState::Closed => {
                self.total_requests.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            CircuitState::Open => {
                // Check if enough time has passed to try half-open
                let now = Self::now_nanos();
                let opened_at = self.opened_at.load(Ordering::Relaxed);
                let elapsed = Duration::from_nanos(now - opened_at);

                if elapsed >= self.config.open_timeout {
                    // Transition to half-open
                    self.set_state(CircuitState::HalfOpen);
                    self.half_open_successes.store(0, Ordering::Relaxed);
                    self.total_requests.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                } else {
                    Err(HttpError::CircuitBreakerOpen)
                }
            }
            CircuitState::HalfOpen => {
                // Allow limited requests through
                self.total_requests.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Record a successful request
    pub fn record_success(&self) {
        let state = self.current_state();

        if state == CircuitState::HalfOpen {
            let successes = self.half_open_successes.fetch_add(1, Ordering::Relaxed) + 1;

            if successes >= self.config.success_threshold {
                // Enough successful requests, close the circuit
                self.set_state(CircuitState::Closed);
                self.reset_statistics();
            }
        }
    }

    /// Record a failed request
    pub fn record_failure(&self) {
        let state = self.current_state();

        match state {
            CircuitState::Closed => {
                let failed = self.failed_requests.fetch_add(1, Ordering::Relaxed) + 1;
                let total = self.total_requests.load(Ordering::Relaxed);

                if total >= self.config.minimum_requests {
                    let failure_rate = failed as f64 / total as f64;

                    if failure_rate >= self.config.failure_threshold {
                        self.open_circuit();
                    }
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state reopens the circuit
                self.open_circuit();
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Get the current state of the circuit
    pub fn current_state(&self) -> CircuitState {
        let state_val = self.state.load(Ordering::Relaxed);
        match state_val {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }

    /// Get current statistics
    pub fn stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: self.current_state(),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            failure_rate: self.failure_rate(),
        }
    }

    /// Calculate current failure rate
    fn failure_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }

        let failed = self.failed_requests.load(Ordering::Relaxed);
        failed as f64 / total as f64
    }

    /// Set the circuit state
    fn set_state(&self, new_state: CircuitState) {
        self.state.store(new_state as usize, Ordering::Release);
    }

    /// Open the circuit
    fn open_circuit(&self) {
        self.set_state(CircuitState::Open);
        self.opened_at.store(Self::now_nanos(), Ordering::Release);
    }

    /// Reset statistics for new window
    fn reset_statistics(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.failed_requests.store(0, Ordering::Relaxed);
        self.half_open_successes.store(0, Ordering::Relaxed);
        self.window_start.store(Self::now_nanos(), Ordering::Relaxed);
    }

    /// Reset window if duration has elapsed
    fn reset_window_if_needed(&self) {
        let now = Self::now_nanos();
        let window_start = self.window_start.load(Ordering::Relaxed);
        let elapsed = Duration::from_nanos(now - window_start);

        if elapsed >= self.config.window_duration {
            // Try to reset (race condition is acceptable, worst case is delayed reset)
            if self.window_start.compare_exchange(window_start, now, Ordering::Release, Ordering::Relaxed).is_ok() {
                self.total_requests.store(0, Ordering::Relaxed);
                self.failed_requests.store(0, Ordering::Relaxed);
            }
        }
    }

    /// Get current time in nanoseconds
    fn now_nanos() -> u64 {
        // Use a fixed reference point for relative time measurements
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_nanos() as u64
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about circuit breaker state
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    pub state: CircuitState,
    pub total_requests: usize,
    pub failed_requests: usize,
    pub failure_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_creation() {
        let cb = CircuitBreaker::new();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn test_successful_requests() {
        let cb = CircuitBreaker::new();

        for _ in 0..10 {
            let result = cb.call(|| Ok::<_, HttpError>(42));
            assert!(result.is_ok());
        }

        assert_eq!(cb.current_state(), CircuitState::Closed);
        let stats = cb.stats();
        assert_eq!(stats.total_requests, 10);
        assert_eq!(stats.failed_requests, 0);
    }

    #[test]
    fn test_circuit_opens_on_failures() {
        let config = CircuitBreakerConfig { failure_threshold: 0.5, minimum_requests: 5, ..Default::default() };
        let cb = CircuitBreaker::with_config(config);

        // Record some failures
        for _ in 0..3 {
            let _ = cb.call(|| Err::<i32, _>(HttpError::InvalidResponse("test".into())));
        }

        // Record some successes
        for _ in 0..2 {
            let _ = cb.call(|| Ok::<_, HttpError>(42));
        }

        // Should still be closed (5 requests, 60% failure, but need >50%)
        assert_eq!(cb.current_state(), CircuitState::Closed);

        // One more failure should open it
        let _ = cb.call(|| Err::<i32, _>(HttpError::InvalidResponse("test".into())));

        // Now should be open (6 requests, 66% failure)
        assert_eq!(cb.current_state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_rejects_when_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 0.5,
            minimum_requests: 2,
            open_timeout: Duration::from_secs(3600), // Long timeout for test
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Force circuit open
        let _ = cb.call(|| Err::<i32, _>(HttpError::InvalidResponse("test".into())));
        let _ = cb.call(|| Err::<i32, _>(HttpError::InvalidResponse("test".into())));

        assert_eq!(cb.current_state(), CircuitState::Open);

        // Next request should be rejected without calling function
        let result = cb.check_allow();
        assert!(matches!(result, Err(HttpError::CircuitBreakerOpen)));
    }

    #[test]
    fn test_aggressive_config() {
        let config = CircuitBreakerConfig::aggressive();
        assert_eq!(config.failure_threshold, 0.3);
        assert_eq!(config.minimum_requests, 5);
    }

    #[test]
    fn test_conservative_config() {
        let config = CircuitBreakerConfig::conservative();
        assert_eq!(config.failure_threshold, 0.7);
        assert_eq!(config.minimum_requests, 20);
    }

    #[tokio::test]
    async fn test_async_call() {
        let cb = CircuitBreaker::new();

        let result = cb.call_async(|| async { Ok::<_, HttpError>(42) }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}
