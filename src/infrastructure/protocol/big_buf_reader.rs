use bytes::{Buf, BytesMut};

use crate::const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE;
use crate::domain::model::message_validation::{MAX_MESSAGE_SIZE, validate_message_length};
use crate::{LynnError, Result};

// One maximum frame plus the next socket read, which may also contain another frame.
const MAX_BUFFER_SIZE: usize = MAX_MESSAGE_SIZE + 10 + DEFAULT_MAX_RECEIVE_BYTES_SIZE;

/// A bounded receive buffer shared by the client and server.
pub(crate) struct BigBufReader {
    data: BytesMut,
    message_header_mark: u16,
    message_tail_mark: u16,
    failed: bool,
}

impl BigBufReader {
    pub(crate) fn new(message_header_mark: u16, message_tail_mark: u16) -> Self {
        Self {
            data: BytesMut::with_capacity(DEFAULT_MAX_RECEIVE_BYTES_SIZE),
            message_header_mark,
            message_tail_mark,
            failed: false,
        }
    }

    fn reject(&mut self, error: LynnError) -> LynnError {
        self.failed = true;
        self.data = BytesMut::new();
        error
    }

    pub(crate) fn extend_from_slice(&mut self, buf: &[u8]) -> Result<()> {
        if self.failed {
            return Err(LynnError::protocol("receive parser is closed"));
        }
        if buf.len() > MAX_BUFFER_SIZE - self.data.len() {
            return Err(self.reject(LynnError::protocol("receive buffer limit exceeded")));
        }
        self.data.extend_from_slice(buf);
        Ok(())
    }

