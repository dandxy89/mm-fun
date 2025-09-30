pub struct AffinityManager {
    websocket_core: usize,
    parser_cores: Vec<usize>,
    writer_core: usize,
}

impl Default for AffinityManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AffinityManager {
    pub fn new() -> Self {
        let total_cores = num_cpus::get();
        Self { websocket_core: 0, parser_cores: (1..total_cores.saturating_sub(1)).collect(), writer_core: total_cores.saturating_sub(1) }
    }

    pub fn pin_websocket_thread(&self) {
        if let Some(core_id) = core_affinity::get_core_ids().and_then(|ids| ids.get(self.websocket_core).cloned()) {
            core_affinity::set_for_current(core_id);
        }
    }

    pub fn pin_parser_thread(&self, parser_id: usize) {
        if let Some(&core_idx) = self.parser_cores.get(parser_id)
            && let Some(core_id) = core_affinity::get_core_ids().and_then(|ids| ids.get(core_idx).cloned())
        {
            core_affinity::set_for_current(core_id);
        }
    }

    pub fn pin_writer_thread(&self) {
        if let Some(core_id) = core_affinity::get_core_ids().and_then(|ids| ids.get(self.writer_core).cloned()) {
            core_affinity::set_for_current(core_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_affinity_manager() {
        let manager = AffinityManager::new();
        assert_eq!(manager.websocket_core, 0);
    }
}
