use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::BytesMut;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use rapidhash::RapidHashMap;
use tungstenite::Message;
use tungstenite::WebSocket;
use tungstenite::stream::MaybeTlsStream;

use crate::BufferPool;

const BUFFER_POOL_SIZE: usize = 1000;
const BUFFER_SIZE: usize = 64 * 1024;

/// Ultra-low latency WebSocket ingestor using MIO
pub struct BinanceIngestor {
    _symbol: Arc<str>,
    url: Box<str>,
    websocket: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
    message_sender: Sender<BytesMut>,
    message_receiver: Receiver<BytesMut>,
    pub running: Arc<AtomicBool>,
    messages_processed: Arc<AtomicU64>,
    buffer_pool: Arc<BufferPool>,
}

impl BinanceIngestor {
    pub fn new(symbol: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let url = format!("wss://stream.binance.com:9443/ws/{}@depth@100ms", symbol.to_lowercase()).into_boxed_str();
        let (tx, rx) = bounded(10_000);

        Ok(Self {
            _symbol: Arc::from(symbol),
            url,
            websocket: None,
            message_sender: tx,
            message_receiver: rx,
            running: Arc::new(AtomicBool::new(false)),
            messages_processed: Arc::new(AtomicU64::new(0)),
            buffer_pool: Arc::new(BufferPool::new(BUFFER_POOL_SIZE, BUFFER_SIZE)),
        })
    }

    /// Create a new trade stream ingestor
    pub fn new_trade_stream(symbol: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let url = format!("wss://stream.binance.com:9443/ws/{}@trade", symbol.to_lowercase()).into_boxed_str();
        let (tx, rx) = bounded(10_000);

        Ok(Self {
            _symbol: Arc::from(symbol),
            url,
            websocket: None,
            message_sender: tx,
            message_receiver: rx,
            running: Arc::new(AtomicBool::new(false)),
            messages_processed: Arc::new(AtomicU64::new(0)),
            buffer_pool: Arc::new(BufferPool::new(BUFFER_POOL_SIZE, BUFFER_SIZE)),
        })
    }

    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Connecting to Binance WebSocket: {}", self.url);
        let (ws, response) = tungstenite::connect(self.url.as_ref())?;
        tracing::info!("Connected successfully. Response status: {}", response.status());
        self.websocket = Some(ws);
        Ok(())
    }

    pub fn start_processing_thread<F>(&self, mut callback: F) -> std::thread::JoinHandle<()>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        let receiver = self.message_receiver.clone();
        let running = Arc::clone(&self.running);
        let messages_processed = Arc::clone(&self.messages_processed);
        let buffer_pool = Arc::clone(&self.buffer_pool);

        std::thread::spawn(move || {
            // Pin to CPU core 1 for consistent performance
            if let Some(core_id) = core_affinity::get_core_ids().and_then(|ids| ids.get(1).cloned()) {
                core_affinity::set_for_current(core_id);
            }

            while running.load(Ordering::Relaxed) {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(buffer) => {
                        callback(&buffer);
                        messages_processed.fetch_add(1, Ordering::Relaxed);
                        // Return buffer to pool for reuse
                        buffer_pool.return_buffer(buffer);
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.running.store(true, Ordering::Relaxed);

        // Pin WebSocket thread to CPU core 0
        if let Some(core_id) = core_affinity::get_core_ids().and_then(|ids| ids.first().cloned()) {
            core_affinity::set_for_current(core_id);
        }

        while self.running.load(Ordering::Relaxed) {
            if let Some(ref mut ws) = self.websocket {
                match ws.read() {
                    Ok(msg) => {
                        if let Message::Text(text) = msg {
                            // Get buffer from pool and copy message data
                            let mut buffer = self.buffer_pool.get();
                            buffer.extend_from_slice(text.as_bytes());
                            if self.message_sender.try_send(buffer).is_err() {
                                tracing::error!("Warning: Processing queue full, dropping message");
                            }
                        } else if let Message::Binary(data) = msg {
                            // Get buffer from pool and copy message data
                            let mut buffer = self.buffer_pool.get();
                            buffer.extend_from_slice(&data);
                            if self.message_sender.try_send(buffer).is_err() {
                                tracing::error!("Warning: Processing queue full, dropping message");
                            }
                        } else if let Message::Close(_) = msg {
                            tracing::error!("WebSocket closed by server");
                            break;
                        }
                    }
                    Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_micros(100));
                        continue;
                    }
                    Err(err) => {
                        tracing::error!("WebSocket error: {err}");
                        return Err(err.into());
                    }
                }
            } else {
                return Err("WebSocket not connected".into());
            }
        }

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn messages_processed(&self) -> u64 {
        self.messages_processed.load(Ordering::Relaxed)
    }
}

/// Multi-symbol ingestor for handling multiple WebSocket connections
pub struct MultiSymbolIngestor {
    ingestors: RapidHashMap<Arc<str>, BinanceIngestor>,
    running: Arc<AtomicBool>,
}

impl Default for MultiSymbolIngestor {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiSymbolIngestor {
    pub fn new() -> Self {
        Self { ingestors: RapidHashMap::default(), running: Arc::new(AtomicBool::new(false)) }
    }

    pub fn add_symbol(&mut self, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
        let symbol_arc: Arc<str> = Arc::from(symbol);
        let mut ingestor = BinanceIngestor::new(symbol)?;
        ingestor.connect()?;
        self.ingestors.insert(symbol_arc, ingestor);
        Ok(())
    }

    pub fn start_all<F>(&mut self, callback: F) -> Result<Vec<std::thread::JoinHandle<()>>, Box<dyn std::error::Error>>
    where
        F: Fn(&str, &[u8]) + Send + Sync + Clone + 'static,
    {
        self.running.store(true, Ordering::Relaxed);
        let mut handles = Vec::new();

        for (symbol, ingestor) in self.ingestors.iter() {
            let symbol_clone = Arc::clone(symbol);
            let symbol_clone2 = Arc::clone(symbol);
            let callback_clone = callback.clone();

            // Start processing thread for this symbol
            let handle = ingestor.start_processing_thread(move |data| {
                callback_clone(&symbol_clone, data);
            });
            handles.push(handle);

            // Start ingestion thread
            let running = Arc::clone(&self.running);
            let mut ingestor_clone = BinanceIngestor::new(symbol.as_ref())?;
            ingestor_clone.connect()?;
            ingestor_clone.running = running;

            let handle = std::thread::spawn(move || {
                if let Err(err) = ingestor_clone.run() {
                    tracing::error!("Error running ingestor for {symbol_clone2}: {err}");
                }
            });
            handles.push(handle);
        }

        Ok(handles)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        for (_, ingestor) in self.ingestors.iter_mut() {
            ingestor.stop();
        }
    }
}
