use serde::Deserialize;
use serde::Deserializer;

use crate::fixed_point::parse_json_decimal_to_fixed_point;

/// Deserialize a JSON string field directly to i64 fixed-point
///
/// Avoids allocating String, parses bytes directly to fixed-point
pub fn deserialize_fixed_point_string<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    parse_json_decimal_to_fixed_point(s.as_bytes()).map_err(serde::de::Error::custom)
}

/// Deserialize a tuple of JSON strings directly to (i64, i64) fixed-point
///
/// For orderbook levels: ("price", "quantity") → (price_fixed, qty_fixed)
pub fn deserialize_price_levels<'de, D>(deserializer: D) -> Result<Vec<(i64, i64)>, D::Error>
where
    D: Deserializer<'de>,
{
    let levels: Vec<(&str, &str)> = Deserialize::deserialize(deserializer)?;

    levels
        .into_iter()
        .map(|(price_str, qty_str)| {
            let price = parse_json_decimal_to_fixed_point(price_str.as_bytes()).map_err(serde::de::Error::custom)?;
            let qty = parse_json_decimal_to_fixed_point(qty_str.as_bytes()).map_err(serde::de::Error::custom)?;
            Ok((price, qty))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[test]
    fn test_deserialize_fixed_point_string() {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_fixed_point_string")]
            price: i64,
        }

        let json = r#"{"price":"42250.15"}"#;
        let result: TestStruct = serde_json::from_str(json).unwrap();
        assert_eq!(result.price, 4225015000000);
    }

    #[test]
    fn test_deserialize_price_levels() {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_price_levels")]
            bids: Vec<(i64, i64)>,
        }

        let json = r#"{"bids":[["42250.15","1.5"],["42250.10","2.0"]]}"#;
        let result: TestStruct = serde_json::from_str(json).unwrap();

        assert_eq!(result.bids.len(), 2);
        assert_eq!(result.bids[0].0, 4225015000000); // price
        assert_eq!(result.bids[0].1, 150000000); // quantity
        assert_eq!(result.bids[1].0, 4225010000000);
        assert_eq!(result.bids[1].1, 200000000);
    }
}
