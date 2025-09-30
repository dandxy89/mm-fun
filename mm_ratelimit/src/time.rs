use std::time::Instant;

/// Time tracking for rate limiters
///
/// Uses Instant for monotonic time measurements with nanosecond precision.
/// All operations are inlined for maximum performance.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TimeSource {
    /// Epoch for relative time measurements
    epoch: Instant,
}

impl TimeSource {
    /// Create a new time source with current time as epoch
    #[inline(always)]
    pub fn new() -> Self {
        Self { epoch: Instant::now() }
    }

    /// Get current time in nanoseconds since epoch
    #[inline(always)]
    pub fn now_nanos(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Get current time as Instant
    #[inline(always)]
    #[allow(dead_code)]
    pub fn now(&self) -> Instant {
        Instant::now()
    }
}

impl Default for TimeSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert seconds to nanoseconds
#[inline(always)]
#[allow(dead_code)]
pub(crate) const fn secs_to_nanos(secs: u64) -> u64 {
    secs * 1_000_000_000
}

/// Convert milliseconds to nanoseconds
#[inline(always)]
#[allow(dead_code)]
pub(crate) const fn millis_to_nanos(millis: u64) -> u64 {
    millis * 1_000_000
}

/// Convert duration to nanoseconds
#[inline(always)]
#[allow(dead_code)]
pub(crate) fn duration_to_nanos(duration: std::time::Duration) -> u64 {
    duration.as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_source() {
        let ts = TimeSource::new();
        let t1 = ts.now_nanos();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let t2 = ts.now_nanos();

        assert!(t2 > t1);
        assert!(t2 - t1 >= millis_to_nanos(10));
    }

    #[test]
    fn test_conversions() {
        assert_eq!(secs_to_nanos(1), 1_000_000_000);
        assert_eq!(millis_to_nanos(1), 1_000_000);
        assert_eq!(duration_to_nanos(std::time::Duration::from_secs(1)), 1_000_000_000);
    }
}
