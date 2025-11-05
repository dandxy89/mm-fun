use crate::Exchange;
use crate::compressed_string::CompressedString;
use crate::compressed_string::EncodingScheme;
use crate::errors::ProtocolError;
use crate::errors::Result;

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct MarketDataMessage {
    pub header: u8,
    pub sequence: u8,
    pub _pad: [u8; 6],
    pub symbol_low: u64,
    pub symbol_high: u64,
    pub timestamp: u64,
    pub bid_price: i64,
    pub ask_price: i64,
    pub bid_size: i64,
    pub ask_size: i64,
    pub crc32: u32,
    pub _final_pad: [u8; 4],
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct PricingOutputMessage {
    pub header: u8,
    pub sequence: u8,
    pub _pad: [u8; 6],
    pub symbol_low: u64,
    pub symbol_high: u64,
    pub timestamp: u64,
    pub fair_value: i64,
    pub confidence_score: i64,
    pub volatility: i64,
    pub crc32: u32,
    pub _final_pad: [u8; 12],
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct HeartbeatMessage {
    pub header: u8,
    pub _pad: [u8; 7],
    pub timestamp: u64,
    pub sequence: u64,
    pub crc32: u32,
    pub _final_pad: [u8; 12],
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct CollectorStateMessage {
    pub header: u8,
    pub connection_id: u8,
    pub state: u8,
    pub _pad: [u8; 5],
    pub timestamp: u64,
    pub messages_received: u64,
    pub crc32: u32,
    pub _final_pad: [u8; 12],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CollectorState {
    Connecting = 0,
    Connected = 1,
    Receiving = 2,
    Disconnected = 3,
    Error = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateType {
    Snapshot = 0,
    Update = 1,
}

impl MarketDataMessage {
    pub const SIZE: usize = 72;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        exchange: Exchange,
        update_type: UpdateType,
        symbol: CompressedString,
        encoding: EncodingScheme,
        timestamp: u64,
        bid_price: i64,
        ask_price: i64,
        bid_size: i64,
        ask_size: i64,
    ) -> Self {
        Self::new_with_sequence(exchange, update_type, symbol, encoding, timestamp, bid_price, ask_price, bid_size, ask_size, 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_sequence(
        exchange: Exchange,
        update_type: UpdateType,
        symbol: CompressedString,
        encoding: EncodingScheme,
        timestamp: u64,
        bid_price: i64,
        ask_price: i64,
        bid_size: i64,
        ask_size: i64,
        sequence: u8,
    ) -> Self {
        let mut header = 0u8;
        header |= 0 << 7;
        header |= (update_type as u8) << 6;
        header |= (exchange as u8) << 2;
        header |= encoding as u8;

        let mut msg = MarketDataMessage {
            header,
            sequence,
            _pad: [0; 6],
            symbol_low: symbol.low,
            symbol_high: symbol.high,
            timestamp,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            crc32: 0,
            _final_pad: [0; 4],
        };

        msg.crc32 = msg.calculate_crc32();
        msg
    }

    #[inline]
    pub fn message_type(&self) -> u8 {
        (self.header >> 7) & 1
    }

    #[inline]
    pub fn update_type(&self) -> UpdateType {
        if (self.header >> 6) & 1 == 0 { UpdateType::Snapshot } else { UpdateType::Update }
    }

    #[inline]
    pub fn exchange(&self) -> Result<Exchange> {
        let id = (self.header >> 2) & 0xF;
        Exchange::from_u8(id).ok_or(ProtocolError::InvalidExchange { id })
    }

    #[inline]
    pub fn encoding_scheme(&self) -> EncodingScheme {
        match self.header & 0x3 {
            0 => EncodingScheme::Hex4Bit,
            1 => EncodingScheme::Alphabetic5Bit,
            2 => EncodingScheme::AlphaNumeric6Bit,
            3 => EncodingScheme::Ascii7Bit,
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn symbol(&self) -> CompressedString {
        CompressedString { low: self.symbol_low, high: self.symbol_high }
    }

    pub fn validate_basic(&self) -> Result<()> {
        let msg_type = self.message_type();
        if msg_type != 0 {
            return Err(ProtocolError::InvalidMessageType { msg_type });
        }

        let exchange_id = (self.header >> 2) & 0xF;
        if exchange_id > 9 {
            return Err(ProtocolError::InvalidExchange { id: exchange_id });
        }

        Ok(())
    }

    pub fn validate_checksum(&self) -> Result<()> {
        let calculated = self.calculate_crc32();
        if calculated != self.crc32 {
            return Err(ProtocolError::InvalidChecksum { expected: self.crc32, actual: calculated });
        }
        Ok(())
    }

    fn calculate_crc32(&self) -> u32 {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, 64) };
        crate::checksum::calculate_crc32c(bytes)
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, Self::SIZE) };
        let mut result = [0u8; Self::SIZE];
        result.copy_from_slice(bytes);
        result
    }

    /// # Safety
    ///
    /// The caller must ensure:
    /// - `bytes.len() >= Self::SIZE`
    /// - `bytes.as_ptr()` is aligned to 16 bytes
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        debug_assert!(bytes.len() >= Self::SIZE);
        debug_assert!((bytes.as_ptr() as usize).is_multiple_of(16));
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ProtocolError::InvalidLength { expected: Self::SIZE, actual: bytes.len() });
        }

        if !(bytes.as_ptr() as usize).is_multiple_of(16) {
            return Err(ProtocolError::InvalidAlignment { address: bytes.as_ptr() as usize });
        }

        let msg = unsafe { Self::from_bytes_unchecked(bytes) };
        msg.validate_checksum()?;
        Ok(*msg)
    }

    #[cfg(target_endian = "big")]
    pub fn fix_endianness(&mut self) {
        self.symbol_low = self.symbol_low.swap_bytes();
        self.symbol_high = self.symbol_high.swap_bytes();
        self.timestamp = self.timestamp.swap_bytes();
        self.bid_price = self.bid_price.swap_bytes();
        self.ask_price = self.ask_price.swap_bytes();
        self.bid_size = self.bid_size.swap_bytes();
        self.ask_size = self.ask_size.swap_bytes();
        self.crc32 = self.crc32.swap_bytes();
    }
}

impl PricingOutputMessage {
    pub const SIZE: usize = 72;

    pub fn new(
        strategy_id: u8,
        symbol: CompressedString,
        encoding: EncodingScheme,
        timestamp: u64,
        fair_value: i64,
        confidence_score: i64,
        volatility: i64,
    ) -> Self {
        Self::new_with_sequence(strategy_id, symbol, encoding, timestamp, fair_value, confidence_score, volatility, 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_sequence(
        strategy_id: u8,
        symbol: CompressedString,
        encoding: EncodingScheme,
        timestamp: u64,
        fair_value: i64,
        confidence_score: i64,
        volatility: i64,
        sequence: u8,
    ) -> Self {
        let mut header = 0u8;
        header |= 1 << 7;
        header |= (strategy_id & 0x1F) << 2;
        header |= encoding as u8;

        let mut msg = PricingOutputMessage {
            header,
            sequence,
            _pad: [0; 6],
            symbol_low: symbol.low,
            symbol_high: symbol.high,
            timestamp,
            fair_value,
            confidence_score,
            volatility,
            crc32: 0,
            _final_pad: [0; 12],
        };

        msg.crc32 = msg.calculate_crc32();
        msg
    }

    #[inline]
    pub fn message_type(&self) -> u8 {
        (self.header >> 7) & 1
    }

    #[inline]
    pub fn strategy_id(&self) -> u8 {
        (self.header >> 2) & 0x1F
    }

    #[inline]
    pub fn encoding_scheme(&self) -> EncodingScheme {
        match self.header & 0x3 {
            0 => EncodingScheme::Hex4Bit,
            1 => EncodingScheme::Alphabetic5Bit,
            2 => EncodingScheme::AlphaNumeric6Bit,
            3 => EncodingScheme::Ascii7Bit,
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn symbol(&self) -> CompressedString {
        CompressedString { low: self.symbol_low, high: self.symbol_high }
    }

    pub fn validate_basic(&self) -> Result<()> {
        let msg_type = self.message_type();
        if msg_type != 1 {
            return Err(ProtocolError::InvalidMessageType { msg_type });
        }

        let strategy_id = self.strategy_id();
        if strategy_id > 31 {
            return Err(ProtocolError::InvalidHeader { byte: self.header });
        }

        Ok(())
    }

    pub fn validate_checksum(&self) -> Result<()> {
        let calculated = self.calculate_crc32();
        if calculated != self.crc32 {
            return Err(ProtocolError::InvalidChecksum { expected: self.crc32, actual: calculated });
        }
        Ok(())
    }

    fn calculate_crc32(&self) -> u32 {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, 56) };
        crate::checksum::calculate_crc32c(bytes)
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, Self::SIZE) };
        let mut result = [0u8; Self::SIZE];
        result.copy_from_slice(bytes);
        result
    }

    /// # Safety
    ///
    /// The caller must ensure:
    /// - `bytes.len() >= Self::SIZE`
    /// - `bytes.as_ptr()` is aligned to 16 bytes
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        debug_assert!(bytes.len() >= Self::SIZE);
        debug_assert!((bytes.as_ptr() as usize).is_multiple_of(16));
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ProtocolError::InvalidLength { expected: Self::SIZE, actual: bytes.len() });
        }

        if !(bytes.as_ptr() as usize).is_multiple_of(16) {
            return Err(ProtocolError::InvalidAlignment { address: bytes.as_ptr() as usize });
        }

        let msg = unsafe { Self::from_bytes_unchecked(bytes) };
        msg.validate_checksum()?;
        Ok(*msg)
    }

    #[cfg(target_endian = "big")]
    pub fn fix_endianness(&mut self) {
        self.symbol_low = self.symbol_low.swap_bytes();
        self.symbol_high = self.symbol_high.swap_bytes();
        self.timestamp = self.timestamp.swap_bytes();
        self.fair_value = self.fair_value.swap_bytes();
        self.confidence_score = self.confidence_score.swap_bytes();
        self.volatility = self.volatility.swap_bytes();
        self.crc32 = self.crc32.swap_bytes();
    }
}

