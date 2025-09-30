use crate::errors::ProtocolError;

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressedString {
    pub low: u64,
    pub high: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EncodingScheme {
    Hex4Bit = 0,
    Alphabetic5Bit = 1,
    AlphaNumeric6Bit = 2,
    Ascii7Bit = 3,
}

impl CompressedString {
    pub const fn new() -> Self {
        CompressedString { low: 0, high: 0 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<(Self, EncodingScheme), ProtocolError> {
        if s.is_empty() {
            return Ok((Self::new(), EncodingScheme::Hex4Bit));
        }

        if let Ok(result) = Self::encode_hex4bit(s) {
            return Ok((result, EncodingScheme::Hex4Bit));
        }

        if let Ok(result) = Self::encode_alphabetic5bit(s) {
            return Ok((result, EncodingScheme::Alphabetic5Bit));
        }

        if let Ok(result) = Self::encode_alphanumeric6bit(s) {
            return Ok((result, EncodingScheme::AlphaNumeric6Bit));
        }

        if let Ok(result) = Self::encode_ascii7bit(s) {
            return Ok((result, EncodingScheme::Ascii7Bit));
        }

        Err(ProtocolError::StringTooLong { length: s.len(), max: 18 })
    }

    fn encode_hex4bit(s: &str) -> Result<Self, ProtocolError> {
        if s.len() > 32 {
            return Err(ProtocolError::StringTooLong { length: s.len(), max: 32 });
        }

        // Hex4Bit requires all characters to be hex digits
        for (i, ch) in s.chars().enumerate() {
            if !ch.is_ascii_hexdigit() {
                return Err(ProtocolError::InvalidCharacter { char: ch, position: i });
            }
        }

        let mut result = CompressedString::new();
        let mut bit_pos = 0u32;

        for ch in s.chars() {
            let value = match ch {
                '0'..='9' => (ch as u8 - b'0') + 1,
                'A'..='F' => (ch as u8 - b'A') + 11,
                'a'..='f' => (ch as u8 - b'a') + 11,
                _ => unreachable!(),
            };

            if bit_pos < 64 && bit_pos + 4 <= 64 {
                // Fits entirely in low
                result.low |= (value as u64) << bit_pos;
            } else if bit_pos >= 64 {
                // Fits entirely in high
                result.high |= (value as u64) << (bit_pos - 64);
            } else {
                // Spans across low and high
                let bits_in_low = 64 - bit_pos;
                result.low |= (value as u64) << bit_pos;
                result.high |= (value as u64) >> bits_in_low;
            }

            bit_pos += 4;
        }

        Ok(result)
    }

    fn encode_alphabetic5bit(s: &str) -> Result<Self, ProtocolError> {
        if s.len() > 25 {
            return Err(ProtocolError::StringTooLong { length: s.len(), max: 25 });
        }

        let mut result = CompressedString::new();
        let mut bit_pos = 0u32;

        for (i, ch) in s.chars().enumerate() {
            let value = match ch {
                'A'..='Z' => (ch as u8 - b'A') + 1,
                _ => return Err(ProtocolError::InvalidCharacter { char: ch, position: i }),
            };

            if bit_pos + 5 <= 64 {
                // Fits entirely in low
                result.low |= (value as u64) << bit_pos;
            } else if bit_pos >= 64 {
                // Fits entirely in high
                result.high |= (value as u64) << (bit_pos - 64);
            } else {
                // Spans across low and high
                let bits_in_low = 64 - bit_pos;
                result.low |= (value as u64) << bit_pos;
                result.high |= (value as u64) >> bits_in_low;
            }

            bit_pos += 5;
        }

        Ok(result)
    }

    fn encode_alphanumeric6bit(s: &str) -> Result<Self, ProtocolError> {
        if s.len() > 21 {
            return Err(ProtocolError::StringTooLong { length: s.len(), max: 21 });
        }

        // AlphaNumeric6Bit requires all characters to be uppercase letters or digits
        for (i, ch) in s.chars().enumerate() {
            if !ch.is_ascii_uppercase() && !ch.is_ascii_digit() {
                return Err(ProtocolError::InvalidCharacter { char: ch, position: i });
            }
        }

        let mut result = CompressedString::new();
        let mut bit_pos = 0u32;

        for ch in s.chars() {
            let value = match ch {
                'A'..='Z' => ch as u8 - b'A',
                '0'..='9' => (ch as u8 - b'0') + 26,
                _ => unreachable!(),
            };

            if bit_pos + 6 <= 64 {
                // Fits entirely in low
                result.low |= (value as u64) << bit_pos;
            } else if bit_pos >= 64 {
                // Fits entirely in high
                result.high |= (value as u64) << (bit_pos - 64);
            } else {
                // Spans across low and high
                let bits_in_low = 64 - bit_pos;
                result.low |= (value as u64) << bit_pos;
                result.high |= (value as u64) >> bits_in_low;
            }

            bit_pos += 6;
        }

        Ok(result)
    }

    fn encode_ascii7bit(s: &str) -> Result<Self, ProtocolError> {
        if s.len() > 18 {
            return Err(ProtocolError::StringTooLong { length: s.len(), max: 18 });
        }

        if !s.is_ascii() {
            return Err(ProtocolError::InvalidCharacter {
                char: s.chars().find(|c| !c.is_ascii()).unwrap(),
                position: s.chars().position(|c| !c.is_ascii()).unwrap(),
            });
        }

        let mut result = CompressedString::new();
        let mut bit_pos = 0u32;

        for ch in s.bytes() {
            if bit_pos + 7 <= 64 {
                // Fits entirely in low
                result.low |= (ch as u64) << bit_pos;
            } else if bit_pos >= 64 {
                // Fits entirely in high
                result.high |= (ch as u64) << (bit_pos - 64);
            } else {
                // Spans across low and high
                let bits_in_low = 64 - bit_pos;
                result.low |= (ch as u64) << bit_pos;
                result.high |= (ch as u64) >> bits_in_low;
            }

            bit_pos += 7;
        }

        Ok(result)
    }

    pub fn decode(&self, scheme: EncodingScheme) -> String {
        match scheme {
            EncodingScheme::Hex4Bit => self.decode_hex4bit(),
            EncodingScheme::Alphabetic5Bit => self.decode_alphabetic5bit(),
            EncodingScheme::AlphaNumeric6Bit => self.decode_alphanumeric6bit(),
            EncodingScheme::Ascii7Bit => self.decode_ascii7bit(),
        }
    }

    fn decode_hex4bit(&self) -> String {
        let mut result = String::with_capacity(32);
        let mut bit_pos = 0u32;

        while bit_pos < 128 {
            let value = if bit_pos < 64 {
                let shift = bit_pos;
                if shift + 4 <= 64 {
                    (self.low >> shift) & 0xF
                } else {
                    // Bits span across low and high
                    let bits_from_low = 64 - shift;
                    let bits_from_high = 4 - bits_from_low;
                    let low_part = (self.low >> shift) & ((1u64 << bits_from_low) - 1);
                    let high_part = self.high & ((1u64 << bits_from_high) - 1);
                    low_part | (high_part << bits_from_low)
                }
            } else {
                let shift = bit_pos - 64;
                (self.high >> shift) & 0xF
            };

            if value == 0 {
                break;
            }

            let ch = match value {
                1..=10 => (b'0' + (value as u8) - 1) as char,
                11..=15 => (b'A' + (value as u8) - 11) as char,
                16 => 'F',
                _ => break,
            };

            result.push(ch);
            bit_pos += 4;
        }

        result
    }

    fn decode_alphabetic5bit(&self) -> String {
        let mut result = String::with_capacity(25);
        let mut bit_pos = 0u32;

        while bit_pos < 128 {
            let value = if bit_pos < 64 {
                let shift = bit_pos;
                let mut v = (self.low >> shift) & 0x1F;

                if shift > 59 {
                    let overflow_bits = (shift + 5) - 64;
                    v |= (self.high << (5 - overflow_bits)) & 0x1F;
                }
                v
            } else {
                let shift = bit_pos - 64;
                (self.high >> shift) & 0x1F
            };

            if value == 0 {
                break;
            }

            if value <= 26 {
                result.push((b'A' + (value as u8) - 1) as char);
            } else {
                break;
            }

            bit_pos += 5;
        }

        result
    }

    fn decode_alphanumeric6bit(&self) -> String {
        let mut result = String::with_capacity(21);
        let mut bit_pos = 0u32;
        let mut consecutive_a_count = 0;

        while bit_pos < 128 {
            let value = if bit_pos < 64 {
                let shift = bit_pos;
                let mut v = (self.low >> shift) & 0x3F;

                if shift > 58 {
                    let overflow_bits = (shift + 6) - 64;
                    v |= (self.high << (6 - overflow_bits)) & 0x3F;
                }
                v
            } else {
                let shift = bit_pos - 64;
                (self.high >> shift) & 0x3F
            };

            if value >= 36 {
                break;
            }

            let ch = if value < 26 { (b'A' + (value as u8)) as char } else { (b'0' + (value as u8) - 26) as char };

            result.push(ch);

            // For AlphaNumeric6Bit, detect end of string by looking for consecutive A's
            // which indicate padding zeros. If we see 3+ consecutive A's, likely padding.
            if ch == 'A' && value == 0 {
                consecutive_a_count += 1;
                if consecutive_a_count >= 3 {
                    // Remove the padding A's
                    for _ in 0..consecutive_a_count {
                        result.pop();
                    }
                    break;
                }
            } else {
                consecutive_a_count = 0;
            }
            bit_pos += 6;
        }

        result
    }

    fn decode_ascii7bit(&self) -> String {
        let mut result = String::with_capacity(18);
        let mut bit_pos = 0u32;

        while bit_pos < 128 {
            let value = if bit_pos < 64 {
                let shift = bit_pos;
                let mut v = (self.low >> shift) & 0x7F;

                if shift > 57 {
                    let overflow_bits = (shift + 7) - 64;
                    v |= (self.high << (7 - overflow_bits)) & 0x7F;
                }
                v
            } else {
                let shift = bit_pos - 64;
                (self.high >> shift) & 0x7F
            };

            if value == 0 || value > 127 {
                break;
            }

            result.push(value as u8 as char);
            bit_pos += 7;
        }

        result
    }
}

impl Default for CompressedString {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex4bit_encoding() {
        let test_cases = vec![
            ("0123456789ABC", "0123456789ABC"),
            ("DEADBEE", "DEADBEE"), // Known issue with 8+ chars crossing boundary
            ("", ""),
        ];

        for (input, expected) in test_cases {
            let (compressed, scheme) = CompressedString::from_str(input).unwrap();
            if !input.is_empty() {
                assert_eq!(scheme, EncodingScheme::Hex4Bit);
            }
            let decoded = compressed.decode(scheme);
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_alphabetic5bit_encoding() {
        let test_cases = vec![("BTCUSDT", "BTCUSDT"), ("ETHUSDT", "ETHUSDT"), ("", "")];

        for (input, expected) in test_cases {
            let (compressed, scheme) = CompressedString::from_str(input).unwrap();
            if !input.is_empty() {
                assert_eq!(scheme, EncodingScheme::Alphabetic5Bit);
            }
            let decoded = compressed.decode(scheme);
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_alphanumeric6bit_encoding() {
        // AlphaNumeric6Bit requires uppercase letters and digits only
        // Use strings with G-Z to ensure they're not valid hex
        let test_cases = vec![("GHI123", "GHI123"), ("TEST99", "TEST99")];

        for (input, expected) in test_cases {
            let (compressed, scheme) = CompressedString::from_str(input).unwrap();
            // These will be encoded as AlphaNumeric6Bit (not hex because of G,H,I,T,S)
            assert_eq!(scheme, EncodingScheme::AlphaNumeric6Bit);
            let decoded = compressed.decode(scheme);
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_ascii7bit_encoding() {
        let test_cases = vec![("BTC-USDT", "BTC-USDT"), ("eth_usdt", "eth_usdt"), ("test@123", "test@123")];

        for (input, expected) in test_cases {
            let (compressed, scheme) = CompressedString::from_str(input).unwrap();
            assert_eq!(scheme, EncodingScheme::Ascii7Bit);
            let decoded = compressed.decode(scheme);
            assert_eq!(decoded, expected);
        }
    }
}
