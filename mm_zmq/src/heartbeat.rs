use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use tokio::task::JoinHandle;
use tokio::time::sleep;

/// Heartbeat configuration
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Interval between heartbeat messages in milliseconds
    pub interval_ms: u64,

    /// Timeout for considering a connection dead in milliseconds
    pub timeout_ms: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_ms: 1000, // 1 second
            timeout_ms: 5000,  // 5 seconds
        }
    }
}

/// Heartbeat monitor that tracks connection health
#[derive(Clone)]
pub struct HeartbeatMonitor {
    last_heartbeat_ns: Arc<AtomicU64>,
    last_sequence: Arc<AtomicU64>,
    config: HeartbeatConfig,
}

impl HeartbeatMonitor {
    /// Creates a new heartbeat monitor
    pub fn new(config: HeartbeatConfig) -> Self {
        Self { last_heartbeat_ns: Arc::new(AtomicU64::new(current_time_nanos())), last_sequence: Arc::new(AtomicU64::new(0)), config }
    }

    /// Records a heartbeat with the given sequence number
    pub fn record_heartbeat(&self, sequence: u64) {
        self.last_heartbeat_ns.store(current_time_nanos(), Ordering::Relaxed);
        self.last_sequence.store(sequence, Ordering::Relaxed);
    }

    /// Returns true if the connection is considered alive
    pub fn is_alive(&self) -> bool {
        let now = current_time_nanos();
        let last = self.last_heartbeat_ns.load(Ordering::Relaxed);
        let elapsed_ms = (now - last) / 1_000_000;
        elapsed_ms < self.config.timeout_ms
    }

    /// Returns the time since last heartbeat in milliseconds
    pub fn time_since_last_heartbeat_ms(&self) -> u64 {
        let now = current_time_nanos();
        let last = self.last_heartbeat_ns.load(Ordering::Relaxed);
        (now - last) / 1_000_000
    }

    /// Returns the last heartbeat sequence number
    pub fn last_sequence(&self) -> u64 {
        self.last_sequence.load(Ordering::Relaxed)
    }

    /// Detects if heartbeat messages were missed (gap in sequence)
    pub fn check_sequence_gap(&self, new_sequence: u64) -> Option<u64> {
        let last = self.last_sequence.load(Ordering::Relaxed);
        if last == 0 {
            None
        } else {
            let expected = (last + 1) % 256; // u8 wraps at 256
            if new_sequence != expected {
                Some(if new_sequence > expected { new_sequence - expected } else { (256 - expected) + new_sequence })
            } else {
                None
            }
        }
    }
}

/// Heartbeat generator for publishers
pub struct HeartbeatGenerator {
    sequence: Arc<AtomicU64>,
    config: HeartbeatConfig,
    running: Arc<AtomicBool>,
}

impl HeartbeatGenerator {
    /// Creates a new heartbeat generator
    pub fn new(config: HeartbeatConfig) -> Self {
        Self { sequence: Arc::new(AtomicU64::new(0)), config, running: Arc::new(AtomicBool::new(false)) }
    }

    /// Starts the heartbeat generation loop
    pub fn start<F>(&self, mut callback: F) -> JoinHandle<()>
    where
        F: FnMut(u64, u8) + Send + 'static,
    {
        self.running.store(true, Ordering::Relaxed);
        let sequence = Arc::clone(&self.sequence);
        let running = Arc::clone(&self.running);
        let interval_ms = self.config.interval_ms;

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                let seq_u8 = (seq % 256) as u8;
                let timestamp = current_time_nanos() / 1_000_000; // Convert to ms

                callback(timestamp, seq_u8);

                sleep(Duration::from_millis(interval_ms)).await;
            }
        })
    }

    /// Stops the heartbeat generation
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Gets the current sequence number
    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }
}

/// Gets current time in nanoseconds since UNIX epoch
fn current_time_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_monitor_creation() {
        let config = HeartbeatConfig::default();
        let monitor = HeartbeatMonitor::new(config);

        assert!(monitor.is_alive());
    }

    #[test]
    fn test_heartbeat_monitor_timeout() {
        let config = HeartbeatConfig { interval_ms: 100, timeout_ms: 50 };
        let monitor = HeartbeatMonitor::new(config);

        // Initially alive
        assert!(monitor.is_alive());

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(100));

        // Should be dead now
        assert!(!monitor.is_alive());
    }

    #[test]
    fn test_heartbeat_sequence_tracking() {
        let monitor = HeartbeatMonitor::new(HeartbeatConfig::default());

        monitor.record_heartbeat(1);
        assert_eq!(monitor.last_sequence(), 1);

        monitor.record_heartbeat(2);
        assert_eq!(monitor.last_sequence(), 2);

        // Check for gap
        let gap = monitor.check_sequence_gap(5);
        assert_eq!(gap, Some(2)); // Expected 3, got 5, gap of 2
    }

    #[test]
    fn test_heartbeat_sequence_wrap() {
        let monitor = HeartbeatMonitor::new(HeartbeatConfig::default());

        monitor.record_heartbeat(255);
        let gap = monitor.check_sequence_gap(0);
        assert_eq!(gap, None); // 255 -> 0 is expected wrap
    }

    #[tokio::test]
    async fn test_heartbeat_generator() {
        use std::sync::Mutex;

        let config = HeartbeatConfig { interval_ms: 50, timeout_ms: 1000 };
        let generator = HeartbeatGenerator::new(config);

        let counter = Arc::new(Mutex::new(0));
        let counter_clone = Arc::clone(&counter);

        let handle = generator.start(move |_ts, _seq| {
            let mut count = counter_clone.lock().unwrap();
            *count += 1;
        });

        // Wait for a few heartbeats
        sleep(Duration::from_millis(200)).await;

        generator.stop();
        let _ = handle.await;

        let count = *counter.lock().unwrap();
        assert!(count >= 3, "Expected at least 3 heartbeats, got {}", count);
    }
}
