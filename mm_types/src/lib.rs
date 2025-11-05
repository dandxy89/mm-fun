use mm_binary::FIXED_POINT_MULTIPLIER;
use mm_binary::from_fixed_point;
// Re-export OrderSide from mm_binary for consistency
pub use mm_binary::messages::OrderSide;
use mm_binary::to_fixed_point;

/// Fixed-point number wrapper for cleaner API
/// Internally uses i64 with 8 decimal places (satoshi precision)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FixedPoint(pub i64);

impl FixedPoint {
    pub const ZERO: Self = FixedPoint(0);

    #[inline(always)]
    pub fn from_f64(value: f64) -> Self {
        FixedPoint(to_fixed_point(value))
    }

    #[inline(always)]
    pub fn from_int(value: i64) -> Self {
        FixedPoint(value * FIXED_POINT_MULTIPLIER)
    }

    #[inline(always)]
    pub fn to_f64(self) -> f64 {
        from_fixed_point(self.0)
    }

    #[inline(always)]
    pub fn to_i64(self) -> i64 {
        self.0
    }

    /// Apply basis points to this value
    /// Example: 100.0 with 50 bps = 100.5
    #[inline(always)]
    pub fn apply_bps(self, bps: f64) -> Self {
        // bps / 10000.0 gives the decimal multiplier
        // self * (bps / 10000.0) gives the adjustment
        let adjustment = (self.0 as i128 * (bps * 10000.0) as i128) / 100_000_000;
        FixedPoint(self.0 + adjustment as i64)
    }

    /// Subtract basis points from this value
    #[inline(always)]
    pub fn subtract_bps(self, bps: f64) -> Self {
        let adjustment = (self.0 as i128 * (bps * 10000.0) as i128) / 100_000_000;
        FixedPoint(self.0 - adjustment as i64)
    }

    /// Multiply by a scalar (f64) while maintaining precision
    #[inline(always)]
    pub fn mul_scalar(self, scalar: f64) -> Self {
        let result = (self.0 as f64 * scalar) as i64;
        FixedPoint(result)
    }
}

impl std::ops::Add for FixedPoint {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        FixedPoint(self.0 + rhs.0)
    }
}

impl std::ops::Sub for FixedPoint {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        FixedPoint(self.0 - rhs.0)
    }
}

impl std::ops::Mul for FixedPoint {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        // Fixed-point multiplication: (a * b) / MULTIPLIER
        let result = (self.0 as i128 * rhs.0 as i128) / FIXED_POINT_MULTIPLIER as i128;
        FixedPoint(result as i64)
    }
}

impl std::ops::Div for FixedPoint {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        // Fixed-point division: (a * MULTIPLIER) / b
        let result = (self.0 as i128 * FIXED_POINT_MULTIPLIER as i128) / rhs.0 as i128;
        FixedPoint(result as i64)
    }
}

impl std::ops::Neg for FixedPoint {
    type Output = Self;

    fn neg(self) -> Self::Output {
        FixedPoint(-self.0)
    }
}

impl std::ops::AddAssign for FixedPoint {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::SubAssign for FixedPoint {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl std::fmt::Display for FixedPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_f64())
    }
}

/// Market state snapshot
#[derive(Debug, Clone, Copy)]
pub struct MarketState {
    pub timestamp: u64,
    pub bid_price: FixedPoint,
    pub ask_price: FixedPoint,
    pub bid_volume: FixedPoint,
    pub ask_volume: FixedPoint,
    pub last_trade_price: Option<FixedPoint>,
    pub last_trade_size: Option<FixedPoint>,
}

impl MarketState {
    /// Calculate mid-price
    pub fn mid_price(&self) -> FixedPoint {
        (self.bid_price + self.ask_price) / FixedPoint::from_int(2)
    }

    /// Calculate spread in basis points
    pub fn spread_bps(&self) -> f64 {
        let spread = self.ask_price - self.bid_price;
        let mid = self.mid_price();
        (spread.to_f64() / mid.to_f64()) * 10000.0
    }

    /// Calculate micro-price (volume-weighted price)
    pub fn micro_price(&self) -> FixedPoint {
        let total_volume = self.bid_volume + self.ask_volume;
        if total_volume == FixedPoint::ZERO {
            return self.mid_price();
        }

        // micro = (bid * ask_vol + ask * bid_vol) / (bid_vol + ask_vol)
        let numerator = self.bid_price * self.ask_volume + self.ask_price * self.bid_volume;
        numerator / total_volume
    }

    /// Calculate order book imbalance (-1 to 1)
    /// Positive = more ask volume (bullish), Negative = more bid volume (bearish)
    pub fn orderbook_imbalance(&self) -> f64 {
        let total_volume = self.bid_volume + self.ask_volume;
        if total_volume == FixedPoint::ZERO {
            return 0.0;
        }

        (self.ask_volume - self.bid_volume).to_f64() / total_volume.to_f64()
    }
}

/// Position tracking
#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub quantity: FixedPoint,
    pub avg_entry_price: FixedPoint,
    pub realized_pnl: FixedPoint,
}

impl Position {
    pub fn new() -> Self {
        Self { quantity: FixedPoint::ZERO, avg_entry_price: FixedPoint::ZERO, realized_pnl: FixedPoint::ZERO }
    }

    /// Calculate unrealized PnL given current market price
    pub fn unrealized_pnl(&self, mark_price: FixedPoint) -> FixedPoint {
        if self.quantity == FixedPoint::ZERO {
            return FixedPoint::ZERO;
        }
        (mark_price - self.avg_entry_price) * self.quantity
    }

    /// Update position with a fill
    pub fn apply_fill(&mut self, side: OrderSide, price: FixedPoint, quantity: FixedPoint) {
        let fill_qty = match side {
            OrderSide::Bid => quantity,  // Buying increases position
            OrderSide::Ask => -quantity, // Selling decreases position
        };

        let new_quantity = self.quantity + fill_qty;

        // Handle position changes
        if self.quantity == FixedPoint::ZERO {
            // Opening new position
            self.quantity = new_quantity;
            self.avg_entry_price = price;
        } else if (self.quantity.to_i64() > 0) == (new_quantity.to_i64() > 0) && new_quantity.to_i64().abs() > self.quantity.to_i64().abs()
        {
            // Adding to position (same sign, larger absolute value)
            let total_cost = self.avg_entry_price * self.quantity + price * fill_qty;
            self.avg_entry_price = total_cost / new_quantity;
            self.quantity = new_quantity;
        } else if (self.quantity.to_i64() > 0) != (new_quantity.to_i64() > 0) {
            // Flipping position (crossing zero)
            // Close old position
            let close_pnl = (price - self.avg_entry_price) * self.quantity;
            self.realized_pnl += close_pnl;

            // Open new position with remainder
            self.quantity = new_quantity;
            self.avg_entry_price = price;
        } else {
            // Reducing position (same sign, smaller absolute value)
            // fill_qty is negative for sells, so negate it to get the closed amount
            let closed_qty = -fill_qty;
            let close_pnl = (price - self.avg_entry_price) * closed_qty;
            self.realized_pnl += close_pnl;
            self.quantity = new_quantity;
        }
    }

    /// Total PnL (realized + unrealized)
    pub fn total_pnl(&self, mark_price: FixedPoint) -> FixedPoint {
        self.realized_pnl + self.unrealized_pnl(mark_price)
    }

    /// Check if position is flat
    pub fn is_flat(&self) -> bool {
        self.quantity == FixedPoint::ZERO
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}
