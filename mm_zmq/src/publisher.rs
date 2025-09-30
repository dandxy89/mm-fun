use bytes::Bytes;
use zmq::Context;

use crate::errors::Result;
use crate::errors::ZmqError;

/// ZeroMQ Publisher for sending binary-encoded messages (synchronous)
pub struct Publisher {
    socket: Option<zmq::Socket>,
    address: Option<String>,
}

impl Publisher {
    /// Creates a new Publisher instance
    pub fn new() -> Self {
        Self { socket: None, address: None }
    }

    /// Binds the publisher to the specified address
    pub fn bind(&mut self, address: &str) -> Result<()> {
        let context = Context::new();
        let socket = context.socket(zmq::PUB).map_err(|e| ZmqError::BindFailed { address: address.to_string(), source: e })?;

        // Set linger to 0 for immediate cleanup
        socket.set_linger(0).ok();

        socket.bind(address).map_err(|e| ZmqError::BindFailed { address: address.to_string(), source: e })?;

        self.socket = Some(socket);
        self.address = Some(address.to_string());
        Ok(())
    }

    /// Connects the publisher to the specified address
    pub fn connect(&mut self, address: &str) -> Result<()> {
        let context = Context::new();
        let socket = context.socket(zmq::PUB).map_err(|e| ZmqError::ConnectFailed { address: address.to_string(), source: e })?;

        socket.connect(address).map_err(|e| ZmqError::ConnectFailed { address: address.to_string(), source: e })?;

        self.socket = Some(socket);
        self.address = Some(address.to_string());
        Ok(())
    }

    /// Publishes a binary message
    pub fn publish(&mut self, data: Bytes) -> Result<()> {
        let socket = self.socket.as_mut().ok_or(ZmqError::NotConnected)?;
        socket.send(&*data, 0).map_err(ZmqError::SendFailed)?;
        Ok(())
    }

    /// Publishes a binary message with a topic prefix
    pub fn publish_with_topic(&mut self, topic: &str, data: Bytes) -> Result<()> {
        let socket = self.socket.as_mut().ok_or(ZmqError::NotConnected)?;

        // Send multipart message: [topic, data]
        socket.send(topic.as_bytes(), zmq::SNDMORE).map_err(ZmqError::SendFailed)?;
        socket.send(&*data, 0).map_err(ZmqError::SendFailed)?;

        Ok(())
    }

    /// Returns the address the publisher is bound/connected to
    pub fn address(&self) -> Option<&str> {
        self.address.as_deref()
    }
}

impl Default for Publisher {
    fn default() -> Self {
        Self::new()
    }
}
