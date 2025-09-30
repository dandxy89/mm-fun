use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use mm_ratelimit::MultiLimiter;
use mm_ratelimit::RateLimiter;
use serde::Deserialize;

use crate::circuit_breaker::CircuitBreaker;
use crate::circuit_breaker::CircuitBreakerConfig;
use crate::client::HttpClient;
use crate::client::HttpClientConfig;
use crate::errors::HttpError;
use crate::errors::Result;

const BINANCE_BASE_URL: &str = "https://api.binance.com";
const BINANCE_TESTNET_URL: &str = "https://testnet.binance.vision";

/// Binance REST API client for market data and trading
pub struct BinanceClient {
    client: HttpClient,
    base_url: String,
    rate_limiter: Arc<MultiLimiter>,
    circuit_breaker: Arc<CircuitBreaker>,
    #[allow(dead_code)]
    api_key: Option<String>,
    #[allow(dead_code)]
    secret_key: Option<String>,
}

impl BinanceClient {
    /// Create a new Binance client with default configuration
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a new client builder
    pub fn builder() -> BinanceClientBuilder {
        BinanceClientBuilder::default()
    }

    /// Get orderbook depth snapshot
    pub async fn orderbook(&self, symbol: &str, limit: u16) -> Result<OrderbookSnapshot> {
        self.rate_limiter.try_acquire(1).map_err(|_| HttpError::RateLimitExceeded)?;

        let url = format!("{}/api/v3/depth", self.base_url);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).query(&[("symbol", symbol), ("limit", &limit.to_string())]).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                // Use from_slice for more efficient deserialization than .json()
                let bytes = response.bytes().await?;
                let snapshot: OrderbookSnapshot = serde_json::from_slice(&bytes)?;
                Ok(snapshot)
            })
            .await
    }

    /// Get recent trades
    pub async fn recent_trades(&self, symbol: &str, limit: Option<u16>) -> Result<Vec<Trade>> {
        self.rate_limiter.try_acquire(1).map_err(|_| HttpError::RateLimitExceeded)?;

        let url = format!("{}/api/v3/trades", self.base_url);
        let limit = limit.unwrap_or(500).min(1000);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).query(&[("symbol", symbol), ("limit", &limit.to_string())]).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                let bytes = response.bytes().await?;
                let trades: Vec<Trade> = serde_json::from_slice(&bytes)?;
                Ok(trades)
            })
            .await
    }

    /// Get 24-hour ticker price change statistics
    pub async fn ticker_24h(&self, symbol: &str) -> Result<Ticker24h> {
        self.rate_limiter.try_acquire(1).map_err(|_| HttpError::RateLimitExceeded)?;

        let url = format!("{}/api/v3/ticker/24hr", self.base_url);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).query(&[("symbol", symbol)]).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                let bytes = response.bytes().await?;
                let ticker: Ticker24h = serde_json::from_slice(&bytes)?;
                Ok(ticker)
            })
            .await
    }

    /// Get current average price
    pub async fn avg_price(&self, symbol: &str) -> Result<AveragePrice> {
        self.rate_limiter.try_acquire(1).map_err(|_| HttpError::RateLimitExceeded)?;

        let url = format!("{}/api/v3/avgPrice", self.base_url);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).query(&[("symbol", symbol)]).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                let bytes = response.bytes().await?;
                let avg: AveragePrice = serde_json::from_slice(&bytes)?;
                Ok(avg)
            })
            .await
    }

    /// Get exchange information (trading rules, symbol info, etc.)
    pub async fn exchange_info(&self) -> Result<ExchangeInfo> {
        self.rate_limiter.try_acquire(1).map_err(|_| HttpError::RateLimitExceeded)?;

        let url = format!("{}/api/v3/exchangeInfo", self.base_url);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                let bytes = response.bytes().await?;
                let info: ExchangeInfo = serde_json::from_slice(&bytes)?;
                Ok(info)
            })
            .await
    }

    /// Test connectivity to the REST API
    pub async fn ping(&self) -> Result<()> {
        let url = format!("{}/api/v3/ping", self.base_url);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                Ok(())
            })
            .await
    }

    /// Get server time
    pub async fn server_time(&self) -> Result<ServerTime> {
        let url = format!("{}/api/v3/time", self.base_url);

        self.circuit_breaker
            .call_async(|| async {
                let response = self.client.get(&url).send().await?;

                if !response.status().is_success() {
                    return Err(self.handle_error_response(response).await);
                }

                let time: ServerTime = response.json().await?;
                Ok(time)
            })
            .await
    }

    /// Handle error response from Binance API
    async fn handle_error_response(&self, response: reqwest::Response) -> HttpError {
        let status = response.status();

        if let Ok(error) = response.json::<BinanceError>().await {
            HttpError::ApiError { code: error.code, message: error.msg }
        } else {
            HttpError::InvalidResponse(format!("HTTP {}", status))
        }
    }

    /// Get current timestamp in milliseconds
    #[allow(dead_code)]
    fn timestamp_ms() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
    }
}

impl Default for BinanceClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default Binance client")
    }
}

/// Builder for configuring Binance client
pub struct BinanceClientBuilder {
    http_config: HttpClientConfig,
    base_url: String,
    requests_per_second: usize,
    circuit_breaker_config: CircuitBreakerConfig,
    api_key: Option<String>,
    secret_key: Option<String>,
}

impl Default for BinanceClientBuilder {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            base_url: BINANCE_BASE_URL.to_string(),
            requests_per_second: 20, // Conservative default (Binance limit is higher)
            circuit_breaker_config: CircuitBreakerConfig::default(),
            api_key: None,
            secret_key: None,
        }
    }
}

