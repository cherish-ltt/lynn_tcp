use std::{
    any::{Any, TypeId},
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
};

use crate::domain::state::app_state::AppState;

use super::handler_system::HandlerContext;

type RequestData = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

/// Read-only metadata and typed data belonging to one matched request.
///
/// Missing data returns `None`; server-wide state remains in `AppState<T>`.
#[derive(Clone)]
pub struct RequestContext {
    method_id: u16,
    peer_addr: SocketAddr,
    data: Option<Arc<RequestData>>,
}

impl RequestContext {
    pub(crate) fn new(method_id: u16, peer_addr: SocketAddr) -> Self {
        Self {
            method_id,
            peer_addr,
            data: None,
        }
    }

    pub fn method_id(&self) -> u16 {
        self.method_id
    }

    /// The transport peer address, not an authenticated user identity.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.data
            .as_ref()?
            .get(&TypeId::of::<T>())?
            .clone()
            .downcast()
            .ok()
    }
}

/// An owned request passed through middleware without changing its route or payload.
pub struct MiddlewareRequest {
    context: HandlerContext,
    data: Option<RequestData>,
}

impl MiddlewareRequest {
    pub(crate) fn new(context: HandlerContext) -> Self {
        Self {
            context,
            data: None,
        }
    }

    pub fn method_id(&self) -> u16 {
        self.context.request_context.method_id()
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.context.request_context.peer_addr()
    }

    /// Borrows the original business payload without advancing the handler's cursor.
    pub fn payload(&self) -> &[u8] {
        self.context.input_buf_vo.payload()
    }

    pub fn app_state<T: Send + Sync + 'static>(&self) -> Option<AppState<T>> {
        self.context.states.get::<T>().map(AppState::new)
    }

    /// Replaces data of the same type and returns the previous value, if any.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<Arc<T>> {
        self.data
            .get_or_insert_with(HashMap::new)
            .insert(TypeId::of::<T>(), Arc::new(value))?
            .downcast()
            .ok()
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.data
            .as_ref()?
            .get(&TypeId::of::<T>())?
            .clone()
            .downcast()
            .ok()
    }

    pub(crate) fn into_context(mut self) -> HandlerContext {
        self.context.request_context.data = self.data.map(Arc::new);
        self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        handler::handler_system::ClientsContext, model::input_buf_vo::InputBufVO,
        state::state_registry::StateRegistry,
    };

    fn context() -> HandlerContext {
        let addr = "127.0.0.1:1234".parse().unwrap();
        HandlerContext::new(
            InputBufVO::new([1, 7, 0, 42].as_slice().into(), addr),
            ClientsContext::new_none(),
            Arc::new(StateRegistry::new()),
            RequestContext::new(7, addr),
        )
    }

    #[test]
    fn empty_request_never_creates_a_data_map_or_shared_snapshot() {
        let context = context();
        assert!(context.request_context.data.is_none());
        let request = MiddlewareRequest::new(context);
        assert!(request.data.is_none());
        assert_eq!(request.payload(), &[42]);
        assert!(request.get::<u32>().is_none());
        assert!(request.into_context().request_context.data.is_none());
    }

    #[test]
    fn inserted_data_is_replaced_and_frozen_only_for_its_request() {
        let mut first = MiddlewareRequest::new(context());
        let second = MiddlewareRequest::new(context());
        assert!(first.insert(10_u32).is_none());
        assert_eq!(*first.insert(20_u32).unwrap(), 10);
        let value = first.get::<u32>().unwrap();
        let frozen = first.into_context().request_context;
        assert!(Arc::ptr_eq(&value, &frozen.get::<u32>().unwrap()));
        assert!(second.get::<u32>().is_none());
        assert!(frozen.get::<u64>().is_none());
    }
}
