mod impl_for_context;

use std::{future::Future, marker::PhantomData, net::SocketAddr, pin::Pin, sync::Arc};

use crate::{
    app::ClientsStruct,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};

// pub mod handler_trait {
//     pub use super::FunctionSystem;
//     pub use super::SystemParam;
//     pub use super::SystemParamFunction;
//     pub use super::SystemParamState;
// }

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
    // 定义一个关联类型System，它必须实现IService trait
    type System: IHandler;
    // 定义一个方法to_system，用于将当前类型转换为System类型
    fn to_system(self) -> Self::System;
}

// 为所有满足条件的F类型实现IntoSystem trait
impl<Param: SystemParam + 'static, F: SystemParamFunction<Param> + 'static> IntoSystem<Param>
    for F
{
    // 指定System类型为FunctionSystem<F, Param>
    type System = FunctionSystem<F, Param>;

    // 实现to_system方法，返回一个FunctionSystem实例
    fn to_system(self) -> Self::System {
        FunctionSystem {
            // 将self赋值给func字段
            func: self,
            // 调用Param的State的init方法初始化state字段
            state: Param::State::init(),
            // 使用PhantomData占位符
            _maker: PhantomData,
        }
    }
}
