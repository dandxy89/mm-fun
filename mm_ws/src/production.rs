use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use mm_orderbook::OrderBook;
use rapidhash::RapidHashMap;

use crate::ingestion::BinanceIngestor;
use crate::ingestion::MultiSymbolIngestor;

/// Configuration for production ingestor
#[derive(Debug, Clone)]
pub struct IngestorConfig {
    pub symbols: Vec<Arc<str>>,
    pub enable_metrics: bool,
    pub max_queue_size: usize,
    pub reconnect_delay: Duration,
    pub metrics_interval: Duration,
}

impl Default for IngestorConfig {
    fn default() -> Self {
        Self {
            symbols: vec![Arc::from("btcusdt")],
            enable_metrics: true,
            max_queue_size: 10_000,
            reconnect_delay: Duration::from_secs(5),
            metrics_interval: Duration::from_secs(1),
        }
    }
}

/// Production-ready ingestor with graceful shutdown and metrics
pub struct ProductionIngestor {
    config: IngestorConfig,
    running: Arc<AtomicBool>,
    messages_processed: Arc<AtomicU64>,
    _last_message_time: Arc<AtomicU64>,
    orderbooks: RapidHashMap<Arc<str>, OrderBook>,
}

impl ProductionIngestor {
    pub fn new(config: IngestorConfig) -> Self {
        let mut orderbooks = RapidHashMap::default();
        for symbol in &config.symbols {
            orderbooks.insert(Arc::clone(symbol), OrderBook::new(symbol.as_ref()));
        }

        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            messages_processed: Arc::new(AtomicU64::new(0)),
            _last_message_time: Arc::new(AtomicU64::new(0)),
            orderbooks,
        }
    }

    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.running.store(true, Ordering::Relaxed);

        // Setup signal handlers for graceful shutdown
        self.setup_signal_handlers();

        // Start metrics thread if enabled
        if self.config.enable_metrics {
            self.start_metrics_thread();
        }

        // Start main ingestion loop
        self.run_ingestion_loop()
    }

    pub fn stop(&self) {
        tracing::info!("Stopping production ingestor...");
        self.running.store(false, Ordering::Relaxed);
    }

    fn setup_signal_handlers(&self) {
        let running = Arc::clone(&self.running);

        ctrlc::set_handler(move || {
            tracing::info!("\nReceived Ctrl+C, shutting down gracefully...");
            running.store(false, Ordering::Relaxed);
        })
        .expect("Error setting Ctrl-C handler");
    }

    fn start_metrics_thread(&self) {
        let messages_processed = Arc::clone(&self.messages_processed);
        let running = Arc::clone(&self.running);
        let interval = self.config.metrics_interval;

        std::thread::spawn(move || {
            let mut last_count = 0;
            let mut last_time = Instant::now();

            while running.load(Ordering::Relaxed) {
                std::thread::sleep(interval);

                let current_count = messages_processed.load(Ordering::Relaxed);
                let current_time = Instant::now();

                let messages_per_second = (current_count - last_count) as f64 / current_time.duration_since(last_time).as_secs_f64();

                tracing::info!(
                    "[METRICS] Messages/sec: {:.2}, Total: {}, Last: {}s ago",
                    messages_per_second,
                    current_count,
                    current_time.duration_since(last_time).as_secs()
                );

                last_count = current_count;
                last_time = current_time;
            }

            tracing::info!("Metrics thread stopped");
        });
    }

    fn run_ingestion_loop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.config.symbols.len() == 1 { self.run_single_symbol() } else { self.run_multi_symbol() }
    }

    fn run_single_symbol(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let symbol = Arc::clone(&self.config.symbols[0]);
        let running = Arc::clone(&self.running);

        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            match self.connect_and_ingest_single(&symbol) {
                Ok(_) => break,
                Err(err) => {
                    tracing::error!("Connection error for {symbol}: {err}, retrying in {:?}", self.config.reconnect_delay);
                    std::thread::sleep(self.config.reconnect_delay);
                }
            }
        }

        Ok(())
    }

    fn connect_and_ingest_single(&mut self, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut ingestor = BinanceIngestor::new(symbol)?;
        ingestor.connect()?;

        let messages_processed = Arc::clone(&self.messages_processed);

        // Start processing thread
        let processing_handle = ingestor.start_processing_thread(move |data| {
            // Convert to string and process
            if let Ok(_json_str) = std::str::from_utf8(data) {
                messages_processed.fetch_add(1, Ordering::Relaxed);
            }
        });

        // Run ingestion
        ingestor.run()?;

        // Wait for processing thread to finish
        let _ = processing_handle.join();

        Ok(())
    }

    fn run_multi_symbol(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            match self.connect_and_ingest_multi() {
                Ok(_) => break,
                Err(err) => {
                    tracing::error!("Multi-symbol connection error: {err}, retrying in {:?}", self.config.reconnect_delay);
                    std::thread::sleep(self.config.reconnect_delay);
                }
            }
        }

        Ok(())
    }

    fn connect_and_ingest_multi(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut multi_ingestor = MultiSymbolIngestor::new();

        // Add all symbols
        for symbol in &self.config.symbols {
            multi_ingestor.add_symbol(symbol.as_ref())?;
        }

        let messages_processed = Arc::clone(&self.messages_processed);

        // Start all ingestion threads
        let handles = multi_ingestor.start_all(move |_symbol, data| {
            // Process message for this symbol
            if let Ok(_json_str) = std::str::from_utf8(data) {
                messages_processed.fetch_add(1, Ordering::Relaxed);
            }
        })?;

        // Wait for all threads
        for handle in handles {
            let _ = handle.join();
        }

        Ok(())
    }

    pub fn get_orderbook(&self, symbol: &str) -> Option<&OrderBook> {
        self.orderbooks.get(symbol)
    }

    pub fn messages_processed(&self) -> u64 {
        self.messages_processed.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = IngestorConfig::default();
        assert_eq!(config.symbols.len(), 1);
        assert!(config.enable_metrics);
    }
}
