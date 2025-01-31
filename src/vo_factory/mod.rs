use bytes::BytesMut;

mod big_buf_reader;
mod input_buf_vo;

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

pub mod input_vo {
    pub use super::input_buf_vo::InputBufVO;
}

pub(crate) mod big_buf {
    pub(crate) use super::big_buf_reader::*;
}
