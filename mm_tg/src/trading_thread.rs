use std::time::Duration;

use crossbeam_channel::Receiver;
use tokio::sync::mpsc::UnboundedSender;

use crate::commands::StatusUpdate;
use crate::commands::TradingCommand;

/// Trading state management
#[derive(Debug)]
pub struct TradingState {
    running: bool,
    min_bps: f64,
    trades_executed: u64,
}

impl TradingState {
    pub fn new() -> Self {
        Self { running: false, min_bps: 0.0, trades_executed: 0 }
    }

    pub fn start(&mut self) {
        self.running = true;
        tracing::info!("Trading started");
    }

    pub fn stop(&mut self) {
        self.running = false;
        tracing::info!("Trading stopped");
    }

    pub fn set_min_bps(&mut self, threshold: f64) -> Result<(), String> {
        if threshold < 0.0 {
            return Err("Threshold must be non-negative".to_string());
        }
        self.min_bps = threshold;
        tracing::info!("Min BPS set to {:.2}", threshold);
        Ok(())
    }

    pub fn get_status(&self) -> String {
        format!("Running: {}\nMin BPS: {:.2}\nTrades Executed: {}", self.running, self.min_bps, self.trades_executed)
    }

    pub fn get_system_info(&self) -> String {
        let num_cpus = num_cpus::get();
        let num_physical = num_cpus::get_physical();
        format!("CPUs: {} ({} physical)\nMemory: Available\nUptime: Running", num_cpus, num_physical)
    }

    pub fn execute_trade(&mut self, symbol: &str, price: f64, quantity: f64) {
        self.trades_executed += 1;
        tracing::info!("Trade executed: {} @ {:.2} x {:.4}", symbol, price, quantity);
    }
}

impl Default for TradingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Run trading thread loop
///
/// This function runs on a pinned thread and processes commands from the Telegram bot.
/// It maintains trading state and sends status updates back to the bot.
pub fn run_trading_loop(cmd_rx: Receiver<TradingCommand>, status_tx: UnboundedSender<StatusUpdate>) {
    let mut state = TradingState::new();

    tracing::info!("Trading thread started");

    loop {
        // Try to receive command with timeout to allow periodic processing
        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(cmd) => match cmd {
                TradingCommand::Start => {
                    state.start();
                    let _ = status_tx.send(StatusUpdate::Started);
                }
                TradingCommand::Stop => {
                    state.stop();
                    let _ = status_tx.send(StatusUpdate::Stopped);
                }
                TradingCommand::GetStatus { respond_to } => {
                    let status = state.get_status();
                    let _ = respond_to.send(status);
                }
                TradingCommand::GetSystemInfo { respond_to } => {
                    let info = state.get_system_info();
                    let _ = respond_to.send(info);
                }
                TradingCommand::SetMinBps { threshold, respond_to } => {
                    let result = state.set_min_bps(threshold);
                    if result.is_ok() {
                        let _ = status_tx.send(StatusUpdate::MinBpsUpdated(threshold));
                    }
                    let _ = respond_to.send(result);
                }
            },
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // Check market conditions, execute strategies, etc.
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                tracing::info!("Trading thread shutting down");
                break;
            }
        }
    }
}
