use crate::CompressedString;
use crate::Exchange;
use crate::checksum;
use crate::compressed_string::EncodingScheme;
use crate::errors::ProtocolError;
use crate::errors::Result;
use crate::messages::UpdateType;

/// Header for orderbook batch messages
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct OrderBookBatchHeader {
    /// Protocol version and message type (0x03 for orderbook batch)
    pub header: u8,
    /// Exchange identifier
    pub exchange: u8,
    /// Update type (0 = snapshot, 1 = update)
    pub update_type: u8,
    /// Symbol encoding scheme
    pub encoding: u8,
    /// Number of bid levels in this message
    pub num_bids: u16,
    /// Number of ask levels in this message
    pub num_asks: u16,
    /// Compressed symbol (low 64 bits)
    pub symbol_low: u64,
    /// Compressed symbol (high 64 bits)
    pub symbol_high: u64,
    /// Exchange timestamp in milliseconds
    pub timestamp: u64,
}

/// Single price level (bid or ask)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct PriceLevel {
    /// Price in fixed-point format
    pub price: i64,
    /// Size/quantity in fixed-point format
    pub size: i64,
}

impl PriceLevel {
    pub const SIZE: usize = 16; // 8 + 8 bytes

    #[inline]
    pub fn new(price: i64, size: i64) -> Self {
        Self { price, size }
    }

    #[inline]
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.price.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.size.to_le_bytes());
        bytes
    }

    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ProtocolError::BufferTooSmall { required: Self::SIZE, actual: bytes.len() });
        }

        Ok(Self { price: i64::from_le_bytes(bytes[0..8].try_into().unwrap()), size: i64::from_le_bytes(bytes[8..16].try_into().unwrap()) })
    }
}

/// Builder for creating orderbook batch messages
pub struct OrderBookBatchMessage {
    header: OrderBookBatchHeader,
    bids: Vec<PriceLevel>,
    asks: Vec<PriceLevel>,
}

impl OrderBookBatchMessage {
    pub const HEADER_SIZE: usize = 32;
    pub const MESSAGE_TYPE: u8 = 0x03;

    /// Create a new orderbook batch message
    pub fn new(exchange: Exchange, update_type: UpdateType, symbol: CompressedString, encoding: EncodingScheme, timestamp: u64) -> Self {
        let header = OrderBookBatchHeader {
            header: Self::MESSAGE_TYPE,
            exchange: exchange as u8,
            update_type: update_type as u8,
            encoding: encoding as u8,
            num_bids: 0,
            num_asks: 0,
            symbol_low: symbol.low,
            symbol_high: symbol.high,
            timestamp,
        };

        Self { header, bids: Vec::new(), asks: Vec::new() }
    }

    /// Add a bid level
    #[inline]
    pub fn add_bid(&mut self, price: i64, size: i64) {
        self.bids.push(PriceLevel::new(price, size));
    }

    /// Add an ask level
    #[inline]
    pub fn add_ask(&mut self, price: i64, size: i64) {
        self.asks.push(PriceLevel::new(price, size));
    }

    /// Add multiple bids at once
    pub fn add_bids(&mut self, levels: impl IntoIterator<Item = (i64, i64)>) {
        for (price, size) in levels {
            self.add_bid(price, size);
        }
    }

    /// Add multiple asks at once
    pub fn add_asks(&mut self, levels: impl IntoIterator<Item = (i64, i64)>) {
        for (price, size) in levels {
            self.add_ask(price, size);
        }
    }

