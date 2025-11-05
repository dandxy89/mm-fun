use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::Receiver;
use mm_aeron::Publisher;
use tracing::error;
use tracing::info;
use tracing::warn;

const MAX_RETRIES: usize = 100;

/// Configuration for a publisher thread
pub struct PublisherConfig {
    pub channel: String,
    pub stream_id: i32,
    pub name: String,
}

impl PublisherConfig {
    pub fn new(channel: impl Into<String>, stream_id: i32, name: impl Into<String>) -> Self {
        Self { channel: channel.into(), stream_id, name: name.into() }
    }
}

/// Spawn a dedicated publisher thread that receives messages from a channel
///
/// This handles:
/// - Publisher creation inside the thread (avoids Send issues)
/// - Back-pressure detection and retry with exponential backoff
/// - Automatic back-pressure metrics tracking
/// - Error logging
///
/// Returns a JoinHandle for the spawned thread
pub fn spawn_channel_publisher(config: PublisherConfig, rx: Receiver<Bytes>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        // Back-pressure metrics
        static BACKPRESSURE_COUNT: AtomicU64 = AtomicU64::new(0);

        // Create publisher inside thread to avoid Send issues
        let mut pub_instance = Publisher::new();
        if let Err(err) = pub_instance.add_publication(&config.channel, config.stream_id) {
            error!("Failed to add {} publisher: {err}", config.name);
            return;
        }
        info!("{} publisher added on stream {}", config.name, config.stream_id);

        while let Ok(msg_bytes) = rx.recv() {
            match pub_instance.publish(msg_bytes.clone()) {
                Ok(_) => {}
                Err(mm_aeron::AeronError::BackPressure) => {
                    // Log back-pressure and retry with exponential backoff
                    let count = BACKPRESSURE_COUNT.fetch_add(1, Ordering::Relaxed);
                    if count.is_multiple_of(1000) && count > 0 {
                        error!("Back-pressure occurred {count} times on {} - consider increasing buffer sizes", config.name);
                    } else if count.is_multiple_of(100) && count > 0 {
                        warn!("Back-pressure detected on {} publisher (count: {count})", config.name);
                    }

                    // Retry with exponential backoff
                    let mut retry_count = 0;
                    loop {
                        std::thread::sleep(Duration::from_micros(10 * 2u64.pow(retry_count.min(5) as u32)));
                        match pub_instance.publish(msg_bytes.clone()) {
                            Ok(_) => break,
                            Err(mm_aeron::AeronError::BackPressure) if retry_count < MAX_RETRIES => {
                                retry_count += 1;
                            }
                            Err(err) => {
                                error!("Failed to publish {} after {retry_count} retries: {err}", config.name);
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to publish {}: {err}", config.name);
                }
            }
        }

        info!("{} publisher thread exiting", config.name);
    })
}