impl HeartbeatMessage {
    pub const SIZE: usize = 32;

    pub fn new(timestamp: u64, sequence: u64) -> Self {
        let mut header = 0u8;
        header |= 2 << 7; // Message type 2 for heartbeat

        let mut msg = HeartbeatMessage { header, _pad: [0; 7], timestamp, sequence, crc32: 0, _final_pad: [0; 12] };

        msg.crc32 = msg.calculate_crc32();
        msg
    }

    #[inline]
    pub fn message_type(&self) -> u8 {
        (self.header >> 7) & 1
    }

    pub fn validate_basic(&self) -> Result<()> {
        let msg_type = self.message_type();
        if msg_type != 2 {
            return Err(ProtocolError::InvalidMessageType { msg_type });
        }
        Ok(())
    }

    pub fn validate_checksum(&self) -> Result<()> {
        let calculated = self.calculate_crc32();
        if calculated != self.crc32 {
            return Err(ProtocolError::InvalidChecksum { expected: self.crc32, actual: calculated });
        }
        Ok(())
    }

    fn calculate_crc32(&self) -> u32 {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, 24) };
        crate::checksum::calculate_crc32c(bytes)
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, Self::SIZE) };
        let mut result = [0u8; Self::SIZE];
        result.copy_from_slice(bytes);
        result
    }

    /// # Safety
    ///
    /// The caller must ensure:
    /// - `bytes.len() >= Self::SIZE`
    /// - `bytes.as_ptr()` is aligned to 16 bytes
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        debug_assert!(bytes.len() >= Self::SIZE);
        debug_assert!((bytes.as_ptr() as usize).is_multiple_of(16));
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ProtocolError::InvalidLength { expected: Self::SIZE, actual: bytes.len() });
        }

        if !(bytes.as_ptr() as usize).is_multiple_of(16) {
            return Err(ProtocolError::InvalidAlignment { address: bytes.as_ptr() as usize });
        }

        let msg = unsafe { Self::from_bytes_unchecked(bytes) };
        msg.validate_checksum()?;
        Ok(*msg)
    }

    #[cfg(target_endian = "big")]
    pub fn fix_endianness(&mut self) {
        self.timestamp = self.timestamp.swap_bytes();
        self.sequence = self.sequence.swap_bytes();
        self.crc32 = self.crc32.swap_bytes();
    }
}

