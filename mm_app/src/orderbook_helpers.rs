use mm_http::binance::BinanceClient;
use mm_http::binance::OrderbookSnapshot;

/// Fetch orderbook snapshot from Binance
///
/// This is a convenience function that creates a tokio runtime and fetches
/// the snapshot synchronously, which is useful for initialization code in
/// non-async contexts.
pub fn fetch_orderbook_snapshot(symbol: &str, depth: u16) -> Result<OrderbookSnapshot, Box<dyn std::error::Error>> {
    Ok(tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let binance_client = BinanceClient::new()?;
        binance_client.orderbook(symbol, depth).await
    })?)
}
