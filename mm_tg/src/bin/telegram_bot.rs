use std::sync::Arc;

use crossbeam_channel::unbounded;
use mm_tg::AuthorizedUsers;
use mm_tg::Command;
use mm_tg::RateLimiter;
use mm_tg::StatusUpdate;
use mm_tg::TradingCommand;
use mm_tg::TradingHandle;
use mm_tg::handle_command;
use teloxide::prelude::*;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise logging
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialise bot
    let bot = Bot::from_env();
    tracing::info!("Telegram bot Initialised");

    // Load authorized users
    let authorized_users = AuthorizedUsers::from_env()?;
    if authorized_users.users.is_empty() {
        tracing::warn!("⚠️  No authorized users configured!");
        tracing::warn!("Set TELEGRAM_AUTHORIZED_USERS environment variable");
        tracing::warn!("Example: TELEGRAM_AUTHORIZED_USERS=123456,789012");
    } else {
        tracing::info!("Authorized {} users", authorized_users.users.len());
    }

    // Create channels
    // Bot → Trading: crossbeam (sync receiver in trading thread)
    let (cmd_tx, cmd_rx) = unbounded::<TradingCommand>();

    // Trading → Bot: tokio (async receiver in bot)
    let (status_tx, status_rx) = mpsc::unbounded_channel::<StatusUpdate>();

    // Spawn trading thread on pinned core
    let core_ids = core_affinity::get_core_ids().unwrap_or_default();
    std::thread::spawn(move || {
        // Pin to first core if available
        if let Some(&core_id) = core_ids.first() {
            core_affinity::set_for_current(core_id);
            tracing::info!("Trading thread pinned to core {:?}", core_id);
        }

        mm_tg::trading_thread::run_trading_loop(cmd_rx, status_tx);
    });

    // Create trading handle
    let trading_handle = Arc::new(TradingHandle::new(cmd_tx));

    // Initialise rate limiter
    let rate_limiter = Arc::new(RateLimiter::new());
    rate_limiter.clone().start_refill_task();

    // Spawn status update handler
    let bot_clone = bot.clone();
    let rate_limiter_clone = rate_limiter.clone();
    tokio::spawn(async move {
        handle_status_updates(bot_clone, status_rx, rate_limiter_clone).await;
    });

    // Build command handler
    let handler = Update::filter_message()
        .filter_map(|update: Update| update.from().map(|u| u.id))
        .filter(move |user_id: teloxide::types::UserId| {
            let authorized = authorized_users.is_authorized(&user_id);
            if !authorized {
                tracing::warn!("Unauthorized access attempt from user {}", user_id);
            }
            authorized
        })
        .filter_command::<Command>()
        .endpoint({
            let trading_handle = trading_handle.clone();
            move |bot: Bot, msg: Message, cmd: Command| {
                let trading_handle = trading_handle.clone();
                async move { handle_command(bot, msg, cmd, trading_handle).await }
            }
        });

    // Start dispatcher with reconnection
    tracing::info!("Starting Telegram bot dispatcher...");
    Dispatcher::builder(bot.clone(), handler.clone()).enable_ctrlc_handler().build().dispatch().await;

    Ok(())
}

/// Handle status updates from trading thread
async fn handle_status_updates(bot: Bot, mut status_rx: mpsc::UnboundedReceiver<StatusUpdate>, rate_limiter: Arc<RateLimiter>) {
    // Get admin chat ID from environment
    let admin_chat_id: Option<i64> = std::env::var("TELEGRAM_ADMIN_CHAT_ID").ok().and_then(|s| s.parse().ok());

    if admin_chat_id.is_none() {
        tracing::warn!("TELEGRAM_ADMIN_CHAT_ID not set, status updates will not be sent");
    }

    while let Some(update) = status_rx.recv().await {
        if let Some(chat_id) = admin_chat_id {
            let chat_id = teloxide::types::ChatId(chat_id);
            let _guard = rate_limiter.acquire(chat_id).await;

            let text = match update {
                StatusUpdate::Started => "Trading started".to_string(),
                StatusUpdate::Stopped => "Trading stopped".to_string(),
                StatusUpdate::MinBpsUpdated(threshold) => {
                    format!("Min BPS updated to: {threshold:.2}")
                }
                StatusUpdate::TradeExecuted { symbol, price, quantity } => {
                    format!("Trade executed: {symbol} @ {price:.2} x {quantity:.4}")
                }
                StatusUpdate::Error(err) => format!("Error: {err}"),
            };

            if let Err(err) = bot.send_message(chat_id, text).await {
                tracing::error!("Failed to send status update: {err}");
            }
        }
    }
}
