#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

use crate::MarketDataMessage;

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
/// # Safety
///
/// This function requires NEON support and should only be called when the
/// "neon" target feature is available. The caller must ensure that the
/// messages array contains valid MarketDataMessage instances.
pub unsafe fn batch_validate_timestamps_neon(messages: &[MarketDataMessage; 2], threshold: u64) -> [bool; 2] {
    unsafe {
        let timestamps = vld1q_u64([messages[0].timestamp, messages[1].timestamp].as_ptr());
        let threshold_vec = vdupq_n_u64(threshold);
        let comparison = vcgtq_u64(timestamps, threshold_vec);

        [vgetq_lane_u64(comparison, 0) != 0, vgetq_lane_u64(comparison, 1) != 0]
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
/// # Safety
///
/// This function requires NEON support and should only be called when the
/// "neon" target feature is available. The caller must ensure that the
/// messages array contains valid MarketDataMessage instances.
pub unsafe fn batch_price_compare_neon(messages: &[MarketDataMessage; 2], threshold: i64) -> ([bool; 2], [bool; 2]) {
    unsafe {
        let bid_prices = vld1q_s64([messages[0].bid_price, messages[1].bid_price].as_ptr());
        let ask_prices = vld1q_s64([messages[0].ask_price, messages[1].ask_price].as_ptr());
        let threshold_vec = vdupq_n_s64(threshold);

        let bid_comparison = vcgtq_s64(bid_prices, threshold_vec);
        let ask_comparison = vcgtq_s64(ask_prices, threshold_vec);

        (
            [vgetq_lane_u64(bid_comparison, 0) != 0, vgetq_lane_u64(bid_comparison, 1) != 0],
            [vgetq_lane_u64(ask_comparison, 0) != 0, vgetq_lane_u64(ask_comparison, 1) != 0],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Exchange;
    use crate::compressed_string::CompressedString;
    use crate::messages::UpdateType;

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn test_neon_timestamp_validation() {
        if !std::arch::is_aarch64_feature_detected!("neon") {
            println!("NEON not available, skipping test");
            return;
        }

        let (symbol, encoding) = CompressedString::from_str("BTCUSDT").unwrap();

        let messages = [
            MarketDataMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 1000, 0, 0, 0, 0),
            MarketDataMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 3000, 0, 0, 0, 0),
        ];

        let results = unsafe { batch_validate_timestamps_neon(&messages, 2000) };
        assert_eq!(results, [false, true]);
    }
}
