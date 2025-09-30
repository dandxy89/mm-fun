use std::collections::HashSet;

use teloxide::types::UserId;

/// Authorization filter for allowed user IDs
#[derive(Clone)]
pub struct AuthorizedUsers {
    pub users: HashSet<UserId>,
}

impl AuthorizedUsers {
    /// Create new authorized users list from user IDs
    pub fn new(user_ids: Vec<u64>) -> Self {
        Self { users: user_ids.into_iter().map(UserId).collect() }
    }

    /// Check if user is authorized
    pub fn is_authorized(&self, user_id: &UserId) -> bool {
        self.users.contains(user_id)
    }

    /// Load authorized users from environment variable
    /// Format: comma-separated list of user IDs (e.g., "123456,789012")
    pub fn from_env() -> anyhow::Result<Self> {
        let user_ids_str = std::env::var("TELEGRAM_AUTHORIZED_USERS").unwrap_or_else(|_| String::new());

        if user_ids_str.is_empty() {
            tracing::warn!("No authorized users configured. Set TELEGRAM_AUTHORIZED_USERS environment variable.");
            return Ok(Self { users: HashSet::new() });
        }

        let user_ids: Result<Vec<u64>, _> = user_ids_str.split(',').map(|s| s.trim().parse::<u64>()).collect();

        match user_ids {
            Ok(ids) => Ok(Self::new(ids)),
            Err(err) => Err(anyhow::anyhow!("Failed to parse authorized user IDs: {err}")),
        }
    }
}
