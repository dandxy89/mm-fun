use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use parking_lot::Mutex;
use rusteron_client::*;
use tracing::info;

use crate::errors::AeronError;
use crate::errors::Result;

/// Fragment handler that captures messages into a shared buffer
struct MessageCapture {
    buffer: Rc<Mutex<Option<Bytes>>>,
}

impl AeronFragmentHandlerCallback for MessageCapture {
    fn handle_aeron_fragment_handler(&mut self, msg: &[u8], _header: AeronHeader) {
        let mut buffer = self.buffer.lock();
        *buffer = Some(Bytes::copy_from_slice(msg));
    }
}

/// Aeron Subscriber for receiving binary-encoded messages
pub struct Subscriber {
    _aeron: Option<Rc<Aeron>>,
    subscription: Option<AeronSubscription>,
    channel: Option<String>,
    stream_id: Option<i32>,
    handler: Option<Handler<AeronFragmentAssembler>>,
    message_buffer: Rc<Mutex<Option<Bytes>>>,
}

impl Subscriber {
    /// Creates a new Subscriber instance
    pub fn new() -> Self {
        Self { _aeron: None, subscription: None, channel: None, stream_id: None, handler: None, message_buffer: Rc::new(Mutex::new(None)) }
    }

    /// Creates a Subscriber from a shared Aeron instance.
    pub fn from_aeron(aeron: Rc<Aeron>, channel: &str, stream_id: i32) -> Result<Self> {
        let subscription = aeron
            .async_add_subscription(
                &channel.into_c_string(),
                stream_id,
                Handlers::no_available_image_handler(),
                Handlers::no_unavailable_image_handler(),
            )
            .map_err(|e| AeronError::SubscriptionFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?
            .poll_blocking(Duration::from_secs(5))
            .map_err(|e| AeronError::SubscriptionFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?;

        // Create fragment handler
        let message_buffer = Rc::new(Mutex::new(None));
        let message_capture = MessageCapture { buffer: Rc::clone(&message_buffer) };

        let (handler, _inner) = Handler::leak_with_fragment_assembler(message_capture).map_err(|e| AeronError::SubscriptionFailed {
            channel: channel.to_string(),
            stream_id,
            message: format!("{:?}", e),
        })?;

        info!("Created subscriber for channel '{}', stream {}", channel, stream_id);

        Ok(Self {
            _aeron: Some(aeron),
            subscription: Some(subscription),
            channel: Some(channel.to_string()),
            stream_id: Some(stream_id),
            handler: Some(handler),
            message_buffer,
        })
    }

    /// Adds a subscription to the specified channel and stream ID.
    pub fn add_subscription(&mut self, channel: &str, stream_id: i32) -> Result<()> {
        // Create Aeron context
        let context = AeronContext::new().map_err(|_| AeronError::ContextCreationFailed)?;

        // Set aeron directory from environment or use default
        let aeron_dir = std::env::var("AERON_DIR").unwrap_or_else(|_| "/dev/shm/aeron".to_string());

        context.set_dir(&aeron_dir.into_c_string()).map_err(|e| AeronError::ClientCreationFailed(format!("{:?}", e)))?;

        // Create Aeron instance
        let aeron = Aeron::new(&context).map_err(|e| AeronError::ClientCreationFailed(format!("{:?}", e)))?;

        // Start the Aeron client
        aeron.start().map_err(|e| AeronError::ClientCreationFailed(format!("{:?}", e)))?;

        let aeron = Rc::new(aeron);

        // Create subscription (async with blocking poll)
        let subscription = aeron
            .async_add_subscription(
                &channel.into_c_string(),
                stream_id,
                Handlers::no_available_image_handler(),
                Handlers::no_unavailable_image_handler(),
            )
            .map_err(|e| AeronError::SubscriptionFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?
            .poll_blocking(Duration::from_secs(5))
            .map_err(|e| AeronError::SubscriptionFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?;

        // Create fragment handler
        let message_capture = MessageCapture { buffer: Rc::clone(&self.message_buffer) };

        let (handler, _inner) = Handler::leak_with_fragment_assembler(message_capture).map_err(|e| AeronError::SubscriptionFailed {
            channel: channel.to_string(),
            stream_id,
            message: format!("{:?}", e),
        })?;

        info!("Created subscriber for channel '{}', stream {}", channel, stream_id);

        self._aeron = Some(aeron);
        self.subscription = Some(subscription);
        self.handler = Some(handler);
        self.channel = Some(channel.to_string());
        self.stream_id = Some(stream_id);

        Ok(())
    }

    /// Receives a binary message with timeout
    pub fn receive_timeout(&mut self, timeout: Duration) -> Result<Bytes> {
        let start = Instant::now();
        let subscription = self.subscription.as_mut().ok_or(AeronError::SubscriberNotConnected)?;
        let handler = self.handler.as_ref().ok_or(AeronError::SubscriberNotConnected)?;

        // Poll for messages until we receive one or timeout
        loop {
            if start.elapsed() > timeout {
                return Err(AeronError::ReceiveTimeout);
            }

            // Clear the buffer
            {
                let mut buffer = self.message_buffer.lock();
                *buffer = None;
            }

            // Poll for messages
            subscription.poll(Some(handler), 10).map_err(|e| AeronError::PublishFailed(format!("{:?}", e)))?;

            // Check if we received a message
            let mut buffer = self.message_buffer.lock();
            if let Some(data) = buffer.take() {
                return Ok(data);
            }

            // Sleep briefly to avoid busy-waiting (100 microseconds)
            drop(buffer);
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Receives a binary message.
    pub fn receive(&mut self) -> Result<Bytes> {
        self.receive_timeout(Duration::from_secs(30))
    }

    /// Tries to receive a message.
    pub fn try_receive(&mut self) -> Result<Option<Bytes>> {
        let subscription = self.subscription.as_mut().ok_or(AeronError::SubscriberNotConnected)?;
        let handler = self.handler.as_ref().ok_or(AeronError::SubscriberNotConnected)?;

        // Clear the buffer
        {
            let mut buffer = self.message_buffer.lock();
            *buffer = None;
        }

        // Poll for messages (single poll, non-blocking)
        subscription.poll(Some(handler), 10).map_err(|e| AeronError::PublishFailed(format!("{:?}", e)))?;

        // Check if we received a message
        let mut buffer = self.message_buffer.lock();
        Ok(buffer.take())
    }

    /// Returns the channel the subscriber is connected to
    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// Returns the stream ID the subscriber is using
    pub fn stream_id(&self) -> Option<i32> {
        self.stream_id
    }

    /// Checks if the subscription is connected
    pub fn is_connected(&self) -> bool {
        if let Some(ref subscription) = self.subscription { subscription.is_connected() } else { false }
    }
}

impl Default for Subscriber {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        if let Some(ref channel) = self.channel {
            if let Some(stream_id) = self.stream_id {
                info!("Dropping Subscriber for channel '{}', stream {}", channel, stream_id);
            }
        }
        // Rust's RAII will handle cleanup of Arc<Aeron> and subscription
        // The rusteron-client library properly cleans up C++ resources on drop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscriber_creation() {
        let subscriber = Subscriber::new();
        assert!(subscriber.channel().is_none());
        assert!(subscriber.stream_id().is_none());
    }

    #[test]
    fn test_subscriber_not_connected_error() {
        let mut subscriber = Subscriber::new();

        let result = subscriber.try_receive();
        assert!(result.is_err());

        match result {
            Err(AeronError::SubscriberNotConnected) => (),
            _ => panic!("Expected SubscriberNotConnected error"),
        }
    }
}
