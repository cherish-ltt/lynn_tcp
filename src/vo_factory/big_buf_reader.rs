use bytes::{Buf, BytesMut};

pub(crate) struct BigBufReader {
    data: BytesMut,
    remaining_data: Option<Vec<u8>>,
    target_len: Option<usize>,
    message_header_mark: u16,
    message_tail_mark: u16,
}

impl BigBufReader {
    pub(crate) fn new(message_header_mark: u16, message_tail_mark: u16) -> Self {
        Self {
            data: BytesMut::with_capacity(0),
            remaining_data: None,
            target_len: None,
            message_header_mark,
            message_tail_mark,
        }
    }

    pub(crate) fn forced_clear(&mut self) {
        self.data.clear();
        self.remaining_data = None;
        self.target_len = None;
    }

    pub(crate) fn check_data(&mut self) {
        if let Some(target_len) = self.target_len {
            if self.data.len() > 10 + target_len && self.data.len() > 12 {
                let mut data;
                if target_len != 0 && target_len >= 2 {
                    data = self.data.split_off(10 + target_len);
                } else {
                    data = self.data.split_off(12);
                }
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
                self.extend_from_slice(&buf);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub(crate) fn get_next_extend_buf_len(&mut self) -> Option<usize> {
        if let Some(target_len) = self.target_len {
            let len = self.data.len();
            if len < target_len + 10 {
                return Some(target_len + 10 - len);
            }
        }
        None
    }

    pub(crate) fn is_complete(&mut self) -> bool {
        if let Some(target_len) = self.target_len {
            if !self.is_empty()
                && self.data.len() >= 10 + target_len.try_into().unwrap_or(0)
                && self.data.len() >= 12
                && u16::from_be_bytes([
                    self.data[10 + target_len - 2],
                    self.data[10 + target_len - 1],
                ]) == self.message_tail_mark
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn get_data(&mut self) -> Vec<u8> {
        let mut result = self.data[10..self.target_len.unwrap() - 2].to_vec();
        self.check_data();
        result
    }

    pub(crate) fn extend_from_slice(&mut self, buf: &[u8]) {
        let buf_len = buf.len();
        if !self.is_complete() {
            let next_len = self.get_next_extend_buf_len();
            if next_len.is_none() || next_len.unwrap() >= buf_len {
                self.data.extend_from_slice(buf);
            } else {
                self.data.extend_from_slice(&buf[0..next_len.unwrap()]);
                let bytes_mut = &buf[next_len.unwrap()..buf_len];
                if self.remaining_data.is_none() {
                    self.remaining_data = Some(bytes_mut.to_vec());
                } else {
                    if let Some(source_bytes_mut) = &self.remaining_data {
                        let mut data = source_bytes_mut.clone();
                        data.extend_from_slice(&bytes_mut);
                        self.remaining_data = Some(data)
                    }
                }
            }
            if self.target_len.is_none() {
                let data_len = self.data.len();
                if data_len >= 2 {
                    let header_mark =
                        u16::from_be_bytes([self.data[0].clone(), self.data[1].clone()]);
                    if header_mark == self.message_header_mark {
                        if data_len >= 10 {
                            let msg_len = u64::from_be_bytes([
                                self.data[2].clone(),
                                self.data[3].clone(),
                                self.data[4].clone(),
                                self.data[5].clone(),
                                self.data[6].clone(),
                                self.data[7].clone(),
                                self.data[8].clone(),
                                self.data[9].clone(),
                            ]);
                            self.target_len = Some(msg_len.try_into().unwrap_or(0));
                        }
                    } else {
                        self.forced_clear();
                    }
                }
            }
        } else {
            self.remaining_data = Some(buf.to_vec());
        }
    }
}
