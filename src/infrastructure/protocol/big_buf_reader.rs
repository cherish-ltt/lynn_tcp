use bytes::BytesMut;

use crate::const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE;
use crate::domain::model::message_validation::validate_message_length;

/// A struct for reading large buffers.
pub(crate) struct BigBufReader {
    /// The data buffer.
    data: BytesMut,
    /// The remaining data buffer.
    remaining_data: Option<BytesMut>,
    /// The target length of the data buffer.
    target_len: Option<usize>,
    /// The message header mark.
    message_header_mark: u16,
    /// The message tail mark.
    message_tail_mark: u16,
}

impl BigBufReader {
    /// Creates a new `BigBufReader` instance.
    ///
    /// # Parameters
    ///
    /// * `message_header_mark` - The message header mark.
    /// * `message_tail_mark` - The message tail mark.
    ///
    /// # Returns
    ///
    /// A new `BigBufReader` instance.
    pub(crate) fn new(message_header_mark: u16, message_tail_mark: u16) -> Self {
        Self {
            data: BytesMut::with_capacity(DEFAULT_MAX_RECEIVE_BYTES_SIZE),
            remaining_data: None,
            target_len: None,
            message_header_mark,
            message_tail_mark,
        }
    }

    /// Forces the buffer to be cleared.
    pub(crate) fn forced_clear(&mut self) {
        self.data.clear();
        self.remaining_data = None;
        self.target_len = None;
    }

    /// Checks the data in the buffer.
    pub(crate) fn check_data(&mut self) {
        if let Some(target_len) = self.target_len {
            if self.data.len() > 10 + target_len && self.data.len() > 12 {
                let data = if target_len != 0 && target_len >= 2 {
                    self.data.split_off(10 + target_len)
                } else {
                    self.data.split_off(12)
                };
                self.data.clear();
                self.target_len = None;
                self.extend_from_slice(&data);
            } else {
                self.data.clear();
                self.target_len = None;
                if let Some(buf) = &self.remaining_data {
                    let buf = buf.clone();
                    self.remaining_data = None;
                    self.extend_from_slice(&buf);
                }
            }
        } else {
            if !self.data.is_empty() {
                self.data.clear();
            }
            if let Some(buf) = &self.remaining_data {
                let buf = buf.clone();
                self.remaining_data = None;
                self.target_len = None;
                self.extend_from_slice(&buf);
            }
        }
    }

    /// Checks if the buffer is empty.
    ///
    /// # Returns
    ///
    /// `true` if the buffer is empty, `false` otherwise.
    pub(crate) fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Gets the length of the next buffer to be extended.
    ///
    /// # Returns
    ///
    /// The length of the next buffer to be extended, or `None` if the buffer is already complete.
    pub(crate) fn get_next_extend_buf_len(&mut self) -> Option<usize> {
        if let Some(target_len) = self.target_len {
            let len = self.data.len();
            if len < target_len + 10 {
                return Some(target_len + 10 - len);
            }
        }
        None
    }

    /// Checks if the buffer is complete.
    ///
    /// # Returns
    ///
    /// `true` if the buffer is complete, `false` otherwise.
    pub(crate) fn is_complete(&mut self) -> bool {
        if let Some(target_len) = self.target_len {
            if !self.is_empty()
                && self.data.len() >= 10 + target_len as usize
                && self.data.len() >= 12
                && u16::from_le_bytes([
                    self.data[10 + target_len - 2],
                    self.data[10 + target_len - 1],
                ]) == self.message_tail_mark
            {
                return true;
            }
        }
        false
    }

    /// Gets the data from the buffer.
    ///
    /// # Returns
    ///
    /// The data from the buffer.
    ///
    /// # Panics
    ///
    /// Panics if `target_len` is not set (i.e., the message hasn't been properly initialized).
    pub(crate) fn get_data(&mut self) -> BytesMut {
        let target_len = self
            .target_len
            .expect("target_len should be set before get_data is called");
        let bytes = BytesMut::from(&self.data[10..target_len + 8]);
        self.check_data();
        bytes
    }

