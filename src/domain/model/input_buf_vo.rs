use std::net::SocketAddr;

use bytes::BytesMut;

use crate::domain::model::message_validation::validate_message_length;

/// A struct representing a value object for an input buffer.
///
/// This struct is used to encapsulate data received from a network connection.
/// It includes the raw data as a byte vector, an index to track the current
/// position in the data, and the address from which the data was received.
#[derive(Clone)]
#[cfg(any(feature = "server", feature = "client"))]
pub struct InputBufVO {
    /// The raw data received from the network as a byte vector.
    data: BytesMut,
    /// The current index in the data vector, used for reading data in chunks.
    index: usize,
    /// The address from which the data was received.
    input_addr: Option<SocketAddr>,
}

/// Trait for InputBufVO operations
#[cfg(any(feature = "server", feature = "client"))]
pub trait InputBufVOTrait {
    fn get_constructor_id(&mut self) -> Option<u8>;
    fn get_method_id(&mut self) -> Option<u16>;
    fn next_u64(&mut self) -> Option<u64>;
    fn next_u8(&mut self) -> Option<u8>;
    fn next_str_with_len(&mut self, len: u64) -> Option<String>;
    fn get_all_bytes(&self) -> BytesMut;
    fn get_remaining_data_len(&self) -> usize;
}

/// Implementation of methods for the `InputBufVO` struct.
///
/// This implementation provides methods to create a new `InputBufVO` instance,
/// retrieve the input address, and read various data types from the buffer.
impl InputBufVO {
    /// Creates a new `InputBufVO` instance.
    ///
    /// This function takes a byte vector and an input address and returns a new
    /// `InputBufVO` instance with the provided data and address. The index is
    /// initialized to 16, which might be the starting position of a specific
    /// field in the data.
    ///
    /// # Parameters
    ///
    /// - `buf`: A byte vector containing the input data.
    /// - `input_addr`: The address from which the input data was received.
    ///
    /// # Returns
    ///
    /// A new `InputBufVO` instance.
    pub(crate) fn new(buf: BytesMut, input_addr: SocketAddr) -> Self {
        Self {
            data: buf,
            index: 3,
            input_addr: Some(input_addr),
        }
    }

    pub(crate) fn new_none() -> Self {
        Self {
            data: BytesMut::new(),
            index: 3,
            input_addr: None,
        }
    }

    /// Creates a new `InputBufVO` instance without a socket address.
    ///
    /// This function takes a byte vector and returns a new `InputBufVO` instance
    /// with the provided data. The index is initialized to 3, which might be the
    /// starting position of a specific field in the data. The input address is set
    /// to `None` as it is not provided.
    ///
    /// # Parameters
    ///
    /// - `buf`: A byte vector containing the input data.
    ///
    /// # Returns
    ///
    /// A new `InputBufVO` instance.
    pub(crate) fn new_without_socket_addr(buf: BytesMut) -> Self {
        Self {
            data: buf,
            index: 3,
            input_addr: None,
        }
    }

    /// Retrieves the input address.
    ///
    /// This method returns the address from which the input data was received.
    ///
    /// # Returns
    ///
    /// The input address.
    pub fn get_input_addr(&self) -> Option<SocketAddr> {
        self.input_addr
    }
}

impl InputBufVOTrait for InputBufVO {
    /// Retrieves the constructor ID from the input buffer.
    ///
    /// This method extracts a `u64` value from the first 8 bytes of the input
    /// buffer and interprets it as the constructor ID. If the extraction fails,
    /// it returns a default value of 0.
    ///
    /// # Returns
    ///
    /// The constructor ID from the input buffer.
    fn get_constructor_id(&mut self) -> Option<u8> {
        let length = self.data.len();
        if length < 1 {
            None
        } else {
            let bytes = &self.data[0..1];
            match bytes.try_into() {
                Ok(value) => Some(u8::from_le_bytes(value)),
                Err(_) => None,
            }
        }
    }

    /// Retrieves the method ID from the input buffer.
    ///
    /// This method extracts a `u64` value from the 9th to the 16th bytes of the
    /// input buffer and interprets it as the method ID. If the extraction fails,
    /// it returns a default value of 0.
    ///
    /// # Returns
    ///
    /// The method ID from the input buffer.
    fn get_method_id(&mut self) -> Option<u16> {
        let length = self.data.len();
        if length < 3 {
            None
        } else {
            let bytes = &self.data[1..3];
            match bytes.try_into() {
                Ok(value) => Some(u16::from_le_bytes(value)),
                Err(_) => None,
            }
        }
    }

    /// Reads the next `u64` value from the input buffer.
    ///
    /// This method reads 8 bytes from the current index in the input buffer and
    /// interprets them as a `u64` value. It also advances the index by 8 bytes
    /// for the next read. If the read fails, it returns a default value of 0.
    ///
    /// # Returns
    ///
    /// The next `u64` value from the input buffer.
    fn next_u64(&mut self) -> Option<u64> {
        let length = self.data.len();
        if length < self.index + 8 {
            None
        } else {
            let bytes = &self.data[self.index..self.index + 8];
            match bytes.try_into() {
                Ok(value) => {
                    self.index += 8;
                    Some(u64::from_le_bytes(value))
                },
                Err(_) => None,
            }
        }
    }

