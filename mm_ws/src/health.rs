use std::time::Duration;
use std::time::Instant;

pub struct HealthChecker {
    last_message_time: Instant,
    timeout: Duration,
}

impl HealthChecker {
    pub fn new(timeout: Duration) -> Self {
        Self { last_message_time: Instant::now(), timeout }
    }

    pub fn update(&mut self) {
        self.last_message_time = Instant::now();
    }

    pub fn is_healthy(&self) -> bool {
        self.last_message_time.elapsed() < self.timeout
    }

    pub fn time_since_last_message(&self) -> Duration {
        self.last_message_time.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker() {
        let mut checker = HealthChecker::new(Duration::from_secs(10));
        assert!(checker.is_healthy());

        std::thread::sleep(Duration::from_millis(100));
        checker.update();
        assert!(checker.is_healthy());
    }
}
