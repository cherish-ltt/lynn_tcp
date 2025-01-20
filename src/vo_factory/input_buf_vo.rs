use std::net::SocketAddr;

use super::InputBufVOTrait;

/// A struct representing a value object for an input buffer.
///
/// This struct is used to encapsulate data received from a network connection.
/// It includes the raw data as a byte vector, an index to track the current
/// position in the data, and the address from which the data was received.
#[derive(Clone)]
#[cfg(any(feature = "server", feature = "client"))]
pub struct InputBufVO {
    /// The raw data received from the network as a byte vector.
    data: Vec<u8>,
    /// The current index in the data vector, used for reading data in chunks.
    index: usize,
    /// The address from which the data was received.
    input_addr: SocketAddr,
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
    pub(crate) fn new(buf: Vec<u8>, input_addr: SocketAddr) -> Self {
        Self {
            data: buf,
            index: 3,
            input_addr,
        }
    }

    /// Retrieves the input address.
    ///
    /// This method returns the address from which the input data was received.
    ///
    /// # Returns
    ///
    /// The input address.
    pub fn get_input_addr(&self) -> SocketAddr {
        self.input_addr
    }
}

impl InputBufVOTrait for InputBufVO {
    fn is_structure_complete(&mut self) -> bool {
        true
    }
    fn is_standard_header(&mut self) -> bool {
        true
    }
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
            return None;
        } else {
            let bytes = &mut self.data[0..1];
            match bytes.try_into() {
                Ok(value) => {
                    return Some(u8::from_be_bytes(value));
                }
                Err(_) => {
                    return None;
                }
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
            return None;
        } else {
            let bytes = &mut self.data[1..3];
            match bytes.try_into() {
                Ok(value) => {
                    return Some(u16::from_be_bytes(value));
                }
                Err(_) => {
                    return None;
                }
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
            return None;
        } else {
            let bytes = &mut self.data[self.index..self.index + 8];
            match bytes.try_into() {
                Ok(value) => {
                    self.index = self.index + 8;
                    return Some(u64::from_be_bytes(value));
                }
                Err(_) => {
                    return None;
                }
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
            return None;
        } else {
            let bytes = &mut self.data[self.index..self.index + 1];
            match bytes.try_into() {
                Ok(value) => {
                    self.index = self.index + 1;
                    return Some(u8::from_be_bytes(value));
                }
                Err(_) => {
                    return None;
                }
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
        let len = len as usize;
        let length = self.data.len();
        if length < self.index + len {
            return None;
        } else {
            let bytes = &mut self.data[self.index..self.index + len].to_vec();
            self.index = self.index + len;
            Some(String::from_utf8_lossy(bytes).to_string())
        }
    }
}
