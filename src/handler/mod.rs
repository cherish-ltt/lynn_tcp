mod impl_for_context;

use std::{future::Future, marker::PhantomData, net::SocketAddr, pin::Pin, sync::Arc};

use crate::{
    app::ClientsStruct,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};

#[derive(Clone)]
pub(crate) struct HandlerContext {
    input_buf_vo: InputBufVO,
    clients_context: ClientsContext,
}

impl HandlerContext {
    pub(crate) fn new(input_buf_vo: InputBufVO, clients_context: ClientsContext) -> Self {
        HandlerContext {
            input_buf_vo,
            clients_context,
        }
    }
}

#[derive(Clone)]
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

    pub async fn get_all_clients_addrs(&self) -> Vec<SocketAddr> {
        let mut result = Vec::new();
        if let Some(clients) = &self.clients {
            let clients = clients.0.read().await;
            for client in clients.iter() {
                result.push(client.0.clone());
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

pub trait SystemParamState: Send + Sync + 'static {
    type Item: SystemParam<State = Self>;

    fn init() -> Self;

    fn get_param(state: &Self, context: &HandlerContext) -> Self::Item;
}

#[derive(Clone)]
pub struct FunctionSystem<F, Param>
where
    Param: SystemParam + 'static,
    F: SystemParamFunction<Param> + 'static,
{
    func: F,
    state: Param::State,
    _maker: PhantomData<Param>,
}

pub trait SystemParamFunction<Param>: Send + Sync + 'static {
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
