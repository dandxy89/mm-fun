pub const FIXED_POINT_MULTIPLIER: i64 = 100_000_000;
pub const DECIMAL_PLACES: u32 = 8;

#[inline(always)]
pub fn to_fixed_point(value: f64) -> i64 {
    (value * FIXED_POINT_MULTIPLIER as f64).round() as i64
}

#[inline(always)]
pub fn from_fixed_point(value: i64) -> f64 {
    value as f64 / FIXED_POINT_MULTIPLIER as f64
}

/// Parse a JSON decimal string directly to fixed-point i64
///
/// Handles strings like "42250.15" or "0.00000123" without allocating
/// Supports up to 8 decimal places for satoshi precision
///
pub fn parse_json_decimal_to_fixed_point(bytes: &[u8]) -> Result<i64, crate::ProtocolError> {
    if bytes.is_empty() {
        return Err(crate::ProtocolError::InvalidLength { expected: 1, actual: 0 });
    }

    let mut result: i64 = 0;
    let mut decimal_pos: Option<usize> = None;
    let mut negative = false;
    let mut pos = 0;

    // Handle negative sign
    if bytes[0] == b'-' {
        negative = true;
        pos = 1;
    }

    // Parse integer and decimal parts
    while pos < bytes.len() {
        match bytes[pos] {
            b'0'..=b'9' => {
                let digit = (bytes[pos] - b'0') as i64;
                result = result * 10 + digit;
            }
            b'.' => {
                if decimal_pos.is_some() {
                    return Err(crate::ProtocolError::InvalidHeader { byte: b'.' });
                }
                decimal_pos = Some(pos);
            }
            _ => return Err(crate::ProtocolError::InvalidHeader { byte: bytes[pos] }),
        }
        pos += 1;
    }

    // Calculate how many decimal places we read
    let decimals_read = if let Some(dot_pos) = decimal_pos { bytes.len() - dot_pos - 1 } else { 0 };

    // Scale to fixed-point (pad or truncate to 8 decimal places)
    if decimals_read < DECIMAL_PLACES as usize {
        // Pad with zeros
        let scale_factor = 10i64.pow(DECIMAL_PLACES - decimals_read as u32);
        result *= scale_factor;
    } else if decimals_read > DECIMAL_PLACES as usize {
        // Truncate excess decimals
        let scale_factor = 10i64.pow((decimals_read - DECIMAL_PLACES as usize) as u32);
        result /= scale_factor;
    }

    if negative {
        result = -result;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_point_conversion() {
        assert_eq!(to_fixed_point(1.0), 100_000_000);
        assert_eq!(to_fixed_point(0.00000001), 1);
        assert_eq!(to_fixed_point(123.456789), 12_345_678_900);
        assert_eq!(to_fixed_point(-50.5), -5_050_000_000);
    }

    #[test]
    fn test_fixed_point_round_trip() {
        let values = vec![1.0, 0.00000001, 123.456789, -50.5, 0.0];
        for value in values {
            let fixed = to_fixed_point(value);
            let result = from_fixed_point(fixed);
            assert!((value - result).abs() < 1e-8);
        }
    }

    #[test]
    fn test_fixed_point_precision() {
        assert_eq!(from_fixed_point(1), 0.00000001);
        assert_eq!(from_fixed_point(100_000_000), 1.0);
        assert_eq!(from_fixed_point(-100_000_000), -1.0);
    }

    #[test]
    fn test_bitcoin_satoshi() {
        assert_eq!(to_fixed_point(0.00000001), 1);
    }

    #[test]
    fn test_parse_json_decimal_to_fixed_point() {
        // Basic cases
        assert_eq!(parse_json_decimal_to_fixed_point(b"1.0").unwrap(), 100_000_000);
        assert_eq!(parse_json_decimal_to_fixed_point(b"0.00000001").unwrap(), 1);
        assert_eq!(parse_json_decimal_to_fixed_point(b"42250.15").unwrap(), 4225015000000);

        // No decimal point
        assert_eq!(parse_json_decimal_to_fixed_point(b"100").unwrap(), 10000000000);

        // Negative numbers
        assert_eq!(parse_json_decimal_to_fixed_point(b"-50.5").unwrap(), -5050000000);

        // Many decimal places (should truncate to 8)
        assert_eq!(parse_json_decimal_to_fixed_point(b"1.123456789").unwrap(), 112345678);

        // Bitcoin price example
        assert_eq!(parse_json_decimal_to_fixed_point(b"42250.15678901").unwrap(), 4225015678901);
    }

    #[test]
    fn test_parse_json_decimal_edge_cases() {
        // Zero
        assert_eq!(parse_json_decimal_to_fixed_point(b"0").unwrap(), 0);
        assert_eq!(parse_json_decimal_to_fixed_point(b"0.0").unwrap(), 0);
        assert_eq!(parse_json_decimal_to_fixed_point(b"0.00000000").unwrap(), 0);

        // Very small number
        assert_eq!(parse_json_decimal_to_fixed_point(b"0.00000001").unwrap(), 1);

        // Errors
        assert!(parse_json_decimal_to_fixed_point(b"").is_err());
        assert!(parse_json_decimal_to_fixed_point(b"abc").is_err());
        assert!(parse_json_decimal_to_fixed_point(b"1.2.3").is_err());
    }
}
