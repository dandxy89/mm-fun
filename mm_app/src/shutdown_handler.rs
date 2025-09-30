use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Sets up a Ctrl+C handler that sets the running flag to false on shutdown signal
pub fn setup(running: Arc<AtomicBool>) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        tracing::info!("Shutdown signal received");
        running.store(false, Ordering::Relaxed);
    })
}

/// Sets up a Ctrl+C handler that sets multiple running flags to false on shutdown signal
pub fn setup_multi(flags: Vec<Arc<AtomicBool>>) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        tracing::info!("Shutdown signal received");
        for flag in &flags {
            flag.store(false, Ordering::Relaxed);
        }
    })
}
