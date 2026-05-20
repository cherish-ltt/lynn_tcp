use crate::domain::handler::handler_system::{AsyncFunc, IntoSystem};
use dashmap::DashMap;
use std::sync::Arc;
use tracing::warn;

pub(crate) struct LynnRouter {
    pub(crate) map: DashMap<u16, Arc<AsyncFunc>>,
}

impl LynnRouter {
    pub(crate) fn new() -> Self {
        LynnRouter {
            map: DashMap::new(),
        }
    }

    pub(crate) fn add_router<Param>(&self, method_id: u16, handler: impl IntoSystem<Param>) {
        if self.map.contains_key(&method_id) {
            warn!(
                "Router - Duplicate method_id {} detected, existing handler will be overwritten",
                method_id
            );
        }
        self.map.insert(method_id, Arc::new(Box::new(handler.to_system())));
    }

    pub(crate) fn get_handler_by_method_id(&self, method_id: &u16) -> Option<Arc<AsyncFunc>> {
        self.map.get(method_id).map(|ref_| ref_.clone())
    }
}
