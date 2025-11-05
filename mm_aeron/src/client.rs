use std::rc::Rc;

use rusteron_client::*;
use tracing::info;

use crate::errors::AeronError;
use crate::errors::Result;
use crate::publisher::Publisher;
use crate::subscriber::Subscriber;

/// Shared Aeron client that can be used to create multiple publishers and subscribers.
///
/// The Aeron client manages a connection to the media driver. Creating multiple
/// client instances is wasteful - the recommended pattern is one Aeron client per
/// thread, with multiple publications/subscriptions sharing that client.
///
/// **Note**: This client is NOT thread-safe (uses `Rc` instead of `Arc`). The underlying
/// Aeron type is not `Send` + `Sync`. If you need to use Aeron across threads, create
/// separate `Publisher`/`Subscriber` instances in each thread using their standalone
/// `add_publication`/`add_subscription` methods.
pub struct AeronClient {
    aeron: Rc<Aeron>,
}

impl AeronClient {
    /// Creates a new shared Aeron client.
    pub fn new() -> Result<Self> {
        let context = AeronContext::new().map_err(|_| AeronError::ContextCreationFailed)?;

        let aeron_dir = std::env::var("AERON_DIR").unwrap_or_else(|_| "/dev/shm/aeron".to_string());

        info!("Creating shared Aeron client with directory: {aeron_dir}");

        context.set_dir(&aeron_dir.into_c_string()).map_err(|err| AeronError::ClientCreationFailed(format!("{err:?}")))?;

        let aeron = Aeron::new(&context).map_err(|err| AeronError::ClientCreationFailed(format!("{err:?}")))?;
        aeron.start().map_err(|err| AeronError::ClientCreationFailed(format!("{err:?}")))?;

        Ok(Self { aeron: Rc::new(aeron) })
    }

    /// Creates a new publisher using this shared client.
    pub fn create_publisher(&self, channel: &str, stream_id: i32) -> Result<Publisher> {
        Publisher::from_aeron(Rc::clone(&self.aeron), channel, stream_id)
    }

    /// Creates a new subscriber using this shared client.
    pub fn create_subscriber(&self, channel: &str, stream_id: i32) -> Result<Subscriber> {
        Subscriber::from_aeron(Rc::clone(&self.aeron), channel, stream_id)
    }

    /// Returns a clone of the underlying Aeron Rc.
    pub fn aeron(&self) -> Rc<Aeron> {
        Rc::clone(&self.aeron)
    }
}

impl Drop for AeronClient {
    fn drop(&mut self) {
        info!("Dropping shared Aeron client");
    }
}