    /// Returns a payload, an incomplete frame, or a terminal protocol error.
    pub(crate) fn next_frame(&mut self) -> Result<Option<BytesMut>> {
        if self.failed {
            return Err(LynnError::protocol("receive parser is closed"));
        }
        if self.data.len() < 2 {
            return Ok(None);
        }
        if u16::from_le_bytes([self.data[0], self.data[1]]) != self.message_header_mark {
            return Err(self.reject(LynnError::protocol("invalid frame header")));
        }
        if self.data.len() < 10 {
            return Ok(None);
        }
        let length = u64::from_le_bytes(self.data[2..10].try_into().unwrap());
        let length = validate_message_length(length).map_err(|error| self.reject(error))?;
        if length < 5 {
            return Err(self.reject(LynnError::protocol(
                "frame must include metadata and trailer",
            )));
        }
        let frame_len = 10 + length;
        if self.data.len() < frame_len {
            return Ok(None);
        }
        if u16::from_le_bytes([self.data[frame_len - 2], self.data[frame_len - 1]])
            != self.message_tail_mark
        {
            return Err(self.reject(LynnError::protocol("invalid frame trailer")));
        }
        let payload = BytesMut::from(&self.data[10..frame_len - 2]);
        self.data.advance(frame_len);
        Ok(Some(payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: u16 = 0x1122;
    const TAIL: u16 = 0x3344;

    fn frame(constructor: u8, method: u16, body: &[u8]) -> BytesMut {
        let mut frame = BytesMut::new();
        frame.extend_from_slice(&HEADER.to_le_bytes());
        frame.extend_from_slice(&((body.len() + 5) as u64).to_le_bytes());
        frame.extend_from_slice(&[constructor]);
        frame.extend_from_slice(&method.to_le_bytes());
        frame.extend_from_slice(body);
        frame.extend_from_slice(&TAIL.to_le_bytes());
        frame
    }

    #[test]
    fn parses_single_and_coalesced_frames_at_every_split() {
        let first = frame(1, 9, b"hello");
        let second = frame(2, 0, b"");
        let input = [first.as_ref(), second.as_ref()].concat();
        for split in 0..=input.len() {
            let mut reader = BigBufReader::new(HEADER, TAIL);
            let mut received = Vec::new();
            for chunk in [&input[..split], &input[split..]] {
                reader.extend_from_slice(chunk).unwrap();
                while let Some(payload) = reader.next_frame().unwrap() {
                    received.push(payload);
                }
            }
            assert_eq!(
                received,
                [
                    BytesMut::from(&b"\x01\x09\x00hello"[..]),
                    BytesMut::from(&b"\x02\x00\x00"[..])
                ]
            );
            assert!(reader.data.is_empty());
        }
    }

    #[test]
    fn parses_byte_by_byte() {
        let input = frame(1, 5, b"payload");
        let mut reader = BigBufReader::new(HEADER, TAIL);
        for (index, byte) in input.iter().enumerate() {
            reader.extend_from_slice(&[*byte]).unwrap();
            let payload = reader.next_frame().unwrap();
            if index + 1 == input.len() {
                assert_eq!(payload.unwrap().as_ref(), b"\x01\x05\x00payload");
            } else {
                assert!(payload.is_none());
            }
        }
    }

    #[test]
    fn invalid_tail_does_not_retain_unbounded_input() {
        let mut invalid = frame(1, 1, b"");
        let end = invalid.len();
        invalid[end - 2..].fill(0);
        for split in 0..invalid.len() {
            let mut reader = BigBufReader::new(HEADER, TAIL);
            reader.extend_from_slice(&invalid[..split]).unwrap();
            assert!(reader.next_frame().unwrap().is_none());
            reader.extend_from_slice(&invalid[split..]).unwrap();
            assert!(reader.next_frame().is_err());
            for _ in 0..10241 {
                assert!(reader.extend_from_slice(&[0; 1024]).is_err());
            }
            assert_eq!(reader.data.len(), 0);
            assert_eq!(reader.data.capacity(), 0);
            assert!(reader.next_frame().is_err());
        }
    }

    #[test]
    fn extracted_small_frame_does_not_pin_large_receive_allocation() {
        let mut reader = BigBufReader::new(HEADER, TAIL);
        let mut input = frame(1, 1, &vec![7; MAX_MESSAGE_SIZE - 5]);
        input.extend_from_slice(&frame(2, 0, b""));
        reader.extend_from_slice(&input).unwrap();
        drop(reader.next_frame().unwrap().unwrap());
        let small = reader.next_frame().unwrap().unwrap();
        assert_eq!(small.len(), 3);
        assert_eq!(small.capacity(), 3);
    }

    #[test]
    fn invalid_second_frame_is_not_hidden_by_first_frame() {
        let mut input = frame(1, 1, b"ok");
        let mut invalid = frame(1, 2, b"bad");
        let end = invalid.len();
        invalid[end - 1] ^= 1;
        input.extend_from_slice(&invalid);
        let mut reader = BigBufReader::new(HEADER, TAIL);
        reader.extend_from_slice(&input).unwrap();
        assert_eq!(
            reader.next_frame().unwrap().unwrap().as_ref(),
            b"\x01\x01\x00ok"
        );
        assert!(reader.next_frame().is_err());
        assert!(reader.data.is_empty());
    }

    #[test]
    fn invalid_header_and_lengths_are_terminal() {
        for length in [0, 2, 3, 4, MAX_MESSAGE_SIZE as u64 + 1, u64::MAX] {
            let mut reader = BigBufReader::new(HEADER, TAIL);
            let mut input = HEADER.to_le_bytes().to_vec();
            input.extend_from_slice(&length.to_le_bytes());
            reader.extend_from_slice(&input).unwrap();
            assert!(reader.next_frame().is_err(), "length {length}");
            assert!(reader.data.is_empty());
        }
        let mut reader = BigBufReader::new(HEADER, TAIL);
        reader.extend_from_slice(&[0, 0]).unwrap();
        assert!(reader.next_frame().is_err());
        assert!(reader.extend_from_slice(&frame(1, 1, b"ok")).is_err());
    }

    #[test]
    fn maximum_frame_and_coalesced_next_read_are_accepted() {
        let mut input = frame(1, 1, &vec![7; MAX_MESSAGE_SIZE - 5]);
        input.extend_from_slice(&frame(2, 0, b""));
        let mut reader = BigBufReader::new(HEADER, TAIL);
        let mut lengths = Vec::new();
        for chunk in input.chunks(DEFAULT_MAX_RECEIVE_BYTES_SIZE) {
            reader.extend_from_slice(chunk).unwrap();
            assert!(reader.data.len() <= MAX_BUFFER_SIZE);
            while let Some(payload) = reader.next_frame().unwrap() {
                lengths.push(payload.len());
            }
        }
        assert_eq!(lengths, [MAX_MESSAGE_SIZE - 2, 3]);
    }

    #[test]
    fn buffer_limit_is_checked_before_allocation_even_without_parsing() {
        let mut reader = BigBufReader::new(HEADER, TAIL);
        reader.extend_from_slice(&vec![0; MAX_BUFFER_SIZE]).unwrap();
        assert!(reader.extend_from_slice(&[0]).is_err());
        assert_eq!(reader.data.capacity(), 0);
        assert!(
            reader
                .extend_from_slice(&vec![0; MAX_BUFFER_SIZE + 1])
                .is_err()
        );
        let mut fresh = BigBufReader::new(HEADER, TAIL);
        assert!(
            fresh
                .extend_from_slice(&vec![0; MAX_BUFFER_SIZE + 1])
                .is_err()
        );
        assert_eq!(fresh.data.capacity(), 0);
    }
}
