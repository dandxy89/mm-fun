pub mod errors;
pub mod heartbeat;
pub mod publisher;
pub mod subscriber;

pub use errors::Result;
pub use errors::ZmqError;
pub use publisher::Publisher;
pub use subscriber::Subscriber;

/// Default ZeroMQ high water mark (message queue size)
pub const DEFAULT_HWM: i32 = 1000;

/// Common ZeroMQ address patterns
pub mod addresses {
    /// TCP address pattern for binding (all interfaces)
    pub const TCP_BIND: &str = "tcp://*";

    /// TCP address pattern for connecting (localhost)
    pub const TCP_LOCALHOST: &str = "tcp://127.0.0.1";

    /// IPC address pattern (Unix domain sockets)
    pub const IPC_PREFIX: &str = "ipc://";

    /// In-process address pattern
    pub const INPROC_PREFIX: &str = "inproc://";
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU16;
    use std::sync::atomic::Ordering;

    use bytes::Bytes;

    use super::*;

    static PORT_COUNTER: AtomicU16 = AtomicU16::new(15_000);

    fn get_test_port() -> u16 {
        PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    #[test]
    fn test_publisher_creation() {
        let publisher = Publisher::new();
        assert!(publisher.address().is_none());
    }

    #[test]
    fn test_subscriber_creation() {
        let subscriber = Subscriber::new();
        assert!(subscriber.address().is_none());
    }

    #[test]
    fn test_publisher_bind() {
        let mut publisher = Publisher::new();
        let port = get_test_port();
        let addr = format!("tcp://127.0.0.1:{port}");

        let result = publisher.bind(&addr);
        assert!(result.is_ok());
        assert_eq!(publisher.address(), Some(addr.as_str()));
    }

    #[test]
    fn test_subscriber_connect() {
        let mut subscriber = Subscriber::new();
        let port = get_test_port();
        let addr = format!("tcp://127.0.0.1:{port}");

        let result = subscriber.connect(&addr, "test");
        assert!(result.is_ok());
        assert_eq!(subscriber.address(), Some(addr.as_str()));
    }

    #[test]
    fn test_pubsub_single_message() {
        let port = get_test_port();
        let addr = format!("tcp://127.0.0.1:{port}");

        {
            // Setup publisher
            let mut publisher = Publisher::new();
            publisher.bind(&addr).unwrap();

            // Setup subscriber
            let mut subscriber = Subscriber::new();
            subscriber.connect(&addr, "").unwrap();

            // Give ZeroMQ time to establish connection
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Publish message
            let test_data = Bytes::from(vec![1, 2, 3, 4, 5]);
            publisher.publish(test_data.clone()).unwrap();

            // Receive message
            let received = subscriber.receive().unwrap();
            assert_eq!(received, test_data);

            // Explicit drop to ensure cleanup
            drop(subscriber);
            drop(publisher);
        }

        // Give ZMQ time to cleanup
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    #[test]
    fn test_subscriber_not_connected_error() {
        let mut subscriber = Subscriber::new();

        let result = subscriber.receive();
        assert!(result.is_err());

        match result {
            Err(ZmqError::NotConnected) => (),
            _ => panic!("Expected NotConnected error"),
        }
    }

    #[test]
    fn test_publisher_not_connected_error() {
        let mut publisher = Publisher::new();
        let data = Bytes::from(vec![1, 2, 3]);

        let result = publisher.publish(data);
        assert!(result.is_err());

        match result {
            Err(ZmqError::NotConnected) => (),
            _ => panic!("Expected NotConnected error"),
        }
    }

    #[test]
    fn test_address_constants() {
        assert_eq!(addresses::TCP_BIND, "tcp://*");
        assert_eq!(addresses::TCP_LOCALHOST, "tcp://127.0.0.1");
        assert_eq!(addresses::IPC_PREFIX, "ipc://");
        assert_eq!(addresses::INPROC_PREFIX, "inproc://");
    }

    #[test]
    fn test_default_hwm() {
        assert_eq!(DEFAULT_HWM, 1000);
    }
}
