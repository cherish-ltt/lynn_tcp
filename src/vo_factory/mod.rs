mod input_buf_vo;

pub trait InputBufVOTrait {
    fn get_constructor_id(&mut self) -> Option<u8>;
    fn get_method_id(&mut self) -> Option<u16>;
    fn next_u64(&mut self) -> Option<u64>;
    fn next_u8(&mut self) -> Option<u8>;
    fn next_str_with_len(&mut self, len: u64) -> Option<String>;
}

pub mod input_vo {
    pub use super::input_buf_vo::InputBufVO;
}
