use std::collections::VecDeque;

use mm_strategy::FixedPoint;
use mm_strategy::OrderSide;
use mm_strategy::Position;
use serde::Deserialize;
use serde::Serialize;

/// Performance metrics for backtest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestMetrics {
    pub start_time: u64,
    pub end_time: u64,
    pub duration_seconds: f64,

    // Trading activity
    pub total_trades: u64,
    pub buy_trades: u64,
    pub sell_trades: u64,
    pub total_volume: f64,

    // PnL metrics
    pub initial_capital: f64,
    pub final_capital: f64,
    pub total_pnl: f64,
    pub total_pnl_pct: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,

    // Performance metrics
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub win_rate: f64,
    pub profit_factor: f64,

    // Position metrics
    pub max_long_position: f64,
    pub max_short_position: f64,
    pub avg_position_size: f64,
    pub time_in_market_pct: f64,

    // Quote metrics
    pub total_quotes: u64,
    pub avg_spread_bps: f64,
    pub avg_quote_age_ms: f64,
}

impl Default for BacktestMetrics {
    fn default() -> Self {
        Self {
            start_time: 0,
            end_time: 0,
            duration_seconds: 0.0,
            total_trades: 0,
            buy_trades: 0,
            sell_trades: 0,
            total_volume: 0.0,
            initial_capital: 0.0,
            final_capital: 0.0,
            total_pnl: 0.0,
            total_pnl_pct: 0.0,
            realized_pnl: 0.0,
            unrealized_pnl: 0.0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            max_drawdown_pct: 0.0,
            win_rate: 0.0,
            profit_factor: 0.0,
            max_long_position: 0.0,
            max_short_position: 0.0,
            avg_position_size: 0.0,
            time_in_market_pct: 0.0,
            total_quotes: 0,
            avg_spread_bps: 0.0,
            avg_quote_age_ms: 0.0,
        }
    }
}

/// Performance tracker during backtest
pub struct PerformanceTracker {
    initial_capital: f64,
    equity_curve: VecDeque<(u64, f64)>, // (timestamp, equity)
    _pnl_samples: VecDeque<f64>,
    max_equity: f64,

    // Trade tracking
    trades: Vec<TradeRecord>,

    // Position tracking
    position_samples: VecDeque<(u64, f64)>,

    // Quote tracking
    quote_count: u64,
    spread_sum: f64,
}

#[derive(Debug, Clone)]
struct TradeRecord {
    _timestamp: u64,
    side: OrderSide,
    _price: f64,
    quantity: f64,
    pnl: f64,
}

impl PerformanceTracker {
    pub fn new(initial_capital: f64) -> Self {
        Self {
            initial_capital,
            equity_curve: VecDeque::new(),
            _pnl_samples: VecDeque::new(),
            max_equity: initial_capital,
            trades: Vec::new(),
            position_samples: VecDeque::new(),
            quote_count: 0,
            spread_sum: 0.0,
        }
    }

    /// Record a fill
    pub fn record_fill(&mut self, timestamp: u64, side: OrderSide, price: f64, quantity: f64, pnl_change: f64) {
        self.trades.push(TradeRecord { _timestamp: timestamp, side, _price: price, quantity, pnl: pnl_change });
    }

    /// Update equity curve
    pub fn update_equity(&mut self, timestamp: u64, equity: f64) {
        self.equity_curve.push_back((timestamp, equity));
        if equity > self.max_equity {
            self.max_equity = equity;
        }

        // Keep last 10000 samples
        if self.equity_curve.len() > 10000 {
            self.equity_curve.pop_front();
        }
    }

    /// Update position
    pub fn update_position(&mut self, timestamp: u64, position: f64) {
        self.position_samples.push_back((timestamp, position));

        // Keep last 10000 samples
        if self.position_samples.len() > 10000 {
            self.position_samples.pop_front();
        }
    }

    /// Record quote
    pub fn record_quote(&mut self, spread_bps: f64) {
        self.quote_count += 1;
        self.spread_sum += spread_bps;
    }

