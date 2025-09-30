use teloxide::utils::command::BotCommands;

/// Telegram bot commands with type-safe parsing
#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Trading Bot Commands:")]
pub enum Command {
    #[command(description = "Start trading operations")]
    Start,

    #[command(description = "Stop all trading")]
    Stop,

    #[command(description = "Get current status")]
    Status,

    #[command(description = "Display system information")]
    System,

    #[command(description = "Set minimum BPS threshold (usage: /minbps <value>)")]
    #[command(parse_with = "split")]
    MinBps { threshold: f64 },

    #[command(description = "Show help message")]
    Help,
}
