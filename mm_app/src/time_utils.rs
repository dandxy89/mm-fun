use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[inline]
/// Returns the current Unix timestamp in milliseconds
pub fn unix_timestamp_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("System time before Unix epoch").as_millis() as u64
}

#[inline]
/// Returns the current Unix timestamp in seconds
pub fn unix_timestamp_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("System time before Unix epoch").as_secs()
}

#[inline]
/// Calculates the elapsed time in milliseconds since a given timestamp
pub fn elapsed_since_ms(timestamp_ms: u64) -> u64 {
    unix_timestamp_ms().saturating_sub(timestamp_ms)
}
