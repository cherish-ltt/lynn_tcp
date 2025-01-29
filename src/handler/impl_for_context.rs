use crate::lynn_tcp_dependents::InputBufVO;

use super::{ClientsContext, HandlerContext, SystemParam, SystemParamState};

impl SystemParam for InputBufVO {
    type State = InputBufVO;
}

impl SystemParamState for InputBufVO {
    type Item = InputBufVO;

    fn init() -> Self {
        InputBufVO::new_none()
    }

    fn get_param(state: &Self, context: &HandlerContext) -> Self::Item {
        context.input_buf_vo.clone()
    }
}

impl SystemParam for ClientsContext {
    type State = ClientsContext;
}

impl SystemParamState for ClientsContext {
    type Item = ClientsContext;

    fn init() -> Self {
        ClientsContext::new_none()
    }

    fn get_param(state: &Self, context: &HandlerContext) -> Self::Item {
        context.clients_context.clone()
    }
}
