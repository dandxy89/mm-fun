use std::path::Path;

use config::Config;
use config::ConfigError;
use config::File;
use mm_sim_executor::SimulatorConfig;
use mm_strategy::StrategyConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StrategyConfigFile {
    pub symbol: String,
    #[serde(flatten)]
    pub strategy: StrategyConfig,
    pub quote_publish_interval_ms: Option<u64>,
    pub max_daily_loss: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct SimulatorConfigFile {
    pub symbol: String,
    #[serde(flatten)]
    pub simulator: SimulatorConfig,
}

pub fn load_strategy_config<P: AsRef<Path>>(path: P) -> Result<StrategyConfigFile, ConfigError> {
    let config = Config::builder().add_source(File::from(path.as_ref())).build()?;

    config.try_deserialize()
}

pub fn load_simulator_config<P: AsRef<Path>>(path: P) -> Result<SimulatorConfigFile, ConfigError> {
    let config = Config::builder().add_source(File::from(path.as_ref())).build()?;

    config.try_deserialize()
}

/// Load strategy config with fallback to default
pub fn load_strategy_config_or_default(path: &str) -> StrategyConfigFile {
    match load_strategy_config(path) {
        Ok(config) => {
            tracing::info!("Loaded strategy config from {path}");
            config
        }
        Err(err) => {
            tracing::warn!("Failed to load strategy config from {}: {}. Using defaults.", path, err);
            StrategyConfigFile {
                symbol: "BTCUSDT".to_string(),
                strategy: StrategyConfig::default(),
                quote_publish_interval_ms: Some(100),
                max_daily_loss: Some(1000.0),
            }
        }
    }
}

/// Load simulator config with fallback to default
pub fn load_simulator_config_or_default(path: &str) -> SimulatorConfigFile {
    match load_simulator_config(path) {
        Ok(config) => {
            tracing::info!("Loaded simulator config from {path}");
            config
        }
        Err(err) => {
            tracing::warn!("Failed to load simulator config from {}: {}. Using defaults.", path, err);
            SimulatorConfigFile { symbol: "BTCUSDT".to_string(), simulator: SimulatorConfig::default() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_configs() {
        let strategy = StrategyConfigFile {
            symbol: "BTCUSDT".to_string(),
            strategy: StrategyConfig::default(),
            quote_publish_interval_ms: Some(100),
            max_daily_loss: Some(1000.0),
        };

        assert_eq!(strategy.symbol, "BTCUSDT");
        assert_eq!(strategy.strategy.min_spread_bps, 5.0);

        let simulator = SimulatorConfigFile { symbol: "BTCUSDT".to_string(), simulator: SimulatorConfig::default() };

        assert_eq!(simulator.symbol, "BTCUSDT");
        assert_eq!(simulator.simulator.order_placement_latency_us, 10_000);
    }
}
