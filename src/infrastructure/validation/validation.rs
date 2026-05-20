//! Input validation module for lynn_tcp framework
//!
//! This module provides validation functions to ensure security and prevent
//! common attacks such as memory exhaustion, buffer overflows, and DDoS.

use crate::LynnError;

use crate::domain::model::message_validation::{
    MAX_MESSAGE_SIZE, MIN_MESSAGE_SIZE, validate_message_length
};

/// Default maximum buffer size for connections
pub const DEFAULT_MAX_BUFFER_SIZE: usize = 16 * 1024 * 1024; // 16MB

/// Validates the complete message format according to the protocol
///
/// # Arguments
///
/// * `data` - The raw message bytes
/// * `message_header_mark` - Expected header mark (2 bytes)
/// * `message_tail_mark` - Expected tail mark (2 bytes)
///
/// # Returns
///
/// * `Ok(usize)` - The validated message body length
/// * `Err(LynnError)` - If the message format is invalid
pub fn validate_message_format(
    data: &[u8],
    message_header_mark: u16,
    message_tail_mark: u16,
) -> Result<usize, LynnError> {
    // Check minimum size for header + length field
    if data.len() < 10 {
        return Err(LynnError::protocol(format!(
            "Message too short: {} bytes (minimum 10 for header+length)",
            data.len()
        )));
    }

    // Validate header mark (bytes 0-1)
    let header = u16::from_le_bytes([data[0], data[1]]);
    if header != message_header_mark {
        return Err(LynnError::protocol(format!(
            "Invalid header mark: 0x{:04X} (expected 0x{:04X})",
            header, message_header_mark
        )));
    }

    // Extract message length (bytes 2-9)
    let msg_len = u64::from_le_bytes([
        data[2], data[3], data[4], data[5],
        data[6], data[7], data[8], data[9],
    ]);

    // Validate message length
    let body_len = validate_message_length(msg_len)?;

    // Check if we have the complete message including tail
    let total_expected_size = 10 + body_len + 2; // header + length + body + tail
    if data.len() < total_expected_size {
        return Err(LynnError::protocol(format!(
            "Incomplete message: {} bytes (expected {} for complete message)",
            data.len(), total_expected_size
        )));
    }

    // Validate tail mark (last 2 bytes)
    let tail_pos = 10 + body_len;
    let tail = u16::from_le_bytes([data[tail_pos], data[tail_pos + 1]]);
    if tail != message_tail_mark {
        return Err(LynnError::protocol(format!(
            "Invalid tail mark: 0x{:04X} (expected 0x{:04X})",
            tail, message_tail_mark
        )));
    }

    Ok(body_len)
}

/// Safe buffer with overflow protection
pub struct SafeBuffer {
    data: Vec<u8>,
    max_size: usize,
}

impl SafeBuffer {
    /// Creates a new safe buffer
    ///
    /// # Arguments
    ///
    /// * `max_size` - Maximum buffer size in bytes
    pub fn new(max_size: usize) -> Self {
        Self {
            data: Vec::with_capacity(4096), // Start with 4KB
            max_size,
        }
    }

    /// Extends the buffer with new data, checking for overflow
    ///
    /// # Arguments
    ///
    /// * `data` - The data to append
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the data was added successfully
    /// * `Err(LynnError)` - If adding the data would exceed the maximum size
    pub fn extend(&mut self, data: &[u8]) -> Result<(), LynnError> {
        // Check for overflow
        if data.len() > self.max_size {
            return Err(LynnError::buffer(format!(
                "Single data chunk too large: {} bytes (maximum {})",
                data.len(), self.max_size
            )));
        }

        if self.data.len() + data.len() > self.max_size {
            return Err(LynnError::buffer(format!(
                "Buffer overflow: current={} bytes, adding={} bytes, maximum={} bytes",
                self.data.len(),
                data.len(),
                self.max_size
            )));
        }

        self.data.extend_from_slice(data);
        Ok(())
    }

    /// Clears the buffer
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Returns the current buffer length
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Returns a reference to the underlying data
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Returns the maximum buffer size
    pub fn max_size(&self) -> usize {
        self.max_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_message_format() {
        let header: u16 = 0x23E9;
        let tail: u16 = 0x1E27;
        let mut data = vec![0u8; 20];

        // Set header
        data[0..2].copy_from_slice(&header.to_le_bytes());
        // Set message length (body = 6 bytes)
        data[2..10].copy_from_slice(&6u64.to_le_bytes());

        // Set tail at position 16
        data[16..18].copy_from_slice(&tail.to_le_bytes());

        assert!(validate_message_format(&data, header, tail).is_ok());
    }

    #[test]
    fn test_safe_buffer() {
        let mut buffer = SafeBuffer::new(100);

        assert!(buffer.extend(&[1, 2, 3]).is_ok());
        assert_eq!(buffer.len(), 3);

        // Test overflow protection
        assert!(buffer.extend(&[0u8; 200]).is_err());
    }
}
