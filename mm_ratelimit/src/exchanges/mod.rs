//! Pre-configured rate limiters for major cryptocurrency exchanges
//!
//! This module provides factory functions that create rate limiters configured
//! to match the official rate limits of various cryptocurrency exchanges.
//!
//! # Supported Exchanges
//!
//! - **Binance**: Spot, Futures, and custom weight-based limits
//! - **Coinbase**: Public and authenticated API limits
//! - **Kraken**: Tiered rate limits based on verification level
//! - **Bybit**: Public and private API limits

pub mod binance;
pub mod bybit;
pub mod coinbase;
pub mod kraken;
