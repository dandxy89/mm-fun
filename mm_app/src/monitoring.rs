use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use mm_binary::CollectorStateMessage;
use mm_binary::HeartbeatMessage;
use mm_zmq::Subscriber;
use tracing::debug;
use tracing::warn;

use crate::time_utils;

/// Configuration for heartbeat monitoring
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// ZMQ address to subscribe to
    pub address: String,
    /// Topic to subscribe to
    pub topic: String,
    /// Timeout in milliseconds before considering heartbeat stale
    pub timeout_ms: u64,
    /// How often to check for stale heartbeats
    pub check_interval: Duration,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            address: crate::zmq_config::HEARTBEAT_SUBSCRIBE_ADDR.to_string(),
            topic: crate::zmq_config::HEARTBEAT_TOPIC.to_string(),
            timeout_ms: crate::zmq_config::HEARTBEAT_TIMEOUT_MS,
            check_interval: Duration::from_secs(2),
        }
    }
}

/// Spawns a background thread to monitor heartbeat messages
pub fn spawn_heartbeat_monitor(
    config: HeartbeatConfig,
    running: Arc<AtomicBool>,
) -> Result<(std::thread::JoinHandle<()>, Arc<AtomicU64>), Box<dyn std::error::Error>> {
    // Connect to heartbeat topic
    let mut subscriber = Subscriber::new();
    subscriber.connect(&config.address, &config.topic)?;
    tracing::info!("Heartbeat monitor connected to {} on topic {}", config.address, config.topic);

    let last_heartbeat_timestamp = Arc::new(AtomicU64::new(time_utils::unix_timestamp_ms()));

    let last_hb_clone = Arc::clone(&last_heartbeat_timestamp);
    let handle = std::thread::spawn(move || {
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
    /// ZMQ address to subscribe to
    pub address: String,
    /// Topic to subscribe to
    pub topic: String,
}

impl Default for StateMonitorConfig {
    fn default() -> Self {
        Self { address: crate::zmq_config::STATE_SUBSCRIBE_ADDR.to_string(), topic: crate::zmq_config::COLLECTOR_STATE_TOPIC.to_string() }
    }
}

/// Spawns a background thread to monitor collector state messages
pub fn spawn_state_monitor(
    config: StateMonitorConfig,
    running: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>, Box<dyn std::error::Error>> {
    // Connect to state topic
    let mut subscriber = Subscriber::new();
    subscriber.connect(&config.address, &config.topic)?;
    tracing::info!("State monitor connected to {} on topic {}", config.address, config.topic);

    let handle = std::thread::spawn(move || {
        while running.load(Ordering::Relaxed) {
            match subscriber.receive() {
                Ok(data) => {
                    if let Ok(state_msg) = CollectorStateMessage::from_bytes(&data)
                        && let Ok(state) = state_msg.state()
                    {
                        debug!("[Collector {}] State: {state:?}, Messages: {}", state_msg.connection_id, state_msg.messages_received);
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
        tracing::error!("Heartbeat timeout detected! Last heartbeat was {}ms ago", elapsed);
        true
    } else {
        false
    }
}
