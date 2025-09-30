use thiserror::Error;

#[derive(Error, Debug)]
pub enum ZmqError {
    #[error("Failed to create ZeroMQ context: {0}")]
    ContextCreation(String),

    #[error("Failed to bind socket to address {address}: {source}")]
    BindFailed { address: String, source: zmq::Error },

    #[error("Failed to connect socket to address {address}: {source}")]
    ConnectFailed { address: String, source: zmq::Error },

    #[error("Failed to send message: {0}")]
    SendFailed(zmq::Error),

    #[error("Failed to receive message: {0}")]
    ReceiveFailed(zmq::Error),

    #[error("Failed to subscribe to topic: {0}")]
    SubscribeFailed(zmq::Error),

    #[error("Invalid address format: {0}")]
    InvalidAddress(String),

    #[error("Socket not connected or bound")]
    NotConnected,

    #[error("Received empty message")]
    EmptyMessage,

    #[error("Operation '{operation}' timed out after {timeout_ms}ms")]
    Timeout { operation: String, timeout_ms: u64 },

    #[error("ZeroMQ error: {0}")]
    Zmq(#[from] zmq::Error),
}

pub type Result<T> = std::result::Result<T, ZmqError>;
