use bytes::Bytes;
use zmq::Context;

use crate::errors::Result;
use crate::errors::ZmqError;

/// ZeroMQ Subscriber for receiving binary-encoded messages (synchronous)
pub struct Subscriber {
    socket: Option<zmq::Socket>,
    address: Option<String>,
    // heartbeat_monitor: Option<HeartbeatMonitor>,
}

impl Subscriber {
    /// Creates a new Subscriber instance
    pub fn new() -> Self {
        Self { socket: None, address: None }
    }

    /// Connects the subscriber to the specified address and subscribes to a topic
    pub fn connect(&mut self, address: &str, topic: &str) -> Result<()> {
        let context = Context::new();
        let socket = context.socket(zmq::SUB).map_err(|e| ZmqError::ConnectFailed { address: address.to_string(), source: e })?;

        // Set linger to 0 for immediate cleanup
        socket.set_linger(0).ok();

        socket.connect(address).map_err(|e| ZmqError::ConnectFailed { address: address.to_string(), source: e })?;

        socket.set_subscribe(topic.as_bytes()).map_err(ZmqError::SubscribeFailed)?;

        self.socket = Some(socket);
        self.address = Some(address.to_string());
        Ok(())
    }

    /// Binds the subscriber to the specified address and subscribes to a topic
    pub fn bind(&mut self, address: &str, topic: &str) -> Result<()> {
        let context = Context::new();
        let socket = context.socket(zmq::SUB).map_err(|e| ZmqError::BindFailed { address: address.to_string(), source: e })?;

        // Set linger to 0 for immediate cleanup
        socket.set_linger(0).ok();

        socket.bind(address).map_err(|e| ZmqError::BindFailed { address: address.to_string(), source: e })?;

        socket.set_subscribe(topic.as_bytes()).map_err(ZmqError::SubscribeFailed)?;

        self.socket = Some(socket);
        self.address = Some(address.to_string());
        Ok(())
    }

    /// Receives a binary message
    pub fn receive(&mut self) -> Result<Bytes> {
        let socket = self.socket.as_mut().ok_or(ZmqError::NotConnected)?;

        // Receive all parts of the multipart message
        let mut parts = Vec::new();
        loop {
            let msg = socket.recv_msg(0).map_err(ZmqError::ReceiveFailed)?;
            parts.push(msg);

            // Check if there are more parts
            if !socket.get_rcvmore().unwrap_or(false) {
                break;
            }
        }

        // Get the last part (the actual data)
        let data = parts.into_iter().last().ok_or(ZmqError::EmptyMessage)?;

        Ok(Bytes::copy_from_slice(&data))
    }

    /// Receives a binary message with its topic
    pub fn receive_with_topic(&mut self) -> Result<(Bytes, Bytes)> {
        let socket = self.socket.as_mut().ok_or(ZmqError::NotConnected)?;

        // Receive all parts of the multipart message
        let mut parts = Vec::new();
        loop {
            let msg = socket.recv_msg(0).map_err(ZmqError::ReceiveFailed)?;
            parts.push(msg);

            // Check if there are more parts
            if !socket.get_rcvmore().unwrap_or(false) {
                break;
            }
        }

        if parts.len() < 2 {
            // If only one part, assume no topic (empty topic)
            let data = parts.into_iter().next().ok_or(ZmqError::EmptyMessage)?;
            return Ok((Bytes::new(), Bytes::copy_from_slice(&data)));
        }

        // First part is topic, last part is data
        let mut parts_iter = parts.into_iter();
        let topic = parts_iter.next().unwrap();
        let data = parts_iter.last().unwrap();

        Ok((Bytes::copy_from_slice(&topic), Bytes::copy_from_slice(&data)))
    }

    /// Returns the address the subscriber is connected/bound to
    pub fn address(&self) -> Option<&str> {
        self.address.as_deref()
    }
}

impl Default for Subscriber {
    fn default() -> Self {
        Self::new()
    }
}
