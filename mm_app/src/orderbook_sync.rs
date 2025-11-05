use std::time::Instant;

use mm_binary::OrderBookBatchMessage;
use tracing::debug;
use tracing::info;
use tracing::warn;

/// Configuration: Number of consecutive skipped updates before triggering resync
const RESYNC_SKIP_THRESHOLD: usize = 20;

/// Configuration: Minimum seconds between resync attempts
const RESYNC_COOLDOWN_SECS: u64 = 30;

/// Manages orderbook synchronization state using Binance sequence IDs
pub struct OrderbookSyncState {
    snapshot_last_update_id: u64,
    last_processed_update_id: u64,
    is_synchronized: bool,
    first_update_seen: Option<u64>,
    updates_since_snapshot: u64,
    consecutive_skipped_updates: usize,
    last_resync_attempt: Option<Instant>,
}

impl OrderbookSyncState {
    pub fn new(snapshot_last_update_id: u64) -> Self {
        info!("Initializing orderbook sync with snapshot lastUpdateId={snapshot_last_update_id}");
        Self {
            snapshot_last_update_id,
            last_processed_update_id: 0,
            is_synchronized: false,
            first_update_seen: None,
            updates_since_snapshot: 0,
            consecutive_skipped_updates: 0,
            last_resync_attempt: None,
        }
    }

    /// Check if this update should be processed
    /// Returns true if update should be applied, false if it should be skipped
    pub fn should_process_update(&mut self, batch: &OrderBookBatchMessage) -> bool {
        let first_id = batch.first_update_id();
        let final_id = batch.final_update_id();
        let prev_id = batch.prev_update_id();

        if !self.is_synchronized {
            // Track the first update ID we've seen
            if self.first_update_seen.is_none() {
                self.first_update_seen = Some(first_id);
            }
            self.updates_since_snapshot += 1;

            // Drop updates older than snapshot
            if final_id < self.snapshot_last_update_id {
                debug!("Dropping old update: u={} < snapshot_lastUpdateId={}", final_id, self.snapshot_last_update_id);
                self.consecutive_skipped_updates += 1;
                return false;
            }

            // Check if updates have moved too far past snapshot (need resync)
            if first_id > self.snapshot_last_update_id + 5000 {
                warn!(
                    "Snapshot is too old! First update U={} is {} updates past snapshot lastUpdateId={}. Need to re-fetch snapshot.",
                    first_id,
                    first_id - self.snapshot_last_update_id,
                    self.snapshot_last_update_id
                );
                // Keep trying for a while before giving up
            }

            // Find sync point: U <= lastUpdateId <= u
            if first_id <= self.snapshot_last_update_id && final_id >= self.snapshot_last_update_id {
                self.is_synchronized = true;
                self.last_processed_update_id = final_id;
                self.consecutive_skipped_updates = 0; // Reset on successful sync
                info!(
                    "Orderbook synchronized! U={}, u={}, snapshot_lastUpdateId={} (waited {} updates)",
                    first_id, final_id, self.snapshot_last_update_id, self.updates_since_snapshot
                );
                return true;
            }

            debug!("Waiting for sync point: U={}, u={}, snapshot_lastUpdateId={}", first_id, final_id, self.snapshot_last_update_id);
            self.consecutive_skipped_updates += 1;
            return false;
        }

        // Validate continuity: pu should equal previous u
        if prev_id != 0 && prev_id != self.last_processed_update_id {
            warn!("Sequence gap detected! Expected pu={}, got pu={}. Marking as desynced.", self.last_processed_update_id, prev_id);
            self.is_synchronized = false;
            self.consecutive_skipped_updates += 1;
            return false;
        }

        self.last_processed_update_id = final_id;
        self.consecutive_skipped_updates = 0; // Reset on successful processing
        true
    }

    pub fn is_synchronized(&self) -> bool {
        self.is_synchronized
    }

    /// Check if we should give up on current snapshot and fetch a new one
    /// Returns true if we've waited too long without syncing
    pub fn should_resync(&mut self) -> bool {
        if self.is_synchronized {
            return false;
        }

        // Check if we've exceeded the consecutive skip threshold
        if self.consecutive_skipped_updates < RESYNC_SKIP_THRESHOLD {
            return false;
        }

        // Check cooldown - prevent resync spam
        if let Some(last_attempt) = self.last_resync_attempt {
            let elapsed = last_attempt.elapsed();
            if elapsed.as_secs() < RESYNC_COOLDOWN_SECS {
                debug!("Resync cooldown active ({} seconds remaining)", RESYNC_COOLDOWN_SECS - elapsed.as_secs());
                return false;
            }
        }

        // Mark resync attempt timestamp
        self.last_resync_attempt = Some(Instant::now());

        info!("Resync triggered: {} consecutive skipped updates (threshold: {})", self.consecutive_skipped_updates, RESYNC_SKIP_THRESHOLD);

        true
    }

    /// Reset the skip counter after successful resync
    pub fn reset_after_resync(&mut self) {
        self.consecutive_skipped_updates = 0;
        self.updates_since_snapshot = 0;
        self.first_update_seen = None;
    }
}
