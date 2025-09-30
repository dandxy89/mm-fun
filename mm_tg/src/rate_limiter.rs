use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use teloxide::types::ChatId;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;

/// Rate limiter guard that releases permits when dropped
pub struct RateLimitGuard {
    _global_permit: OwnedSemaphorePermit,
    _chat_permit: OwnedSemaphorePermit,
}

/// Rate limiter enforcing Telegram API limits
/// - 30 messages per second globally
/// - 1 message per second per chat
pub struct RateLimiter {
    global_limiter: Arc<Semaphore>,
    per_chat_limiters: Arc<DashMap<ChatId, Arc<Semaphore>>>,
}

impl RateLimiter {
    /// Create new rate limiter
    pub fn new() -> Self {
        Self {
            // Allow 30 concurrent messages (will be refilled every second)
            global_limiter: Arc::new(Semaphore::new(30)),
            per_chat_limiters: Arc::new(DashMap::new()),
        }
    }

    /// Acquire permits for both global and per-chat rate limits
    pub async fn acquire(&self, chat_id: ChatId) -> RateLimitGuard {
        // Acquire global permit
        let global_permit = self.global_limiter.clone().acquire_owned().await.expect("Global semaphore closed");

        // Get or create per-chat limiter
        let chat_limiter = self.per_chat_limiters.entry(chat_id).or_insert_with(|| Arc::new(Semaphore::new(1))).clone();

        // Acquire per-chat permit
        let chat_permit = chat_limiter.acquire_owned().await.expect("Chat semaphore closed");

        // Return guard that holds both permits
        RateLimitGuard { _global_permit: global_permit, _chat_permit: chat_permit }
    }

    /// Start background task to refill permits
    pub fn start_refill_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                // Refill global permits (30 per second)
                let available = self.global_limiter.available_permits();
                if available < 30 {
                    self.global_limiter.add_permits(30 - available);
                }

                // Refill per-chat permits (1 per second each)
                for entry in self.per_chat_limiters.iter() {
                    let semaphore = entry.value();
                    if semaphore.available_permits() == 0 {
                        semaphore.add_permits(1);
                    }
                }
            }
        });
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
