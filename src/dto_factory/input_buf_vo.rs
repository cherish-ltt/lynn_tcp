use std::net::SocketAddr;

/// A struct representing a value object for an input buffer.
///
/// This struct is used to encapsulate data received from a network connection.
/// It includes the raw data as a byte vector, an index to track the current
/// position in the data, and the address from which the data was received.
#[derive(Clone)]
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
            index: 16,
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

    /// Retrieves the constructor ID from the input buffer.
    ///
    /// This method extracts a `u64` value from the first 8 bytes of the input
    /// buffer and interprets it as the constructor ID. If the extraction fails,
    /// it returns a default value of 0.
    ///
    /// # Returns
    ///
    /// The constructor ID from the input buffer.
    pub fn get_constructor_id(&self) -> u64 {
        let bytes = &self.data[0..8];
        u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))
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
    pub fn get_method_id(&self) -> u64 {
        let bytes = &self.data[8..16];
        u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))
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
    pub fn next_u64(&mut self) -> u64 {
        let bytes = &self.data[self.index..self.index + 8];
        self.index = self.index + 8;
        u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))
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
    pub fn next_u8(&mut self) -> u8 {
        let bytes = &self.data[self.index..self.index + 1];
        self.index = self.index + 1;
        u8::from_be_bytes(bytes.try_into().unwrap_or([0; 1]))
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
    pub fn next_str_with_len(&mut self, len: usize) -> String {
        let bytes = &self.data[self.index..self.index + len].to_vec();
        self.index = self.index + len;
        String::from_utf8_lossy(bytes).to_string()
    }
}