impl BinanceClientBuilder {
    /// Use testnet environment
    pub fn testnet(mut self) -> Self {
        self.base_url = BINANCE_TESTNET_URL.to_string();
        self
    }

    /// Set custom base URL
    pub fn base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    /// Configure HTTP client settings
    pub fn http_config(mut self, config: HttpClientConfig) -> Self {
        self.http_config = config;
        self
    }

    /// Set rate limit (requests per second)
    pub fn rate_limit(mut self, requests_per_second: usize) -> Self {
        self.requests_per_second = requests_per_second;
        self
    }

    /// Configure circuit breaker
    pub fn circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.circuit_breaker_config = config;
        self
    }

    /// Set API credentials for authenticated endpoints
    pub fn credentials(mut self, api_key: String, secret_key: String) -> Self {
        self.api_key = Some(api_key);
        self.secret_key = Some(secret_key);
        self
    }

    /// Use low-latency configuration optimized for trading
    pub fn low_latency(mut self) -> Self {
        self.http_config = HttpClientConfig::low_latency();
        self.circuit_breaker_config = CircuitBreakerConfig::aggressive();
        self.requests_per_second = 50;
        self
    }

    /// Build the Binance client
    pub fn build(self) -> Result<BinanceClient> {
        let client = HttpClient::with_config(self.http_config)?;

        // Use the mm_ratelimit Binance preset
        let rate_limiter = mm_ratelimit::exchanges::binance::spot_limits();

        let circuit_breaker = CircuitBreaker::with_config(self.circuit_breaker_config);

        Ok(BinanceClient {
            client,
            base_url: self.base_url,
            rate_limiter: Arc::new(rate_limiter),
            circuit_breaker: Arc::new(circuit_breaker),
            api_key: self.api_key,
            secret_key: self.secret_key,
        })
    }
}

// Response types using zero-allocation deserialization
// JSON strings are parsed directly to i64 fixed-point (prices/quantities) without allocating
// Uses custom deserializers from mm_binary::serde_helpers

#[derive(Debug, Deserialize)]
pub struct OrderbookSnapshot {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    #[serde(deserialize_with = "mm_binary::serde_helpers::deserialize_price_levels")]
    pub bids: Vec<(i64, i64)>, // [price_fixed, quantity_fixed]
    #[serde(deserialize_with = "mm_binary::serde_helpers::deserialize_price_levels")]
    pub asks: Vec<(i64, i64)>, // [price_fixed, quantity_fixed]
}

#[derive(Debug, Deserialize)]
pub struct Trade {
    pub id: u64,
    #[serde(deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub price: i64,
    #[serde(rename = "qty", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub quantity: i64,
    #[serde(rename = "quoteQty", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub quote_quantity: i64,
    pub time: u64,
    #[serde(rename = "isBuyerMaker")]
    pub is_buyer_maker: bool,
    #[serde(rename = "isBestMatch")]
    pub is_best_match: bool,
}

#[derive(Debug, Deserialize)]
pub struct Ticker24h {
    pub symbol: String,
    #[serde(rename = "priceChange", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub price_change: i64,
    #[serde(rename = "priceChangePercent", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub price_change_percent: i64,
    #[serde(rename = "weightedAvgPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub weighted_avg_price: i64,
    #[serde(rename = "prevClosePrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub prev_close_price: i64,
    #[serde(rename = "lastPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub last_price: i64,
    #[serde(rename = "lastQty", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub last_quantity: i64,
    #[serde(rename = "bidPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub bid_price: i64,
    #[serde(rename = "bidQty", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub bid_quantity: i64,
    #[serde(rename = "askPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub ask_price: i64,
    #[serde(rename = "askQty", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub ask_quantity: i64,
    #[serde(rename = "openPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub open_price: i64,
    #[serde(rename = "highPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub high_price: i64,
    #[serde(rename = "lowPrice", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub low_price: i64,
    #[serde(deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub volume: i64,
    #[serde(rename = "quoteVolume", deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub quote_volume: i64,
    #[serde(rename = "openTime")]
    pub open_time: u64,
    #[serde(rename = "closeTime")]
    pub close_time: u64,
    #[serde(rename = "firstId")]
    pub first_id: u64,
    #[serde(rename = "lastId")]
    pub last_id: u64,
    pub count: u64,
}

#[derive(Debug, Deserialize)]
pub struct AveragePrice {
    pub mins: u64,
    #[serde(deserialize_with = "mm_binary::serde_helpers::deserialize_fixed_point_string")]
    pub price: i64,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeInfo {
    pub timezone: String,
    #[serde(rename = "serverTime")]
    pub server_time: u64,
    pub symbols: Vec<SymbolInfo>,
}

#[derive(Debug, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    pub status: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
}

#[derive(Debug, Deserialize)]
pub struct ServerTime {
    #[serde(rename = "serverTime")]
    pub server_time: u64,
}

#[derive(Debug, Deserialize)]
struct BinanceError {
    code: i64,
    msg: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_default() {
        let builder = BinanceClientBuilder::default();
        assert_eq!(builder.base_url, BINANCE_BASE_URL);
        assert_eq!(builder.requests_per_second, 20);
    }

    #[test]
    fn test_builder_testnet() {
        let builder = BinanceClientBuilder::default().testnet();
        assert_eq!(builder.base_url, BINANCE_TESTNET_URL);
    }

    #[test]
    fn test_builder_low_latency() {
        let builder = BinanceClientBuilder::default().low_latency();
        assert_eq!(builder.requests_per_second, 50);
    }
}
