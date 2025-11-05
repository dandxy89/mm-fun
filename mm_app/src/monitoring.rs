use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use mm_aeron::Subscriber;
use mm_binary::CollectorStateMessage;
use mm_binary::HeartbeatMessage;
use tracing::debug;
use tracing::warn;

use crate::time_utils;

/// Configuration for heartbeat monitoring
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Aeron channel to subscribe to
    pub channel: String,
    /// Aeron stream ID to subscribe to
    pub stream_id: i32,
    /// Timeout in milliseconds before considering heartbeat stale
    pub timeout_ms: u64,
    /// How often to check for stale heartbeats
    pub check_interval: Duration,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            channel: crate::aeron_config::HEARTBEAT_CHANNEL.to_string(),
            stream_id: crate::aeron_config::HEARTBEAT_STREAM_ID,
            timeout_ms: crate::aeron_config::HEARTBEAT_TIMEOUT_MS,
            check_interval: Duration::from_secs(2),
        }
    }
}

/// Spawns a background thread to monitor heartbeat messages
pub fn spawn_heartbeat_monitor(
    config: HeartbeatConfig,
    running: Arc<AtomicBool>,
) -> Result<(std::thread::JoinHandle<()>, Arc<AtomicU64>), Box<dyn std::error::Error>> {
    let last_heartbeat_timestamp = Arc::new(AtomicU64::new(time_utils::unix_timestamp_ms()));

    let last_hb_clone = Arc::clone(&last_heartbeat_timestamp);
    let handle = std::thread::spawn(move || {
        // Create subscriber inside the thread to avoid Send issues
        let mut subscriber = Subscriber::new();
        if let Err(err) = subscriber.add_subscription(&config.channel, config.stream_id) {
            tracing::error!("Failed to subscribe to heartbeat stream: {err}");
            return;
        }
        tracing::info!("Heartbeat monitor subscribed to stream {}", config.stream_id);

        let mut last_sequence: Option<u64> = None;

        while running.load(Ordering::Relaxed) {
            match subscriber.receive() {
                Ok(data) => {
                    if let Ok(hb_msg) = HeartbeatMessage::from_bytes(&data) {
                        let now = time_utils::unix_timestamp_ms();

                        // Update last heartbeat timestamp
                        last_hb_clone.store(hb_msg.timestamp, Ordering::Relaxed);

                        // Calculate latency
                        let latency_ms = now.saturating_sub(hb_msg.timestamp);

                        // Check for sequence gaps
                        if let Some(last_seq) = last_sequence {
                            let expected = last_seq.wrapping_add(1);
                            if hb_msg.sequence != expected {
                                warn!(
                                    "Heartbeat sequence gap detected: expected {expected}, got {} (gap: {})",
                                    hb_msg.sequence,
                                    hb_msg.sequence.saturating_sub(expected)
                                );
                            }
                        }

                        last_sequence = Some(hb_msg.sequence);
                        debug!("Heartbeat received: seq={}, latency={}ms", hb_msg.sequence, latency_ms);
                    }
                }
                Err(err) => {
                    warn!("Heartbeat subscriber error: {err}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        tracing::info!("Heartbeat monitor thread exiting");
    });

    Ok((handle, last_heartbeat_timestamp))
}

/// Configuration for collector state monitoring
#[derive(Debug, Clone)]
pub struct StateMonitorConfig {
    /// Aeron channel to subscribe to
    pub channel: String,
    /// Aeron stream ID to subscribe to
    pub stream_id: i32,
}

impl Default for StateMonitorConfig {
    fn default() -> Self {
        Self { channel: crate::aeron_config::STATE_CHANNEL.to_string(), stream_id: crate::aeron_config::STATE_STREAM_ID }
    }
}

/// Spawns a background thread to monitor collector state messages
pub fn spawn_state_monitor(
    config: StateMonitorConfig,
    running: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>, Box<dyn std::error::Error>> {
    let handle = std::thread::spawn(move || {
        // Create subscriber inside the thread to avoid Send issues
        let mut subscriber = Subscriber::new();
        if let Err(err) = subscriber.add_subscription(&config.channel, config.stream_id) {
            tracing::error!("Failed to subscribe to state stream: {err}");
            return;
        }
        tracing::info!("State monitor subscribed to stream {}", config.stream_id);

        while running.load(Ordering::Relaxed) {
            match subscriber.receive() {
                Ok(data) => {
                    if let Ok(state_msg) = CollectorStateMessage::from_bytes(&data) {
                        if let Ok(state) = state_msg.state() {
                            debug!("[Collector {}] State: {state:?}, Messages: {}", state_msg.connection_id, state_msg.messages_received);
                        }
                    }
                }
                Err(err) => {
                    warn!("State subscriber error: {err}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        tracing::info!("State monitor thread exiting");
    });

    Ok(handle)
}

/// Helper to check if heartbeat is stale and log errors
pub fn is_heartbeat_stale(last_timestamp: &Arc<AtomicU64>, timeout_ms: u64) -> bool {
    let now = time_utils::unix_timestamp_ms();
    let last_hb = last_timestamp.load(Ordering::Relaxed);
    let elapsed = now.saturating_sub(last_hb);

    if elapsed > timeout_ms {
        tracing::error!("Heartbeat timeout detected! Last heartbeat was {elapsed}ms ago");
        true
    } else {
        false
    }
}

/// Return type for setup_default_monitors
type MonitorSetupResult = Result<(std::thread::JoinHandle<()>, std::thread::JoinHandle<()>, Arc<AtomicU64>), Box<dyn std::error::Error>>;

/// Convenience function to setup both state and heartbeat monitors with default configs
///
/// Returns (state_monitor_handle, heartbeat_monitor_handle, last_heartbeat_timestamp)
pub fn setup_default_monitors(running: Arc<AtomicBool>) -> MonitorSetupResult {
    let state_monitor_handle = spawn_state_monitor(StateMonitorConfig::default(), Arc::clone(&running))?;
    let (heartbeat_monitor_handle, last_heartbeat_timestamp) = spawn_heartbeat_monitor(HeartbeatConfig::default(), running)?;

    Ok((state_monitor_handle, heartbeat_monitor_handle, last_heartbeat_timestamp))
}

/// Helper struct to periodically check heartbeat staleness
///
/// This encapsulates the pattern of checking heartbeat staleness at regular intervals
pub struct HeartbeatChecker {
    last_check: Instant,
    check_interval: Duration,
    timeout_ms: u64,
}

impl HeartbeatChecker {
    /// Create a new HeartbeatChecker with default settings
    pub fn new() -> Self {
        Self { last_check: Instant::now(), check_interval: Duration::from_secs(2), timeout_ms: crate::aeron_config::HEARTBEAT_TIMEOUT_MS }
    }

    /// Create a new HeartbeatChecker with custom settings
    pub fn with_config(check_interval: Duration, timeout_ms: u64) -> Self {
        Self { last_check: Instant::now(), check_interval, timeout_ms }
    }

    /// Check heartbeat staleness if enough time has elapsed since last check
    ///
    /// Returns true if heartbeat is stale, false otherwise
    pub fn check_if_needed(&mut self, last_heartbeat: &Arc<AtomicU64>) -> bool {
        if self.last_check.elapsed() > self.check_interval {
            self.last_check = Instant::now();
            is_heartbeat_stale(last_heartbeat, self.timeout_ms)
        } else {
            false
        }
    }
}

impl Default for HeartbeatChecker {
    fn default() -> Self {
        Self::new()
    }
}
