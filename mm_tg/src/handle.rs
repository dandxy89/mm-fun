use anyhow::Result;
use crossbeam_channel::Sender;
use tokio::sync::oneshot;

use crate::commands::TradingCommand;

/// Handle for sending commands to the trading engine
#[derive(Clone)]
pub struct TradingHandle {
    command_tx: Sender<TradingCommand>,
}

impl TradingHandle {
    pub fn new(command_tx: Sender<TradingCommand>) -> Self {
        Self { command_tx }
    }

    /// Send start command to trading engine
    pub fn send_start(&self) -> Result<()> {
        self.command_tx.send(TradingCommand::Start)?;
        Ok(())
    }

    /// Send stop command to trading engine
    pub fn send_stop(&self) -> Result<()> {
        self.command_tx.send(TradingCommand::Stop)?;
        Ok(())
    }

    /// Get current trading status
    pub async fn get_status(&self) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(TradingCommand::GetStatus { respond_to: tx })?;
        Ok(rx.await?)
    }

    /// Get system information
    pub async fn get_system_info(&self) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(TradingCommand::GetSystemInfo { respond_to: tx })?;
        Ok(rx.await?)
    }

    /// Set minimum BPS threshold
    pub async fn set_min_bps(&self, threshold: f64) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(TradingCommand::SetMinBps { threshold, respond_to: tx })?;
        rx.await?.map_err(|err| anyhow::anyhow!(err))
    }
}
