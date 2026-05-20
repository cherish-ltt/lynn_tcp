//! Message validation logic — pure domain-level validation.
//!
//! These functions validate message length and format constraints as business rules.
//! They have no infrastructure dependencies and belong to the domain layer.

use crate::LynnError;

/// Maximum message size in bytes (10MB)
/// This prevents memory exhaustion attacks from malicious large messages
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Minimum message size in bytes (at least constructor_id + method_id)
pub const MIN_MESSAGE_SIZE: usize = 3;

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

#[cfg(test)]
#[cfg(feature = "server")]
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
}