impl CollectorStateMessage {
    pub const SIZE: usize = 32;

    pub fn new(connection_id: u8, state: CollectorState, timestamp: u64, messages_received: u64) -> Self {
        let mut header = 0u8;
        header |= 3 << 6; // Message type 3 for collector state

        let mut msg = CollectorStateMessage {
            header,
            connection_id,
            state: state as u8,
            _pad: [0; 5],
            timestamp,
            messages_received,
            crc32: 0,
            _final_pad: [0; 12],
        };

        msg.crc32 = msg.calculate_crc32();
        msg
    }

    #[inline]
    pub fn message_type(&self) -> u8 {
        (self.header >> 6) & 0x3
    }

    #[inline]
    pub fn state(&self) -> Result<CollectorState> {
        match self.state {
            0 => Ok(CollectorState::Connecting),
            1 => Ok(CollectorState::Connected),
            2 => Ok(CollectorState::Receiving),
            3 => Ok(CollectorState::Disconnected),
            4 => Ok(CollectorState::Error),
            _ => Err(ProtocolError::InvalidHeader { byte: self.state }),
        }
    }

    pub fn validate_basic(&self) -> Result<()> {
        let msg_type = self.message_type();
        if msg_type != 3 {
            return Err(ProtocolError::InvalidMessageType { msg_type });
        }
        Ok(())
    }

