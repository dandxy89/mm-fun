use std::io;

use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Initialise tracing with non-blocking file appender
pub fn init(app_name: &str, log_dir: &str, default_level: Level) -> WorkerGuard {
    // Create log directory if it doesn't exist
    let _ = std::fs::create_dir_all(log_dir);

    // Set up non-blocking file appender
    // This creates a background thread that handles all I/O
    let file_appender = tracing_appender::rolling::hourly(log_dir, format!("{app_name}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Configure environment filter
    // Respects RUST_LOG env var, falls back to default_level
    let env_filter = EnvFilter::builder().with_default_directive(default_level.into()).from_env_lossy();

    // Build the subscriber with optimized settings
    let fmt_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_target(true) // Include module path
        .with_thread_ids(true) // Useful for multi-threaded debugging
        .with_line_number(true)
        .with_ansi(false) // Disable color codes in files
        .compact(); // Compact format for better performance

    // Install the subscriber globally
    tracing_subscriber::registry().with(env_filter).with(fmt_layer).init();

    guard
}

/// Initialize tracing with both file and stdout output
pub fn init_with_stdout(app_name: &str, log_dir: &str, default_level: Level) -> WorkerGuard {
    let _ = std::fs::create_dir_all(log_dir);

    // File appender (non-blocking)
    let file_appender = tracing_appender::rolling::hourly(log_dir, format!("{app_name}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::builder().with_default_directive(default_level.into()).from_env_lossy();

    // File layer (no ANSI colors)
    let file_layer =
        fmt::layer().with_writer(non_blocking).with_target(true).with_thread_ids(true).with_line_number(true).with_ansi(false).compact();

    // Stdout layer (with ANSI colors for readability)
    let stdout_layer =
        fmt::layer().with_writer(io::stdout).with_target(true).with_thread_ids(true).with_line_number(true).with_ansi(true).compact();

    tracing_subscriber::registry().with(env_filter).with(file_layer).with(stdout_layer).init();

    guard
}
