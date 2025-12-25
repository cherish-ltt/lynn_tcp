//! Input validation module for lynn_tcp framework
//!
//! This module provides validation functions to ensure security and prevent
//! common attacks such as memory exhaustion, buffer overflows, and DDoS.

use crate::LynnError;
use std::net::SocketAddr;
use std::collections::HashMap;
use std::sync::Arc;
use std::net::IpAddr;
use dashmap::DashMap;
use tracing::{warn, error};

/// Maximum message size in bytes (10MB)
/// This prevents memory exhaustion attacks from malicious large messages
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Minimum message size in bytes (at least constructor_id + method_id)
pub const MIN_MESSAGE_SIZE: usize = 3;

/// Default maximum number of connections per IP address
pub const DEFAULT_MAX_CONNECTIONS_PER_IP: usize = 10;

/// Default message rate limit (messages per second per client)
pub const DEFAULT_MESSAGE_RATE_LIMIT: usize = 1000;

/// Default maximum buffer size for connections
pub const DEFAULT_MAX_BUFFER_SIZE: usize = 16 * 1024 * 1024; // 16MB

/// Validates message length to prevent memory exhaustion attacks
///
/// # Arguments
///
/// * `len` - The claimed message length from the protocol header
///
/// # Returns
///
/// * `Ok(usize)` - The validated length
/// * `Err(LynnError)` - If the length is invalid
pub fn validate_message_length(len: u64) -> Result<usize, LynnError> {
    // Check for overflow when converting to usize
    let len = if len > usize::MAX as u64 {
        return Err(LynnError::protocol(format!(
            "Message length {} exceeds usize::MAX",
            len
        )));
    } else {
        len as usize
    };

    // Check minimum length
    if len < MIN_MESSAGE_SIZE {
        return Err(LynnError::protocol(format!(
            "Message too short: {} bytes (minimum {})",
            len, MIN_MESSAGE_SIZE
        )));
    }

    // Check maximum length
    if len > MAX_MESSAGE_SIZE {
        return Err(LynnError::protocol(format!(
            "Message too large: {} bytes (maximum {})",
            len, MAX_MESSAGE_SIZE
        )));
    }

    Ok(len)
}

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

/// Connection limiter to prevent DDoS attacks
#[derive(Clone)]
pub struct ConnectionLimiter {
    max_connections: usize,
    max_connections_per_ip: usize,
    per_ip_counts: Arc<DashMap<IpAddr, usize>>,
}

impl ConnectionLimiter {
    /// Creates a new connection limiter
    ///
    /// # Arguments
    ///
    /// * `max_connections` - Maximum total connections
    /// * `max_connections_per_ip` - Maximum connections per IP address
    pub fn new(max_connections: usize, max_connections_per_ip: usize) -> Self {
        Self {
            max_connections,
            max_connections_per_ip,
            per_ip_counts: Arc::new(DashMap::new()),
        }
    }

    /// Checks if a new connection from the given address is allowed
    ///
    /// # Arguments
    ///
    /// * `addr` - The socket address of the incoming connection
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the connection is allowed
    /// * `Err(LynnError)` - If the connection should be rejected
    pub fn can_accept_connection(&self, addr: SocketAddr) -> Result<(), LynnError> {
        // Check total connection count
        let total_count: usize = self.per_ip_counts.iter().map(|entry| *entry.value()).sum();

        if total_count >= self.max_connections {
            warn!(
                "Rejecting connection from {}: maximum connections reached ({})",
                addr, total_count
            );
            return Err(LynnError::server(format!(
                "Maximum connections reached: {}",
                self.max_connections
            )));
        }

        // Check per-IP connection count
        let ip = addr.ip();
        let ip_count = *self.per_ip_counts.entry(ip).or_insert(0);

        if ip_count >= self.max_connections_per_ip {
            warn!(
                "Rejecting connection from {}: too many connections from this IP ({})",
                addr, ip_count
            );
            return Err(LynnError::server(format!(
                "Too many connections from IP: {} (limit: {})",
                ip, self.max_connections_per_ip
            )));
        }

        Ok(())
    }