    /// Extends the buffer with the given slice.
    ///
    /// # Parameters
    ///
    /// * `buf` - The slice to extend the buffer with.
    pub(crate) fn extend_from_slice(&mut self, buf: &[u8]) {
        let buf_len = buf.len();
        if !self.is_complete() {
            let next_len = self.get_next_extend_buf_len();
            match next_len {
                Some(len) if len < buf_len => {
                    // Extend with part of the buffer
                    self.data.extend_from_slice(&buf[0..len]);
                    let bytes_mut = &buf[len..buf_len];
                    if self.remaining_data.is_none() {
                        self.remaining_data = Some(BytesMut::from(bytes_mut));
                    } else {
                        if let Some(source_bytes_mut) = &self.remaining_data {
                            let mut data = source_bytes_mut.clone();
                            data.extend_from_slice(bytes_mut);
                            self.remaining_data = Some(data)
                        }
                    }
                },
                // Either next_len is None or next_len >= buf_len, extend with full buffer
                _ => {
                    self.data.extend_from_slice(buf);
                },
            }
            if self.target_len.is_none() {
                let data_len = self.data.len();
                if data_len >= 2 {
                    let header_mark = u16::from_le_bytes([self.data[0], self.data[1]]);
                    if header_mark == self.message_header_mark {
                        if data_len >= 10 {
                            let msg_len = u64::from_le_bytes([
                                self.data[2],
                                self.data[3],
                                self.data[4],
                                self.data[5],
                                self.data[6],
                                self.data[7],
                                self.data[8],
                                self.data[9],
                            ]);

                            // Validate message length to prevent memory exhaustion attacks
                            match validate_message_length(msg_len) {
                                Ok(validated_len) => {
                                    self.target_len = Some(validated_len);
                                },
                                Err(e) => {
                                    tracing::warn!("Invalid message length: {}", e);
                                    self.forced_clear();
                                },
                            }
                        }
                    } else {
                        self.forced_clear();
                    }
                }
            }
        } else {
            self.remaining_data = Some(BytesMut::from(buf));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: u16 = 0x1122;
    const TAIL: u16 = 0x3344;

    fn frame(constructor: u8, method: u16, body: &[u8]) -> BytesMut {
        let msg_len = (1 + 2 + body.len() + 2) as u64;
        let mut f = BytesMut::new();
        f.extend_from_slice(&HEADER.to_le_bytes());
        f.extend_from_slice(&msg_len.to_le_bytes());
        f.extend_from_slice(&[constructor]);
        f.extend_from_slice(&method.to_le_bytes());
        f.extend_from_slice(body);
        f.extend_from_slice(&TAIL.to_le_bytes());
        f
    }

    #[test]
    fn parses_a_single_frame() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        r.extend_from_slice(&frame(1, 9, b"hello"));
        assert!(r.is_complete());
        assert!(!r.is_empty());
        let data = r.get_data();
        assert_eq!(
            &data[..],
            &[1, 9, 0, b'h', b'e', b'l', b'l', b'o'],
            "payload must keep constructor id, method id and body"
        );
        // After extraction the reader is empty and ready for the next frame.
        assert!(!r.is_complete());
        assert!(r.is_empty());
    }

    #[test]
    fn parses_byte_by_byte() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        let f = frame(1, 5, b"payload");
        for byte in f.iter() {
            r.extend_from_slice(&[*byte]);
        }
        assert!(r.is_complete());
        let data = r.get_data();
        assert_eq!(
            &data[..],
            &[1, 5, 0, b'p', b'a', b'y', b'l', b'o', b'a', b'd']
        );
    }

    #[test]
    fn parses_two_frames_in_one_feed() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        let mut both = frame(1, 1, b"a");
        both.extend_from_slice(&frame(2, 0, &[])); // heart-shaped frame
        r.extend_from_slice(&both);

        let first = r.get_data();
        assert_eq!(&first[..], &[1, 1, 0, b'a']);
        assert!(r.is_complete(), "the second frame must already be buffered");
        let second = r.get_data();
        assert_eq!(&second[..], &[2, 0, 0]);
        assert!(!r.is_complete());
        assert!(r.is_empty());
    }

    #[test]
    fn buffers_partial_frames() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        let f = frame(1, 5, b"payload");
        r.extend_from_slice(&f[..10]);
        assert!(!r.is_complete());
        assert_eq!(
            r.get_next_extend_buf_len(),
            Some(f.len() - 10),
            "must report exactly how many bytes are still missing"
        );
        r.extend_from_slice(&f[10..]);
        assert!(r.is_complete());
    }

    #[test]
    fn wrong_header_clears_and_recovers() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        let mut garbage = frame(1, 1, b"x");
        garbage[0] = 0x99;
        garbage[1] = 0x99;
        r.extend_from_slice(&garbage);
        assert!(r.is_empty(), "frames with a wrong header must be dropped");

        r.extend_from_slice(&frame(1, 1, b"ok"));
        assert!(r.is_complete(), "reader must recover after invalid data");
    }

    #[test]
    fn oversized_length_clears_buffer() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        let mut malicious = BytesMut::new();
        malicious.extend_from_slice(&HEADER.to_le_bytes());
        malicious.extend_from_slice(&u64::MAX.to_le_bytes());
        r.extend_from_slice(&malicious);
        assert!(r.is_empty(), "invalid lengths must not allocate");
    }

    #[test]
    fn forced_clear_resets_everything() {
        let mut r = BigBufReader::new(HEADER, TAIL);
        r.extend_from_slice(&frame(1, 1, b"abc"));
        assert!(r.is_complete());
        r.forced_clear();
        assert!(r.is_empty());
        assert!(!r.is_complete());
        assert_eq!(r.get_next_extend_buf_len(), None);
    }
}
