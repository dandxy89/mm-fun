//! Integration tests for Aeron pub/sub functionality
//!
//! These tests require a running Aeron Media Driver.
//! Run with: `cargo test --test integration_test -- --test-threads=1`
//!
//! Note: Tests are run with --test-threads=1 to avoid conflicts
//! with the shared Aeron media driver.

use std::time::Duration;

use bytes::Bytes;
use mm_aeron::DEFAULT_IPC_CHANNEL;
use mm_aeron::Publisher;
use mm_aeron::Subscriber;

/// Stream ID for testing
const TEST_STREAM_ID: i32 = 999;

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_pubsub_roundtrip() {
    // Create publisher
    let mut publisher = Publisher::new();
    publisher.add_publication(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID).expect("Failed to add publication");

    // Create subscriber
    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID).expect("Failed to add subscription");

    // Wait for connection to establish
    std::thread::sleep(Duration::from_millis(100));

    // Publish test message
    let test_data = Bytes::from_static(b"test message");
    publisher.publish(test_data.clone()).expect("Failed to publish");

    // Receive message with timeout
    let received = subscriber.receive_timeout(Duration::from_secs(5)).expect("Failed to receive message");

    assert_eq!(received, test_data);
}

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_multiple_messages() {
    let mut publisher = Publisher::new();
    publisher.add_publication(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 1).expect("Failed to add publication");

    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 1).expect("Failed to add subscription");

    std::thread::sleep(Duration::from_millis(100));

    // Send multiple messages
    let messages = vec!["message1", "message2", "message3"];
    for msg in &messages {
        let data = Bytes::from(msg.as_bytes().to_vec());
        publisher.publish(data).expect("Failed to publish");
    }

    // Receive all messages
    for expected in &messages {
        let received = subscriber.receive_timeout(Duration::from_secs(5)).expect("Failed to receive");
        assert_eq!(received, Bytes::from(expected.as_bytes()));
    }
}

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_large_message() {
    let mut publisher = Publisher::new();
    publisher.add_publication(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 2).expect("Failed to add publication");

    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 2).expect("Failed to add subscription");

    std::thread::sleep(Duration::from_millis(100));

    // Create a large message (10KB)
    let large_data: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
    let test_data = Bytes::from(large_data);

    publisher.publish(test_data.clone()).expect("Failed to publish");

    let received = subscriber.receive_timeout(Duration::from_secs(5)).expect("Failed to receive");

    assert_eq!(received, test_data);
    assert_eq!(received.len(), 10_000);
}

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_try_receive_non_blocking() {
    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 3).expect("Failed to add subscription");

    std::thread::sleep(Duration::from_millis(100));

    // Try to receive when no messages are available
    let result = subscriber.try_receive().expect("try_receive failed");
    assert!(result.is_none(), "Expected no message");
}

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_receive_timeout() {
    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 4).expect("Failed to add subscription");

    std::thread::sleep(Duration::from_millis(100));

    // Receive with short timeout should timeout
    let result = subscriber.receive_timeout(Duration::from_millis(500));
    assert!(result.is_err(), "Expected timeout error");
}

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_publisher_subscriber_connection_status() {
    let mut publisher = Publisher::new();
    publisher.add_publication(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 5).expect("Failed to add publication");

    // Initially not connected (no subscriber)
    assert!(!publisher.is_connected(), "Publisher should not be connected yet");

    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 5).expect("Failed to add subscription");

    // Wait for connection
    std::thread::sleep(Duration::from_millis(200));

    // Now should be connected
    assert!(publisher.is_connected(), "Publisher should be connected after subscriber joins");
    assert!(subscriber.is_connected(), "Subscriber should be connected");
}

#[test]
#[ignore] // Requires Aeron Media Driver to be running
fn test_concurrent_publishers() {
    use std::thread;

    let mut subscriber = Subscriber::new();
    subscriber.add_subscription(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 6).expect("Failed to add subscription");

    std::thread::sleep(Duration::from_millis(100));

    // Spawn multiple publisher threads
    let handles: Vec<_> = (0..3)
        .map(|i| {
            thread::spawn(move || {
                let mut publisher = Publisher::new();
                publisher.add_publication(DEFAULT_IPC_CHANNEL, TEST_STREAM_ID + 6).expect("Failed to add publication");

                std::thread::sleep(Duration::from_millis(100));

                let msg = format!("message from thread {}", i);
                publisher.publish(Bytes::from(msg.as_bytes().to_vec())).expect("Failed to publish");
            })
        })
        .collect();

    // Wait for all publishers
    for handle in handles {
        handle.join().unwrap();
    }

    // Receive all messages (order not guaranteed)
    let mut received = Vec::new();
    for _ in 0..3 {
        let msg = subscriber.receive_timeout(Duration::from_secs(5)).expect("Failed to receive");
        received.push(String::from_utf8(msg.to_vec()).unwrap());
    }

    // Verify we got all messages
    assert_eq!(received.len(), 3);
    for i in 0..3 {
        let expected = format!("message from thread {}", i);
        assert!(received.contains(&expected), "Missing message from thread {}", i);
    }
}
