use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use crate::bot_commands::Command;
use crate::handle::TradingHandle;

/// Handle incoming Telegram commands
pub async fn handle_command(bot: Bot, msg: Message, cmd: Command, trading_handle: Arc<TradingHandle>) -> ResponseResult<()> {
    let chat_id = msg.chat.id;

    match cmd {
        Command::Start => {
            if let Err(err) = trading_handle.send_start() {
                tracing::error!("Failed to send start command: {err}");
                bot.send_message(chat_id, "Failed to start trading").await?;
            } else {
                bot.send_message(chat_id, "Trading started").await?;
            }
        }
        Command::Stop => {
            if let Err(err) = trading_handle.send_stop() {
                tracing::error!("Failed to send stop command: {err}");
                bot.send_message(chat_id, "Failed to stop trading").await?;
            } else {
                bot.send_message(chat_id, "Trading stopped").await?;
            }
        }
        Command::Status => match trading_handle.get_status().await {
            Ok(status) => {
                bot.send_message(chat_id, format!("Status:\n{status}")).await?;
            }
            Err(err) => {
                tracing::error!("Failed to get status: {err}");
                bot.send_message(chat_id, "Failed to get status").await?;
            }
        },
        Command::System => match trading_handle.get_system_info().await {
            Ok(info) => {
                bot.send_message(chat_id, format!("🖥 System Info:\n{info}")).await?;
            }
            Err(err) => {
                tracing::error!("Failed to get system info: {err}");
                bot.send_message(chat_id, "Failed to get system info").await?;
            }
        },
        Command::MinBps { threshold } => {
            if threshold <= 0.0 {
                bot.send_message(chat_id, "Threshold must be positive").await?;
                return Ok(());
            }

            match trading_handle.set_min_bps(threshold).await {
                Ok(_) => {
                    bot.send_message(chat_id, format!("Min BPS threshold set to: {threshold:.2}")).await?;
                }
                Err(err) => {
                    tracing::error!("Failed to set min BPS: {err}");
                    bot.send_message(chat_id, format!("Failed to set min BPS: {err}")).await?;
                }
            }
        }
        Command::Help => {
            let help_text = Command::descriptions().to_string();
            bot.send_message(chat_id, help_text).await?;
        }
    }

    Ok(())
}