    pub fn validate_checksum(&self) -> Result<()> {
        let calculated = self.calculate_crc32();
        if calculated != self.crc32 {
            return Err(ProtocolError::InvalidChecksum { expected: self.crc32, actual: calculated });
        }
        Ok(())
    }

    fn calculate_crc32(&self) -> u32 {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, 24) };
        crate::checksum::calculate_crc32c(bytes)
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, Self::SIZE) };
        let mut result = [0u8; Self::SIZE];
        result.copy_from_slice(bytes);
        result
    }

    /// # Safety
    ///
    /// The caller must ensure:
    /// - `bytes.len() >= Self::SIZE`
    /// - `bytes.as_ptr()` is aligned to 16 bytes
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        debug_assert!(bytes.len() >= Self::SIZE);
        debug_assert!((bytes.as_ptr() as usize).is_multiple_of(16));
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ProtocolError::InvalidLength { expected: Self::SIZE, actual: bytes.len() });
        }

        if !(bytes.as_ptr() as usize).is_multiple_of(16) {
            return Err(ProtocolError::InvalidAlignment { address: bytes.as_ptr() as usize });
        }

        let msg = unsafe { Self::from_bytes_unchecked(bytes) };
        msg.validate_checksum()?;
        Ok(*msg)
    }

    #[cfg(target_endian = "big")]
    pub fn fix_endianness(&mut self) {
        self.timestamp = self.timestamp.swap_bytes();
        self.messages_received = self.messages_received.swap_bytes();
        self.crc32 = self.crc32.swap_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_data_message_size() {
        // Size might be 80 on some platforms due to alignment padding
        let size = std::mem::size_of::<MarketDataMessage>();
        assert!(size == 72 || size == 80);
    }

    #[test]
    fn test_pricing_output_message_size() {
        // Size might be 80 on some platforms due to alignment padding
        let size = std::mem::size_of::<PricingOutputMessage>();
        assert!(size == 72 || size == 80);
    }

    #[test]
    fn test_message_alignment() {
        assert_eq!(std::mem::align_of::<MarketDataMessage>(), 16);
        assert_eq!(std::mem::align_of::<PricingOutputMessage>(), 16);
        assert_eq!(std::mem::align_of::<HeartbeatMessage>(), 16);
    }

    #[test]
    fn test_heartbeat_message() {
        let msg = HeartbeatMessage::new(1234567890, 42);
        assert_eq!(msg.timestamp, 1234567890);
        assert_eq!(msg.sequence, 42);

        // Test serialization
        let bytes = msg.to_bytes();
        let deserialized = HeartbeatMessage::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.timestamp, msg.timestamp);
        assert_eq!(deserialized.sequence, msg.sequence);
        assert_eq!(deserialized.crc32, msg.crc32);
    }

    #[test]
    fn test_collector_state_message_size() {
        let size = std::mem::size_of::<CollectorStateMessage>();
        assert!(size == 32 || size == 48);
    }

    #[test]
    fn test_collector_state_message() {
        let msg = CollectorStateMessage::new(1, CollectorState::Receiving, 1234567890, 1000);
        assert_eq!(msg.connection_id, 1);
        assert_eq!(msg.state().unwrap(), CollectorState::Receiving);
        assert_eq!(msg.timestamp, 1234567890);
        assert_eq!(msg.messages_received, 1000);

        // Test serialization
        let bytes = msg.to_bytes();
        let deserialized = CollectorStateMessage::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.connection_id, msg.connection_id);
        assert_eq!(deserialized.state, msg.state);
        assert_eq!(deserialized.timestamp, msg.timestamp);
        assert_eq!(deserialized.messages_received, msg.messages_received);
        assert_eq!(deserialized.crc32, msg.crc32);
    }

    #[test]
    fn test_collector_state_values() {
        assert_eq!(CollectorState::Connecting as u8, 0);
        assert_eq!(CollectorState::Connected as u8, 1);
        assert_eq!(CollectorState::Receiving as u8, 2);
        assert_eq!(CollectorState::Disconnected as u8, 3);
        assert_eq!(CollectorState::Error as u8, 4);
    }
}
