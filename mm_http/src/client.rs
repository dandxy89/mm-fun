use std::time::Duration;

use reqwest::Client;
use reqwest::ClientBuilder;

use crate::errors::Result;

/// Configuration for HTTP client.
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Maximum idle connections per host (default: 50)
    pub pool_max_idle_per_host: usize,

    /// Idle timeout for connections (default: 90s)
    pub pool_idle_timeout: Duration,

    /// Connection establishment timeout (default: 10s)
    pub connect_timeout: Duration,

    /// Total request timeout (default: 30s)
    pub request_timeout: Duration,

    /// TCP keepalive interval (default: 60s)
    pub tcp_keepalive: Duration,

    /// Enable TCP_NODELAY (default: true)
    pub tcp_nodelay: bool,

    /// Enable HTTP/2 prior knowledge (default: false)
    pub http2_prior_knowledge: bool,

    /// HTTP/2 initial stream window size (default: 1MB)
    pub http2_initial_stream_window_size: Option<u32>,

    /// HTTP/2 initial connection window size (default: 2MB)
    pub http2_initial_connection_window_size: Option<u32>,

    /// HTTP/2 adaptive window sizing (default: true)
    pub http2_adaptive_window: bool,

    /// HTTP/2 keep-alive interval (default: 30s)
    pub http2_keep_alive_interval: Duration,

    /// HTTP/2 keep-alive timeout (default: 20s)
    pub http2_keep_alive_timeout: Duration,

    /// Enable Hickory DNS for async resolution (default: true)
    pub hickory_dns: bool,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            pool_max_idle_per_host: 50,
            pool_idle_timeout: Duration::from_secs(90),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(60),
            tcp_nodelay: true,
            http2_prior_knowledge: false,
            http2_initial_stream_window_size: Some(1_000_000),
            http2_initial_connection_window_size: Some(2_000_000),
            http2_adaptive_window: true,
            http2_keep_alive_interval: Duration::from_secs(30),
            http2_keep_alive_timeout: Duration::from_secs(20),
            hickory_dns: true,
        }
    }
}

impl HttpClientConfig {
    /// Configuration with shorter timeouts.
    pub fn low_latency() -> Self {
        Self {
            pool_max_idle_per_host: 10,
            pool_idle_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(10),
            tcp_keepalive: Duration::from_secs(30),
            tcp_nodelay: true,
            http2_prior_knowledge: true,
            http2_initial_stream_window_size: Some(1_000_000),
            http2_initial_connection_window_size: Some(2_000_000),
            http2_adaptive_window: true,
            http2_keep_alive_interval: Duration::from_secs(20),
            http2_keep_alive_timeout: Duration::from_secs(10),
            hickory_dns: true,
        }
    }

    /// Configuration with larger connection pool.
    pub fn high_throughput() -> Self {
        Self { pool_max_idle_per_host: 100, pool_idle_timeout: Duration::from_secs(120), ..Default::default() }
    }
}

pub struct HttpClient {
    client: Client,
    config: HttpClientConfig,
}

impl HttpClient {
    /// Create a new HTTP client with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(HttpClientConfig::default())
    }

    /// Create a new HTTP client with custom configuration
    pub fn with_config(config: HttpClientConfig) -> Result<Self> {
        let mut builder = ClientBuilder::new()
            // Connection pooling
            .pool_max_idle_per_host(config.pool_max_idle_per_host)
            .pool_idle_timeout(config.pool_idle_timeout)
            // TCP optimization
            .tcp_nodelay(config.tcp_nodelay)
            .tcp_keepalive(Some(config.tcp_keepalive))
            // Timeouts
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            // TLS with rustls
            .use_rustls_tls()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            // HTTP/2 optimization
            .http2_adaptive_window(config.http2_adaptive_window)
            .http2_keep_alive_interval(Some(config.http2_keep_alive_interval))
            .http2_keep_alive_timeout(config.http2_keep_alive_timeout)
            // Compression
            .gzip(true)
            .brotli(true);

        // Optional HTTP/2 configurations
        if config.http2_prior_knowledge {
            builder = builder.http2_prior_knowledge();
        }

        if let Some(size) = config.http2_initial_stream_window_size {
            builder = builder.http2_initial_stream_window_size(size);
        }

        if let Some(size) = config.http2_initial_connection_window_size {
            builder = builder.http2_initial_connection_window_size(size);
        }

        // Hickory DNS for async resolution
        if config.hickory_dns {
            builder = builder.hickory_dns(true);
        }

        let client = builder.build()?;

        Ok(Self { client, config })
    }

    /// Get the underlying reqwest client
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Get the client configuration
    pub fn config(&self) -> &HttpClientConfig {
        &self.config
    }

    /// Create a GET request builder
    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    /// Create a POST request builder
    pub fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    /// Create a PUT request builder
    pub fn put(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.put(url)
    }

    /// Create a DELETE request builder
    pub fn delete(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.delete(url)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default HTTP client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HttpClientConfig::default();
        assert_eq!(config.pool_max_idle_per_host, 50);
        assert_eq!(config.pool_idle_timeout, Duration::from_secs(90));
        assert!(config.tcp_nodelay);
        assert!(config.hickory_dns);
    }

    #[test]
    fn test_low_latency_config() {
        let config = HttpClientConfig::low_latency();
        assert_eq!(config.pool_max_idle_per_host, 10);
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert!(config.http2_prior_knowledge);
    }

    #[test]
    fn test_high_throughput_config() {
        let config = HttpClientConfig::high_throughput();
        assert_eq!(config.pool_max_idle_per_host, 100);
        assert_eq!(config.pool_idle_timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_client_creation() {
        let client = HttpClient::new();
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_with_custom_config() {
        let config = HttpClientConfig::low_latency();
        let client = HttpClient::with_config(config);
        assert!(client.is_ok());
    }
}
