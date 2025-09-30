use tokio::sync::oneshot;

/// Commands sent from Telegram bot to trading engine
#[derive(Debug)]
pub enum TradingCommand {
    Start,
    Stop,
    GetStatus { respond_to: oneshot::Sender<String> },
    GetSystemInfo { respond_to: oneshot::Sender<String> },
    SetMinBps { threshold: f64, respond_to: oneshot::Sender<Result<(), String>> },
}

/// Status updates sent from trading engine to bot
#[derive(Clone, Debug)]
pub enum StatusUpdate {
    Started,
    Stopped,
    MinBpsUpdated(f64),
    TradeExecuted { symbol: String, price: f64, quantity: f64 },
    Error(String),
}