    /// Records a new connection
    pub fn add_connection(&self, addr: SocketAddr) {
        let ip = addr.ip();
        *self.per_ip_counts.entry(ip).or_insert(0) += 1;
    }

    /// Removes a connection
    pub fn remove_connection(&self, addr: SocketAddr) {
        let ip = addr.ip();
        if let Some(mut count) = self.per_ip_counts.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
            }
            // Remove entry if count is zero
            if *count == 0 {
                self.per_ip_counts.remove(&ip);
            }
        }
    }

    /// Gets the current number of connections
    pub fn total_connections(&self) -> usize {
        self.per_ip_counts.iter().map(|entry| *entry.value()).sum()
    }

    /// Gets the number of connections for a specific IP
    pub fn connections_for_ip(&self, ip: IpAddr) -> usize {
        self.per_ip_counts.get(&ip).map(|v| *v).unwrap_or(0)
    }
}

/// Rate limiter for message processing
#[derive(Clone)]
pub struct RateLimiter {
    messages_per_second: usize,
    // Note: For a production implementation, you'd want a more sophisticated
    // time-based rate limiting algorithm (token bucket, sliding window, etc.)
    // This is a simplified version for demonstration
}

impl RateLimiter {
    /// Creates a new rate limiter
    ///
    /// # Arguments
    ///
    /// * `messages_per_second` - Maximum messages allowed per second
    pub fn new(messages_per_second: usize) -> Self {
        Self {
            messages_per_second,
        }
    }

    /// Checks if a message should be allowed based on rate limit
    ///
    /// Note: This is a placeholder. A real implementation would need to
    /// track message counts per client over time windows.
    ///
    /// For now, this always returns Ok to maintain backward compatibility.
    pub fn check_rate(&self) -> Result<(), LynnError> {
        // TODO: Implement actual rate limiting with time tracking
        // For production, consider:
        // - Token bucket algorithm
        // - Sliding window counter
        // - Leaky bucket algorithm
        Ok(())
    }
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
    fn test_validate_message_length_valid() {
        assert_eq!(validate_message_length(100).unwrap(), 100);
        assert_eq!(validate_message_length(1024).unwrap(), 1024);
    }

    #[test]
    fn test_validate_message_length_too_small() {
        assert!(validate_message_length(2).is_err());
        assert!(validate_message_length(0).is_err());
    }

    #[test]
    fn test_validate_message_length_too_large() {
        assert!(validate_message_length(MAX_MESSAGE_SIZE as u64 + 1).is_err());
    }

    #[test]
    fn test_validate_message_format() {
        let header: u16 = 0x23E9;
        let tail: u16 = 0x1E27;
        let mut data = vec
![0u8; 20];

        // Set header
        data[0..2].copy_from_slice(&header.to_le_bytes());
        // Set message length (body = 6 bytes)
        data[2..10].copy_from_slice(&6u64.to_le_bytes());

        // Set tail at position 16
        data[16..18].copy_from_slice(&tail.to_le_bytes());

        assert!(validate_message_format(&data, header, tail).is_ok());
    }

    #[test]
    fn test_connection_limiter() {
        let limiter = ConnectionLimiter::new(100, 5);
        let addr = "127.0.0.1:8080".parse().unwrap();

        // Should allow connections up to the limit
        for _ in 0..5 {
            assert!(limiter.can_accept_connection(addr).is_ok());
            limiter.add_connection(addr);
        }

        // Should reject the 6th connection
        assert!(limiter.can_accept_connection(addr).is_err());

        // Remove one connection
        limiter.remove_connection(addr);

        // Should allow again
        assert!(limiter.can_accept_connection(addr).is_ok());
    }

    #[test]
    fn test_safe_buffer() {
        let mut buffer = SafeBuffer::new(100);

        assert!(buffer.extend(&[1, 2, 3]).is_ok());
        assert_eq!(buffer.len(), 3);

        // Test overflow protection
        assert!(buffer.extend(&vec![0u8; 200]).is_err());
    }
}
