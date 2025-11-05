use thiserror::Error;

/// Result type for Aeron operations
pub type Result<T> = std::result::Result<T, AeronError>;

/// Errors that can occur when using Aeron
#[derive(Error, Debug)]
pub enum AeronError {
    /// Failed to create Aeron context
    #[error("Failed to create Aeron context")]
    ContextCreationFailed,

    /// Failed to create Aeron client
    #[error("Failed to create Aeron client: {0}")]
    ClientCreationFailed(String),

    /// Failed to add publication
    #[error("Failed to add publication to channel {channel} with stream ID {stream_id}: {message}")]
    PublicationFailed { channel: String, stream_id: i32, message: String },

    /// Failed to add subscription
    #[error("Failed to add subscription to channel {channel} with stream ID {stream_id}: {message}")]
    SubscriptionFailed { channel: String, stream_id: i32, message: String },

    /// Failed to publish message
    #[error("Failed to publish message: {0}")]
    PublishFailed(String),

    /// Publication not connected
    #[error("Publication not connected")]
    NotConnected,

    /// Subscription not connected
    #[error("Subscription not connected")]
    SubscriberNotConnected,

    /// No message available
    #[error("No message available")]
    NoMessage,

    /// Buffer too small for offer
    #[error("Offer failed - buffer may be full, back pressure applied")]
    BackPressure,

    /// Invalid channel string
    #[error("Invalid channel string")]
    InvalidChannel,

    /// Receive timeout
    #[error("Receive timeout - no message received within specified duration")]
    ReceiveTimeout,
}