    /// Get the total message size in bytes
    pub fn size(&self) -> usize {
        Self::HEADER_SIZE + (self.bids.len() + self.asks.len()) * PriceLevel::SIZE + 4 // +4 for CRC32
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let total_size = self.size();
        let mut bytes = Vec::with_capacity(total_size);

        // Update header counts
        let mut header = self.header;
        header.num_bids = self.bids.len() as u16;
        header.num_asks = self.asks.len() as u16;

        // Serialize header
        bytes.push(header.header);
        bytes.push(header.exchange);
        bytes.push(header.update_type);
        bytes.push(header.encoding);
        bytes.extend_from_slice(&header.num_bids.to_le_bytes());
        bytes.extend_from_slice(&header.num_asks.to_le_bytes());
        bytes.extend_from_slice(&header.symbol_low.to_le_bytes());
        bytes.extend_from_slice(&header.symbol_high.to_le_bytes());
        bytes.extend_from_slice(&header.timestamp.to_le_bytes());

        // Serialize bids
        for bid in &self.bids {
            bytes.extend_from_slice(&bid.to_bytes());
        }

        // Serialize asks
        for ask in &self.asks {
            bytes.extend_from_slice(&ask.to_bytes());
        }

        // Calculate and append CRC32C
        let crc = checksum::calculate_crc32c(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());

        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::HEADER_SIZE + 4 {
            return Err(ProtocolError::BufferTooSmall { required: Self::HEADER_SIZE + 4, actual: bytes.len() });
        }

        // Verify CRC32C
        let crc_offset = bytes.len() - 4;
        let expected_crc = u32::from_le_bytes(bytes[crc_offset..].try_into().unwrap());
        let actual_crc = checksum::calculate_crc32c(&bytes[..crc_offset]);
        if expected_crc != actual_crc {
            return Err(ProtocolError::InvalidChecksum { expected: expected_crc, actual: actual_crc });
        }

        // Parse header
        if bytes[0] != Self::MESSAGE_TYPE {
            return Err(ProtocolError::InvalidMessageType { msg_type: bytes[0] });
        }

        let header = OrderBookBatchHeader {
            header: bytes[0],
            exchange: bytes[1],
            update_type: bytes[2],
            encoding: bytes[3],
            num_bids: u16::from_le_bytes([bytes[4], bytes[5]]),
            num_asks: u16::from_le_bytes([bytes[6], bytes[7]]),
            symbol_low: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            symbol_high: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            timestamp: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        };

        let mut offset = Self::HEADER_SIZE;

        // Parse bids
        let mut bids = Vec::with_capacity(header.num_bids as usize);
        for _ in 0..header.num_bids {
            bids.push(PriceLevel::from_bytes(&bytes[offset..])?);
            offset += PriceLevel::SIZE;
        }

        // Parse asks
        let mut asks = Vec::with_capacity(header.num_asks as usize);
        for _ in 0..header.num_asks {
            asks.push(PriceLevel::from_bytes(&bytes[offset..])?);
            offset += PriceLevel::SIZE;
        }

        Ok(Self { header, bids, asks })
    }

    /// Get the exchange
    pub fn exchange(&self) -> Result<Exchange> {
        Exchange::from_u8(self.header.exchange).ok_or(ProtocolError::InvalidExchange { id: self.header.exchange })
    }

    /// Get the update type
    pub fn update_type(&self) -> UpdateType {
        if self.header.update_type == 0 { UpdateType::Snapshot } else { UpdateType::Update }
    }

    /// Get the symbol
    pub fn symbol(&self) -> CompressedString {
        CompressedString { low: self.header.symbol_low, high: self.header.symbol_high }
    }

    /// Get the encoding scheme
    pub fn encoding(&self) -> EncodingScheme {
        match self.header.encoding {
            0 => EncodingScheme::Hex4Bit,
            1 => EncodingScheme::Alphabetic5Bit,
            2 => EncodingScheme::AlphaNumeric6Bit,
            _ => EncodingScheme::Ascii7Bit,
        }
    }

    /// Get the timestamp
    pub fn timestamp(&self) -> u64 {
        self.header.timestamp
    }

    /// Get bids
    pub fn bids(&self) -> &[PriceLevel] {
        &self.bids
    }

    /// Get asks
    pub fn asks(&self) -> &[PriceLevel] {
        &self.asks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::to_fixed_point;

    #[test]
    fn test_orderbook_batch_roundtrip() {
        let symbol = CompressedString::from_str("BTCUSDT").unwrap().0;
        let encoding = CompressedString::from_str("BTCUSDT").unwrap().1;

        let mut msg = OrderBookBatchMessage::new(Exchange::Binance, UpdateType::Snapshot, symbol, encoding, 1234567890);

        msg.add_bid(to_fixed_point(50000.0), to_fixed_point(1.5));
        msg.add_bid(to_fixed_point(49999.0), to_fixed_point(2.0));
        msg.add_ask(to_fixed_point(50001.0), to_fixed_point(1.2));
        msg.add_ask(to_fixed_point(50002.0), to_fixed_point(0.8));

        let bytes = msg.to_bytes();
        let decoded = OrderBookBatchMessage::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.bids().len(), 2);
        assert_eq!(decoded.asks().len(), 2);
        assert_eq!(decoded.timestamp(), 1234567890);
    }
}
