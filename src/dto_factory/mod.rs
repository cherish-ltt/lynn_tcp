mod msg_select;
mod router_handler;

pub mod input_dto {
    pub(crate) use super::msg_select::*;
    pub use super::router_handler::HandlerResult;
    pub(crate) use super::router_handler::IHandlerCombinedTrait;
}
