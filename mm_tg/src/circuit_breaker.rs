use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker to prevent cascading failures
pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure_time: AtomicU64,
    threshold: u32,
    timeout: Duration,
}

impl CircuitBreaker {
    /// Create new circuit breaker
    pub fn new(threshold: u32, timeout: Duration) -> Self {
        Self { failure_count: AtomicU32::new(0), last_failure_time: AtomicU64::new(0), threshold, timeout }
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        let failures = self.failure_count.load(Ordering::Relaxed);

        if failures < self.threshold {
            return CircuitState::Closed;
        }

        let last_failure = self.last_failure_time.load(Ordering::Relaxed);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

        if now - last_failure > self.timeout.as_secs() { CircuitState::HalfOpen } else { CircuitState::Open }
    }

    /// Record successful operation
    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
    }

    /// Record failed operation
    pub fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        self.last_failure_time.store(now, Ordering::Relaxed);
    }

    /// Check if circuit allows request
    pub fn is_available(&self) -> bool {
        matches!(self.state(), CircuitState::Closed | CircuitState::HalfOpen)
    }

    /// Execute function with circuit breaker protection
    pub async fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        if !self.is_available() {
            return Err(CircuitBreakerError::CircuitOpen);
        }

        match f.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(err) => {
                self.record_failure();
                Err(CircuitBreakerError::Failure(err))
            }
        }
    }
}

/// Circuit breaker error types
#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open")]
    CircuitOpen,
    #[error("Operation failed: {0}")]
    Failure(E),
}
