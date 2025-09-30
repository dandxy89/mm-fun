//! Rate limiting for cryptocurrency exchanges
//!
//! `mm_ratelimit` provides high-performance, lock-free rate limiting implementations
//! designed for latency-critical trading systems. All rate limiters use atomic operations
//! for thread-safe access with sub-microsecond overhead.
//!
//! # Features
//!
//! - **Leaky Bucket**: Continuous token refill at a constant rate with burst capacity
//! - **Fixed Window**: Hard resets at fixed time intervals (per-second, per-minute, etc.)
//! - **Multi-Limiter**: Coordinate multiple simultaneous rate limits (e.g., Binance)
//! - **Exchange Presets**: Pre-configured limiters for major crypto exchanges
//! - **Lock-Free**: All implementations use atomic operations for maximum concurrency
//! - **Zero-Allocation**: Hot paths avoid allocations for minimal latency
//!
//! # Quick Start
//!
//! ```rust
//! use mm_ratelimit::{LeakyBucket, FixedWindow, MultiLimiter, RateLimiter};
//!
//! // Leaky bucket: 100 requests/sec with 150 burst
//! let leaky = LeakyBucket::new(150, 100.0);
//! if leaky.try_acquire(1).is_ok() {
//!     // Request allowed
//! }
//!
//! // Fixed window: 60 requests per minute
//! let fixed = FixedWindow::per_minute(60);
//! if fixed.try_acquire(1).is_ok() {
//!     // Request allowed
//! }
//!
//! // Multi-limiter: Multiple simultaneous limits (Binance-style)
//! let multi = MultiLimiter::builder()
//!     .with_limiter(LeakyBucket::new(1200, 20.0))  // 1200 weight/min
//!     .with_limiter(FixedWindow::per_second(50))    // 50 raw req/sec
//!     .build();
//! ```
//!
//! # Performance
//!
//! All rate limiters are designed for ultra-low latency:
//! - Single-threaded acquisition: < 100ns
//! - Multi-threaded contention: < 500ns
//! - Zero allocations in hot path
//! - Lock-free atomic operations only
//!
//! # Exchange Support
//!
//! Pre-configured limiters for major exchanges are available in the `exchanges` module:
//!
//! ```rust
//! use mm_ratelimit::exchanges::binance;
//!
//! // Binance spot trading limits
//! let limiter = binance::spot_limits();
//! ```

pub mod error;
pub mod exchanges;
pub mod fixed_window;
pub mod leaky_bucket;
pub mod limiter;
pub mod multi_limiter;
mod time;

pub use error::RateLimitError;
pub use error::Result;
pub use fixed_window::FixedWindow;
pub use fixed_window::FixedWindowBuilder;
pub use leaky_bucket::LeakyBucket;
pub use leaky_bucket::LeakyBucketBuilder;
pub use limiter::RateLimiter;
pub use multi_limiter::MultiLimiter;
pub use multi_limiter::MultiLimiterBuilder;
