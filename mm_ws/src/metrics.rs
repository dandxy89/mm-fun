use std::time::Instant;

/// Statistics for performance monitoring
#[derive(Debug, Clone)]
pub struct IngestorStats {
    pub messages_received: u64,
    pub bytes_received: u64,
    pub parse_errors: u64,
    pub last_message_time: u64,
}

impl Default for IngestorStats {
    fn default() -> Self {
        Self::new()
    }
}

impl IngestorStats {
    pub fn new() -> Self {
        Self { messages_received: 0, bytes_received: 0, parse_errors: 0, last_message_time: 0 }
    }
}

/// Performance metrics collector
pub struct PerformanceMetrics {
    latency_ns: Vec<u64>,
    pub message_count: u64,
    start_time: Instant,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self { latency_ns: Vec::with_capacity(100000), message_count: 0, start_time: Instant::now() }
    }

    pub fn record_latency(&mut self, latency_ns: u64) {
        self.latency_ns.push(latency_ns);
        self.message_count += 1;
    }

    pub fn throughput(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 { self.message_count as f64 / elapsed } else { 0.0 }
    }

    pub fn percentile(&self, p: f64) -> Option<u64> {
        if self.latency_ns.is_empty() {
            return None;
        }

        let mut sorted = self.latency_ns.clone();
        sorted.sort_unstable();
        let index = ((sorted.len() as f64 * p / 100.0) as usize).min(sorted.len() - 1);
        Some(sorted[index])
    }

    pub fn print_stats(&self) {
        tracing::info!("\n=== Performance Statistics ===");
        tracing::info!("Total messages: {}", self.message_count);
        tracing::info!("Throughput: {:.2} msg/sec", self.throughput());

        if !self.latency_ns.is_empty() {
            tracing::info!("\nLatency (nanoseconds):");
            if let Some(p50) = self.percentile(50.0) {
                tracing::info!("  P50:   {:>10} ns ({:>7.2} μs)", p50, p50 as f64 / 1000.0);
            }
            if let Some(p95) = self.percentile(95.0) {
                tracing::info!("  P95:   {:>10} ns ({:>7.2} μs)", p95, p95 as f64 / 1000.0);
            }
            if let Some(p99) = self.percentile(99.0) {
                tracing::info!("  P99:   {:>10} ns ({:>7.2} μs)", p99, p99 as f64 / 1000.0);
            }
            if let Some(p999) = self.percentile(99.9) {
                tracing::info!("  P99.9: {:>10} ns ({:>7.2} μs)", p999, p999 as f64 / 1000.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_metrics() {
        let mut metrics = PerformanceMetrics::new();
        metrics.record_latency(1000);
        metrics.record_latency(2000);
        metrics.record_latency(3000);

        assert_eq!(metrics.message_count, 3);
        assert_eq!(metrics.percentile(50.0), Some(2000));
    }
}
