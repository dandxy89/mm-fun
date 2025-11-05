/// Aeron IPC channel for market data (collector publishes here)
pub const MARKET_DATA_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for market data
pub const MARKET_DATA_STREAM_ID: i32 = 10;

/// Aeron IPC channel for collector state (collector publishes here)
pub const STATE_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for collector state
pub const STATE_STREAM_ID: i32 = 11;

/// Aeron IPC channel for heartbeats (collector publishes here)
pub const HEARTBEAT_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for heartbeat
pub const HEARTBEAT_STREAM_ID: i32 = 12;

/// Aeron IPC channel for trade data (collector publishes here)
pub const TRADE_DATA_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for trade data
pub const TRADE_DATA_STREAM_ID: i32 = 13;

/// Aeron IPC channel for pricing output (mm_pricing publishes here)
pub const PRICING_OUTPUT_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for pricing output
pub const PRICING_OUTPUT_STREAM_ID: i32 = 14;

/// Aeron IPC channel for strategy quotes (mm_strategy publishes here)
pub const STRATEGY_QUOTES_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for strategy quotes
pub const STRATEGY_QUOTES_STREAM_ID: i32 = 15;

/// Aeron IPC channel for order fills (mm_sim_executor publishes here)
pub const ORDER_FILLS_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for order fills
pub const ORDER_FILLS_STREAM_ID: i32 = 16;

/// Aeron IPC channel for position updates (mm_strategy publishes here)
pub const POSITION_CHANNEL: &str = "aeron:ipc";

/// Aeron stream ID for position updates
pub const POSITION_STREAM_ID: i32 = 17;

/// Default channel capacity for bounded channels (can be overridden via env var)
pub fn default_channel_capacity() -> usize {
    std::env::var("CHANNEL_CAPACITY").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000)
}

/// Kept for backwards compatibility
pub const DEFAULT_CHANNEL_CAPACITY: usize = 10_000;

/// State update interval in milliseconds (can be overridden via env var)
pub fn state_update_interval_ms() -> u64 {
    std::env::var("STATE_UPDATE_INTERVAL_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(1_000)
}

/// Default state update interval in milliseconds
pub const STATE_UPDATE_INTERVAL_MS: u64 = 1_000;

/// Heartbeat interval in milliseconds (can be overridden via env var)
pub fn heartbeat_interval_ms() -> u64 {
    std::env::var("HEARTBEAT_INTERVAL_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(1_000)
}

/// Default heartbeat interval in milliseconds
pub const HEARTBEAT_INTERVAL_MS: u64 = 1_000;

/// Heartbeat timeout in milliseconds (can be overridden via env var)
/// Default is 5 seconds (5x the heartbeat interval) to allow for network jitter
pub fn heartbeat_timeout_ms() -> u64 {
    std::env::var("HEARTBEAT_TIMEOUT_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(5_000) // Changed from 1_000 to 5_000 (5x interval)
}

/// Default heartbeat timeout in milliseconds
/// IMPORTANT: Should be at least 3x HEARTBEAT_INTERVAL_MS to avoid false positives
pub const HEARTBEAT_TIMEOUT_MS: u64 = 5_000;

/// Media driver directory (shared between processes)
/// This is where the Aeron media driver stores its control files
pub const MEDIA_DRIVER_DIR: &str = "/dev/shm/aeron";

/// Alternative: UDP channels for network communication
/// Use these instead of IPC if running in separate containers
pub mod udp {
    /// Market data UDP channel
    pub const MARKET_DATA_CHANNEL: &str = "aeron:udp?endpoint=localhost:40123";

    /// State UDP channel
    pub const STATE_CHANNEL: &str = "aeron:udp?endpoint=localhost:40124";

    /// Heartbeat UDP channel
    pub const HEARTBEAT_CHANNEL: &str = "aeron:udp?endpoint=localhost:40125";

    /// Trade data UDP channel
    pub const TRADE_DATA_CHANNEL: &str = "aeron:udp?endpoint=localhost:40126";

    /// Pricing output UDP channel
    pub const PRICING_OUTPUT_CHANNEL: &str = "aeron:udp?endpoint=localhost:40127";

    /// Strategy quotes UDP channel
    pub const STRATEGY_QUOTES_CHANNEL: &str = "aeron:udp?endpoint=localhost:40128";

    /// Order fills UDP channel
    pub const ORDER_FILLS_CHANNEL: &str = "aeron:udp?endpoint=localhost:40129";

    /// Position updates UDP channel
    pub const POSITION_CHANNEL: &str = "aeron:udp?endpoint=localhost:40130";
}
