#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

use crate::CompressedString;
use crate::MarketDataMessage;

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn batch_validate_timestamps_avx2(messages: &[MarketDataMessage; 4], threshold: u64) -> [bool; 4] {
    let timestamps = _mm256_set_epi64x(
        messages[3].timestamp as i64,
        messages[2].timestamp as i64,
        messages[1].timestamp as i64,
        messages[0].timestamp as i64,
    );

    let threshold_vec = _mm256_set1_epi64x(threshold as i64);
    let comparison = _mm256_cmpgt_epi64(timestamps, threshold_vec);
    let mask = _mm256_movemask_epi8(comparison);

    [(mask & 0xFF) != 0, ((mask >> 8) & 0xFF) != 0, ((mask >> 16) & 0xFF) != 0, ((mask >> 24) & 0xFF) != 0]
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx512f"))]
#[target_feature(enable = "avx512f")]
pub unsafe fn batch_symbol_compare_avx512(messages: &[MarketDataMessage; 8], target_symbol: CompressedString) -> u8 {
    let symbols_low = _mm512_set_epi64(
        messages[7].symbol_low as i64,
        messages[6].symbol_low as i64,
        messages[5].symbol_low as i64,
        messages[4].symbol_low as i64,
        messages[3].symbol_low as i64,
        messages[2].symbol_low as i64,
        messages[1].symbol_low as i64,
        messages[0].symbol_low as i64,
    );
    let target_low = _mm512_set1_epi64(target_symbol.low as i64);

    let symbols_high = _mm512_set_epi64(
        messages[7].symbol_high as i64,
        messages[6].symbol_high as i64,
        messages[5].symbol_high as i64,
        messages[4].symbol_high as i64,
        messages[3].symbol_high as i64,
        messages[2].symbol_high as i64,
        messages[1].symbol_high as i64,
        messages[0].symbol_high as i64,
    );
    let target_high = _mm512_set1_epi64(target_symbol.high as i64);

    let cmp_low = _mm512_cmpeq_epi64_mask(symbols_low, target_low);
    let cmp_high = _mm512_cmpeq_epi64_mask(symbols_high, target_high);

    cmp_low & cmp_high
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn batch_price_compare_avx2(messages: &[MarketDataMessage; 4], threshold: i64) -> ([bool; 4], [bool; 4]) {
    let bid_prices = _mm256_set_epi64x(messages[3].bid_price, messages[2].bid_price, messages[1].bid_price, messages[0].bid_price);

    let ask_prices = _mm256_set_epi64x(messages[3].ask_price, messages[2].ask_price, messages[1].ask_price, messages[0].ask_price);

    let threshold_vec = _mm256_set1_epi64x(threshold);

    let bid_comparison = _mm256_cmpgt_epi64(bid_prices, threshold_vec);
    let ask_comparison = _mm256_cmpgt_epi64(ask_prices, threshold_vec);

    let bid_mask = _mm256_movemask_epi8(bid_comparison);
    let ask_mask = _mm256_movemask_epi8(ask_comparison);

    (
        [(bid_mask & 0xFF) != 0, ((bid_mask >> 8) & 0xFF) != 0, ((bid_mask >> 16) & 0xFF) != 0, ((bid_mask >> 24) & 0xFF) != 0],
        [(ask_mask & 0xFF) != 0, ((ask_mask >> 8) & 0xFF) != 0, ((ask_mask >> 16) & 0xFF) != 0, ((ask_mask >> 24) & 0xFF) != 0],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Exchange;
    use crate::compressed_string::EncodingScheme;
    use crate::messages::UpdateType;

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_avx2_timestamp_validation() {
        if !is_x86_feature_detected!("avx2") {
            println!("AVX2 not available, skipping test");
            return;
        }

        let (symbol, encoding) = CompressedString::from_str("BTCUSDT").unwrap();

        let messages = [
            MarketDataMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 1000, 0, 0, 0, 0),
            MarketDataMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 2000, 0, 0, 0, 0),
            MarketDataMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 3000, 0, 0, 0, 0),
            MarketDataMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 4000, 0, 0, 0, 0),
        ];

        let results = unsafe { batch_validate_timestamps_avx2(&messages, 2500) };
        assert_eq!(results, [false, false, true, true]);
    }
}
