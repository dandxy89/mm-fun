//! Aeron IPC messaging abstraction for market data distribution.

pub mod client;
pub mod errors;
pub mod publisher;
pub mod subscriber;

pub use client::AeronClient;
pub use errors::AeronError;
pub use errors::Result;
pub use publisher::Publisher;
pub use subscriber::Subscriber;

/// Default Aeron channel for IPC (shared memory)
pub const DEFAULT_IPC_CHANNEL: &str = "aeron:ipc";

/// Default Aeron channel for UDP localhost
pub const DEFAULT_UDP_CHANNEL: &str = "aeron:udp?endpoint=localhost:40123";

/// Common stream IDs mapped from ZMQ topics
pub mod stream_ids {
    /// Market data stream (replaces ZMQ market_data topic)
    pub const MARKET_DATA: i32 = 10;

    /// Collector state stream (replaces ZMQ collector_state topic)
    pub const COLLECTOR_STATE: i32 = 11;

    /// Heartbeat stream (replaces ZMQ heartbeat topic)
    pub const HEARTBEAT: i32 = 12;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_id_constants() {
        assert_eq!(stream_ids::MARKET_DATA, 10);
        assert_eq!(stream_ids::COLLECTOR_STATE, 11);
        assert_eq!(stream_ids::HEARTBEAT, 12);
    }

    #[test]
    fn test_channel_constants() {
        assert_eq!(DEFAULT_IPC_CHANNEL, "aeron:ipc");
        assert!(DEFAULT_UDP_CHANNEL.starts_with("aeron:udp"));
    }
}
