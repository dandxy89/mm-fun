/// ZMQ publisher address for market data (collector binds here)
pub const MARKET_DATA_PUBLISH_ADDR: &str = "tcp://*:5555";

/// ZMQ subscriber address for market data (pricing connects here)
pub const MARKET_DATA_SUBSCRIBE_ADDR: &str = "tcp://localhost:5555";

/// ZMQ publisher address for collector state (collector binds here)
pub const STATE_PUBLISH_ADDR: &str = "tcp://*:5556";

/// ZMQ subscriber address for collector state (pricing connects here)
pub const STATE_SUBSCRIBE_ADDR: &str = "tcp://localhost:5556";

/// ZMQ publisher address for heartbeats (collector binds here)
pub const HEARTBEAT_PUBLISH_ADDR: &str = "tcp://*:5557";

/// ZMQ subscriber address for heartbeats (pricing connects here)
pub const HEARTBEAT_SUBSCRIBE_ADDR: &str = "tcp://localhost:5557";

/// Topic name for market data messages
pub const MARKET_DATA_TOPIC: &str = "market_data";

/// Topic name for collector state messages
pub const COLLECTOR_STATE_TOPIC: &str = "collector_state";

/// Topic name for heartbeat messages
pub const HEARTBEAT_TOPIC: &str = "heartbeat";

/// Default channel capacity for bounded channels
pub const DEFAULT_CHANNEL_CAPACITY: usize = 10_000;

/// Default state update interval in milliseconds
pub const STATE_UPDATE_INTERVAL_MS: u64 = 1_000;

/// Default heartbeat interval in milliseconds
pub const HEARTBEAT_INTERVAL_MS: u64 = 1_000;

/// Default heartbeat timeout in milliseconds
pub const HEARTBEAT_TIMEOUT_MS: u64 = 1_000;
