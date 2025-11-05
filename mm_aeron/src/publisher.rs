use std::rc::Rc;
use std::time::Duration;

use bytes::Bytes;
use rusteron_client::*;
use tracing::info;

use crate::errors::AeronError;
use crate::errors::Result;

/// Aeron Publisher for sending binary-encoded messages
pub struct Publisher {
    _aeron: Option<Rc<Aeron>>,
    publication: Option<AeronPublication>,
    channel: Option<String>,
    stream_id: Option<i32>,
}

impl Publisher {
    /// Creates a new Publisher instance
    pub fn new() -> Self {
        Self { _aeron: None, publication: None, channel: None, stream_id: None }
    }

    /// Creates a Publisher from a shared Aeron instance.
    pub fn from_aeron(aeron: Rc<Aeron>, channel: &str, stream_id: i32) -> Result<Self> {
        let publication = aeron
            .async_add_publication(&channel.into_c_string(), stream_id)
            .map_err(|e| AeronError::PublicationFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?
            .poll_blocking(Duration::from_secs(5))
            .map_err(|e| AeronError::PublicationFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?;

        info!("Created publisher for channel '{}', stream {}", channel, stream_id);

        Ok(Self { _aeron: Some(aeron), publication: Some(publication), channel: Some(channel.to_string()), stream_id: Some(stream_id) })
    }

    /// Adds a publication to the specified channel and stream ID.
    pub fn add_publication(&mut self, channel: &str, stream_id: i32) -> Result<()> {
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

        // Create publication (async with blocking poll)
        let publication = aeron
            .async_add_publication(&channel.into_c_string(), stream_id)
            .map_err(|e| AeronError::PublicationFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?
            .poll_blocking(Duration::from_secs(5))
            .map_err(|e| AeronError::PublicationFailed { channel: channel.to_string(), stream_id, message: format!("{:?}", e) })?;

        info!("Created publisher for channel '{}', stream {}", channel, stream_id);

        self._aeron = Some(aeron);
        self.publication = Some(publication);
        self.channel = Some(channel.to_string());
        self.stream_id = Some(stream_id);

        Ok(())
    }

    /// Publishes a binary message
    pub fn publish(&mut self, data: Bytes) -> Result<()> {
        let publication = self.publication.as_mut().ok_or(AeronError::NotConnected)?;

        // Offer message to publication (returns i64 position)
        let position = publication.offer(data.as_ref(), Handlers::no_reserved_value_supplier_handler());

        if position > 0 { Ok(()) } else { Err(AeronError::BackPressure) }
    }

    /// Returns the channel the publisher is connected to
    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// Returns the stream ID the publisher is using
    pub fn stream_id(&self) -> Option<i32> {
        self.stream_id
    }

    /// Checks if the publication is connected to at least one subscriber
    pub fn is_connected(&self) -> bool {
        if let Some(ref publication) = self.publication { publication.is_connected() } else { false }
    }
}

impl Default for Publisher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        if let Some(ref channel) = self.channel {
            if let Some(stream_id) = self.stream_id {
                info!("Dropping Publisher for channel '{}', stream {}", channel, stream_id);
            }
        }
        // Rust's RAII will handle cleanup of Arc<Aeron> and publication
        // The rusteron-client library properly cleans up C++ resources on drop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publisher_creation() {
        let publisher = Publisher::new();
        assert!(publisher.channel().is_none());
        assert!(publisher.stream_id().is_none());
    }
}
