#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

// Use jemalloc for better performance
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

pub mod checksum;
pub mod compressed_string;
pub mod errors;
pub mod fixed_point;
pub mod messages;
pub mod orderbook_message;
pub mod serde_helpers;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod simd_x86;

#[cfg(target_arch = "aarch64")]
pub mod simd_arm;

pub use compressed_string::CompressedString;
pub use errors::ProtocolError;
pub use fixed_point::DECIMAL_PLACES;
pub use fixed_point::FIXED_POINT_MULTIPLIER;
pub use fixed_point::from_fixed_point;
pub use fixed_point::parse_json_decimal_to_fixed_point;
pub use fixed_point::to_fixed_point;
pub use messages::CollectorState;
pub use messages::CollectorStateMessage;
pub use messages::HeartbeatMessage;
pub use messages::MarketDataMessage;
pub use messages::PricingOutputMessage;
pub use orderbook_message::OrderBookBatchMessage;
pub use orderbook_message::PriceLevel;

pub const PROTOCOL_VERSION: u16 = 0x0200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exchange {
    Binance = 0,
    Coinbase = 1,
    Kraken = 2,
    Bybit = 3,
    Okx = 4,
    Bitfinex = 5,
    KuCoin = 6,
    Huobi = 7,
    GateIo = 8,
    Bitget = 9,
}

impl Exchange {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Exchange::Binance),
            1 => Some(Exchange::Coinbase),
            2 => Some(Exchange::Kraken),
            3 => Some(Exchange::Bybit),
            4 => Some(Exchange::Okx),
            5 => Some(Exchange::Bitfinex),
            6 => Some(Exchange::KuCoin),
            7 => Some(Exchange::Huobi),
            8 => Some(Exchange::GateIo),
            9 => Some(Exchange::Bitget),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    None,
    Basic,
    Standard,
    Strict,
}

#[cfg(target_arch = "x86_64")]
pub struct Features {
    pub crc32c: bool,
    pub avx2: bool,
    pub avx512f: bool,
}

#[cfg(target_arch = "x86_64")]
pub fn detect_features() -> Features {
    Features {
        crc32c: is_x86_feature_detected!("sse4.2"),
        avx2: is_x86_feature_detected!("avx2"),
        avx512f: is_x86_feature_detected!("avx512f"),
    }
}

#[cfg(target_arch = "aarch64")]
pub struct Features {
    pub crc32c: bool,
    pub neon: bool,
}

#[cfg(target_arch = "aarch64")]
pub fn detect_features() -> Features {
    // Disable runtime feature detection on ARM due to cranelift codegen issues
    // See: https://github.com/rust-lang/rustc_codegen_cranelift/issues/171
    Features { crc32c: false, neon: false }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub struct Features {
    pub crc32c: bool,
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn detect_features() -> Features {
    Features { crc32c: false }
}