    /// Reads the next `u8` value from the input buffer.
    ///
    /// This method reads 1 byte from the current index in the input buffer and
    /// interprets it as a `u8` value. It also advances the index by 1 byte for
    /// the next read. If the read fails, it returns a default value of 0.
    ///
    /// # Returns
    ///
    /// The next `u8` value from the input buffer.
    fn next_u8(&mut self) -> Option<u8> {
        let length = self.data.len();
        if length < self.index + 1 {
            None
        } else {
            let bytes = &self.data[self.index..self.index + 1];
            match bytes.try_into() {
                Ok(value) => {
                    self.index += 1;
                    Some(u8::from_le_bytes(value))
                },
                Err(_) => None,
            }
        }
    }

    /// Reads a string of a specified length from the input buffer.
    ///
    /// This method reads a specified number of bytes from the current index in
    /// the input buffer and interprets them as a string. It also advances the
    /// index by the specified length for the next read. If the read fails, it
    /// returns an empty string.
    ///
    /// # Parameters
    ///
    /// - `len`: The length of the string to read.
    ///
    /// # Returns
    ///
    /// The next string from the input buffer.
    fn next_str_with_len(&mut self, len: u64) -> Option<String> {
        // Validate the length to prevent memory exhaustion attacks
        let validated_len = match validate_message_length(len) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("Invalid string length in next_str_with_len: {}", e);
                return None;
            },
        };

        let length = self.data.len();
        if length < self.index + validated_len {
            None
        } else {
            let bytes = &mut self.data[self.index..self.index + validated_len].to_vec();
            self.index += validated_len;
            Some(String::from_utf8_lossy(bytes).to_string())
        }
    }

    /// Retrieves all the bytes from the input buffer.
    ///
    /// This method returns a clone of the entire byte vector that represents
    /// the input data. This can be useful for cases where the entire data needs
    /// to be accessed or processed.
    ///
    /// # Returns
    ///
    /// A vector containing all the bytes from the input buffer.
    fn get_all_bytes(&self) -> BytesMut {
        let mut vec = self.data.clone();
        if vec.len() > 3 {
            return vec.split_off(3);
        } else {
            vec = BytesMut::new();
        }
        vec
    }

    /// Calculates the length of the remaining data in the input buffer.
    ///
    /// This method determines how many bytes are left to be read from the input
    /// buffer based on the current index. It returns the difference between the
    /// total length of the data and the current index. If the index is greater
    /// than or equal to the data length, it returns 0, indicating that there is
    /// no remaining data.
    ///
    /// # Returns
    ///
    /// The length of the remaining data in the input buffer.
    fn get_remaining_data_len(&self) -> usize {
        let data_len = self.data.len();
        let index = self.index;
        if data_len == 0 {
            0
        } else {
            data_len.saturating_sub(index)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::input_buf_vo::InputBufVOTrait;

    fn sample_body() -> BytesMut {
        // constructor_id=1, method_id=42, payload "hey"
        let mut b = BytesMut::new();
        b.extend_from_slice(&[1u8]);
        b.extend_from_slice(&42u16.to_le_bytes());
        b.extend_from_slice(b"hey");
        b
    }

    #[test]
    fn parses_fields_sequentially() {
        let addr: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let mut vo = InputBufVO::new(sample_body(), addr);
        assert_eq!(vo.get_input_addr(), Some(addr));
        assert_eq!(vo.get_constructor_id(), Some(1));
        assert_eq!(vo.get_method_id(), Some(42));
        assert_eq!(vo.next_str_with_len(3).as_deref(), Some("hey"));
        assert_eq!(vo.get_remaining_data_len(), 0);
    }

    #[test]
    fn reads_numbers_from_payload() {
        let mut body = BytesMut::new();
        body.extend_from_slice(&[1u8]);
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&7u64.to_le_bytes());
        body.extend_from_slice(&[9u8]);

        let mut vo = InputBufVO::new(body, "127.0.0.1:1".parse().unwrap());
        assert_eq!(vo.next_u64(), Some(7));
        assert_eq!(vo.next_u8(), Some(9));
        // Buffer exhausted: further reads return None instead of panicking.
        assert_eq!(vo.next_u64(), None);
        assert_eq!(vo.next_u8(), None);
    }

    #[test]
    fn short_buffers_return_none() {
        let mut vo = InputBufVO::new_without_socket_addr(BytesMut::from(&[1u8, 2][..]));
        assert_eq!(vo.get_constructor_id(), Some(1));
        assert_eq!(vo.get_method_id(), None, "method id needs 3 bytes");
        assert_eq!(vo.next_u64(), None);
        assert_eq!(vo.next_str_with_len(1), None);
    }

    #[test]
    fn oversized_string_len_is_rejected() {
        let mut vo = InputBufVO::new(sample_body(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(vo.next_str_with_len(u64::MAX), None);
    }

    #[test]
    fn get_all_bytes_strips_the_frame_prefix() {
        let vo = InputBufVO::new(sample_body(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(&vo.get_all_bytes()[..], b"hey");
        // Non-destructive: repeated reads return the same payload.
        assert_eq!(&vo.get_all_bytes()[..], b"hey");
    }

    #[test]
    fn empty_vo_behaves_gracefully() {
        let mut vo = InputBufVO::new_none();
        assert_eq!(vo.get_input_addr(), None);
        assert_eq!(vo.get_constructor_id(), None);
        assert_eq!(vo.get_method_id(), None);
        assert!(vo.get_all_bytes().is_empty());
        assert_eq!(vo.get_remaining_data_len(), 0);
    }

    #[test]
    fn without_socket_addr_has_no_addr() {
        let vo = InputBufVO::new_without_socket_addr(sample_body());
        assert_eq!(vo.get_input_addr(), None);
        assert_eq!(vo.get_remaining_data_len(), 3);
    }
}
