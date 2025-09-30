use std::time::Duration;

use teloxide::ApiError;
use teloxide::RequestError;
use teloxide::prelude::*;
use teloxide::types::ChatId;

/// Send message with automatic retry on rate limiting and transient errors
pub async fn send_with_retry(bot: &Bot, chat_id: ChatId, text: String) -> Result<(), RequestError> {
    match bot.send_message(chat_id, text.clone()).await {
        Ok(_) => Ok(()),
        Err(RequestError::RetryAfter(seconds)) => {
            tracing::warn!("Rate limited, waiting {seconds:?}");
            // Sleep for the retry duration
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            bot.send_message(chat_id, text).await?;
            Ok(())
        }
        Err(RequestError::Network(ref err)) => {
            tracing::error!("Network error: {err}");
            // Retry once after 1 second
            tokio::time::sleep(Duration::from_secs(1)).await;
            match bot.send_message(chat_id, text).await {
                Ok(_) => Ok(()),
                Err(err) => Err(err),
            }
        }
        Err(RequestError::Api(ApiError::BotBlocked)) => {
            tracing::warn!("Bot was blocked by user {chat_id}");
            Ok(()) // Don't retry blocks
        }
        Err(RequestError::Api(ApiError::UserDeactivated)) => {
            tracing::warn!("User {chat_id} is deactivated");
            Ok(()) // Don't retry deactivated users
        }
        Err(err) => {
            tracing::error!("Unexpected error sending message: {err}");
            Err(err)
        }
    }
}

/// Exponential backoff for reconnection attempts
pub struct ExponentialBackoff {
    current: Duration,
    max: Duration,
}

impl ExponentialBackoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self { current: initial, max }
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = std::cmp::min(self.current * 2, self.max);
        delay
    }

    pub fn reset(&mut self) {
        self.current = Duration::from_secs(1);
    }
}
