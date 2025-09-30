use std::fmt;

#[derive(Debug, Clone)]
pub enum ProtocolError {
    InvalidLength { expected: usize, actual: usize },
    InvalidAlignment { address: usize },
    InvalidChecksum { expected: u32, actual: u32 },
    InvalidHeader { byte: u8 },
    InvalidExchange { id: u8 },
    InvalidEncodingScheme { scheme: u8 },
    StringTooLong { length: usize, max: usize },
    InvalidCharacter { char: char, position: usize },
    InvalidMessageType { msg_type: u8 },
    BufferTooSmall { required: usize, actual: usize },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::InvalidLength { expected, actual } => {
                write!(f, "Invalid message length: expected {} bytes, got {} bytes", expected, actual)
            }
            ProtocolError::InvalidAlignment { address } => {
                write!(f, "Invalid memory alignment: address {:#x} is not 16-byte aligned", address)
            }
            ProtocolError::InvalidChecksum { expected, actual } => {
                write!(f, "Invalid checksum: expected {:#x}, calculated {:#x}", expected, actual)
            }
            ProtocolError::InvalidHeader { byte } => {
                write!(f, "Invalid header byte: {:#x}", byte)
            }
            ProtocolError::InvalidExchange { id } => {
                write!(f, "Invalid exchange ID: {} (must be 0-15)", id)
            }
            ProtocolError::InvalidEncodingScheme { scheme } => {
                write!(f, "Invalid encoding scheme: {} (must be 0-3)", scheme)
            }
            ProtocolError::StringTooLong { length, max } => {
                write!(f, "String too long: {} characters exceeds maximum of {}", length, max)
            }
            ProtocolError::InvalidCharacter { char, position } => {
                write!(f, "Invalid character '{}' at position {}", char, position)
            }
            ProtocolError::InvalidMessageType { msg_type } => {
                write!(f, "Invalid message type: {}", msg_type)
            }
            ProtocolError::BufferTooSmall { required, actual } => {
                write!(f, "Buffer too small: {} bytes required, only {} bytes available", required, actual)
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

pub type Result<T> = std::result::Result<T, ProtocolError>;
