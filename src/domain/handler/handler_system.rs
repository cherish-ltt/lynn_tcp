use std::{future::Future, marker::PhantomData, net::SocketAddr, pin::Pin, sync::Arc};

use crate::domain::model::handler_result::HandlerResult;
use crate::domain::model::input_buf_vo::InputBufVO;
use crate::domain::model::lynn_user::ClientsStruct;
use crate::domain::state::state_registry::StateRegistry;

#[derive(Clone)]
pub(crate) struct HandlerContext {
    pub(crate) input_buf_vo: InputBufVO,
    pub(crate) clients_context: ClientsContext,
    pub(crate) states: Arc<StateRegistry>,
}

impl HandlerContext {
    pub(crate) fn new(
        input_buf_vo: InputBufVO,
        clients_context: ClientsContext,
        states: Arc<StateRegistry>,
    ) -> Self {
        HandlerContext {
            input_buf_vo,
            clients_context,
            states,
        }
    }
}

#[derive(Clone)]
#[cfg(feature = "server")]
pub struct ClientsContext {
    clients: Option<ClientsStruct>,
}

impl ClientsContext {
    pub(crate) fn new(clients: ClientsStruct) -> Self {
        Self {
            clients: Some(clients),
        }
    }

    pub(crate) fn new_none() -> Self {
        Self { clients: None }
    }

    #[cfg(feature = "server")]
    pub async fn get_all_clients_addrs(&self) -> Vec<SocketAddr> {
        let mut result = Vec::new();
        if let Some(clients) = &self.clients {
            let clients = &clients.0;
            for client in clients.iter() {
                result.push(*client.key());
            }
        }
        result
    }
}

pub(crate) trait IHandler: Send + Sync + 'static {
    fn handler(
        &self,
        context: HandlerContext,
    ) -> Pin<Box<dyn Future<Output = HandlerResult> + Send + 'static>>;
}

pub(crate) type AsyncFunc = Box<dyn IHandler>;

impl<Param: SystemParam, F: SystemParamFunction<Param>> IHandler for FunctionSystem<F, Param> {
    fn handler(
        &self,
        context: HandlerContext,
    ) -> Pin<Box<dyn Future<Output = HandlerResult> + Send + 'static>> {
        let param = Param::State::get_param(&self.state, &context);
        Box::pin(self.func.run(param))
    }
}

pub(crate) trait SystemParam: Send + Sync + 'static {
    type State: SystemParamState<Item = Self>;
}

pub(crate) trait SystemParamState: Send + Sync + 'static {
    type Item: SystemParam<State = Self>;

    fn init() -> Self;

    fn get_param(state: &Self, context: &HandlerContext) -> Self::Item;
}

#[derive(Clone)]
pub(crate) struct FunctionSystem<F, Param>
where
    Param: SystemParam + 'static,
    F: SystemParamFunction<Param> + 'static,
{
    func: F,
    state: Param::State,
    _maker: PhantomData<Param>,
}

pub(crate) trait SystemParamFunction<Param>: Send + Sync + 'static {
    fn run(&self, params: Param) -> Pin<Box<dyn Future<Output = HandlerResult> + Send + 'static>>;
}

pub(crate) trait IntoSystem<Param>: Sized {
    type System: IHandler;
    fn to_system(self) -> Self::System;
}

impl<Param: SystemParam + 'static, F: SystemParamFunction<Param> + 'static> IntoSystem<Param>
    for F
{
    type System = FunctionSystem<F, Param>;

    fn to_system(self) -> Self::System {
        FunctionSystem {
            func: self,
            state: Param::State::init(),
            _maker: PhantomData,
        }
    }
}