    /// Calculate final metrics
    pub fn calculate_metrics(&self, position: &Position, mark_price: FixedPoint) -> BacktestMetrics {
        let start_time = self.equity_curve.front().map(|(t, _)| *t).unwrap_or(0);
        let end_time = self.equity_curve.back().map(|(t, _)| *t).unwrap_or(0);
        let duration_seconds = (end_time - start_time) as f64 / 1_000_000_000.0;

        // Calculate trade metrics
        let total_trades = self.trades.len() as u64;
        let buy_trades = self.trades.iter().filter(|t| matches!(t.side, OrderSide::Bid)).count() as u64;
        let sell_trades = self.trades.iter().filter(|t| matches!(t.side, OrderSide::Ask)).count() as u64;
        let total_volume: f64 = self.trades.iter().map(|t| t.quantity).sum();

        // Calculate PnL metrics
        let unrealized_pnl = position.unrealized_pnl(mark_price).to_f64();
        let realized_pnl = position.realized_pnl.to_f64();
        let total_pnl = realized_pnl + unrealized_pnl;
        let final_capital = self.initial_capital + total_pnl;
        let total_pnl_pct = (total_pnl / self.initial_capital) * 100.0;

        // Calculate drawdown
        let (max_drawdown, max_drawdown_pct) = self.calculate_max_drawdown();

        // Calculate Sharpe ratio
        let sharpe_ratio = self.calculate_sharpe_ratio();

        // Calculate win rate
        let winning_trades = self.trades.iter().filter(|t| t.pnl > 0.0).count();
        let win_rate = if total_trades > 0 { (winning_trades as f64 / total_trades as f64) * 100.0 } else { 0.0 };

        // Calculate profit factor
        let gross_profit: f64 = self.trades.iter().filter(|t| t.pnl > 0.0).map(|t| t.pnl).sum();
        let gross_loss: f64 = self.trades.iter().filter(|t| t.pnl < 0.0).map(|t| -t.pnl).sum();
        let profit_factor = if gross_loss > 0.0 { gross_profit / gross_loss } else { 0.0 };

        // Calculate position metrics
        let max_long_position = self.position_samples.iter().map(|(_, p)| *p).fold(0.0, f64::max);
        let max_short_position = self.position_samples.iter().map(|(_, p)| *p).fold(0.0, f64::min);

        let avg_position_size = if !self.position_samples.is_empty() {
            self.position_samples.iter().map(|(_, p)| p.abs()).sum::<f64>() / self.position_samples.len() as f64
        } else {
            0.0
        };

        let time_in_market = self.position_samples.iter().filter(|(_, p)| p.abs() > 0.01).count();
        let time_in_market_pct =
            if !self.position_samples.is_empty() { (time_in_market as f64 / self.position_samples.len() as f64) * 100.0 } else { 0.0 };

        // Calculate quote metrics
        let avg_spread_bps = if self.quote_count > 0 { self.spread_sum / self.quote_count as f64 } else { 0.0 };

        BacktestMetrics {
            start_time,
            end_time,
            duration_seconds,
            total_trades,
            buy_trades,
            sell_trades,
            total_volume,
            initial_capital: self.initial_capital,
            final_capital,
            total_pnl,
            total_pnl_pct,
            realized_pnl,
            unrealized_pnl,
            sharpe_ratio,
            max_drawdown,
            max_drawdown_pct,
            win_rate,
            profit_factor,
            max_long_position,
            max_short_position,
            avg_position_size,
            time_in_market_pct,
            total_quotes: self.quote_count,
            avg_spread_bps,
            avg_quote_age_ms: 0.0, // Can be calculated if needed
        }
    }

    fn calculate_max_drawdown(&self) -> (f64, f64) {
        let mut max_dd = 0.0;
        let mut max_dd_pct = 0.0;
        let mut peak = self.initial_capital;

        for (_, equity) in &self.equity_curve {
            if *equity > peak {
                peak = *equity;
            }

            let dd = peak - equity;
            let dd_pct = if peak > 0.0 { (dd / peak) * 100.0 } else { 0.0 };

            if dd > max_dd {
                max_dd = dd;
                max_dd_pct = dd_pct;
            }
        }

        (max_dd, max_dd_pct)
    }

    fn calculate_sharpe_ratio(&self) -> f64 {
        if self.equity_curve.len() < 2 {
            return 0.0;
        }

        // Calculate returns
        let mut returns = Vec::new();
        let equity_vec: Vec<_> = self.equity_curve.iter().collect();
        for window in equity_vec.windows(2) {
            let ret = (window[1].1 - window[0].1) / window[0].1;
            returns.push(ret);
        }

        if returns.is_empty() {
            return 0.0;
        }

        // Calculate mean and std dev
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return 0.0;
        }

        // Annualize (assuming samples are ~1 second apart, 252 trading days)
        let samples_per_year = 252.0 * 24.0 * 3600.0;
        let annualized_return = mean * samples_per_year;
        let annualized_std = std_dev * samples_per_year.sqrt();

        annualized_return / annualized_std
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_tracker() {
        let mut tracker = PerformanceTracker::new(10000.0);

        // Record some trades
        tracker.record_fill(1000, OrderSide::Bid, 50000.0, 0.1, 10.0);
        tracker.record_fill(2000, OrderSide::Ask, 50010.0, 0.1, 5.0);

        // Update equity
        tracker.update_equity(1000, 10010.0);
        tracker.update_equity(2000, 10015.0);

        let position = Position { quantity: FixedPoint::ZERO, avg_entry_price: FixedPoint::ZERO, realized_pnl: FixedPoint::from_f64(15.0) };

        let metrics = tracker.calculate_metrics(&position, FixedPoint::from_f64(50010.0));

        assert_eq!(metrics.total_trades, 2);
        assert_eq!(metrics.buy_trades, 1);
        assert_eq!(metrics.sell_trades, 1);
        assert_eq!(metrics.realized_pnl, 15.0);
    }
}
