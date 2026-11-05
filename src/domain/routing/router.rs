use crate::domain::handler::{
    handler_system::{AsyncFunc, HandlerContext, IntoSystem},
    middleware::{Middleware, MiddlewareChain, Next},
    request_context::MiddlewareRequest,
};
use crate::domain::model::handler_result::HandlerResult;
use crate::infrastructure::metrics::metrics::METRICS;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::warn;

pub(crate) struct Route {
    handler: Arc<AsyncFunc>,
    pub(crate) middleware: Option<Box<[Arc<dyn Middleware>]>>,
}

impl Route {
    pub(crate) async fn run(
        self: Arc<Self>,
        context: HandlerContext,
    ) -> crate::Result<HandlerResult> {
        if self.middleware.is_none() {
            return Ok(self.run_handler(context).await);
        }
        Next {
            route: self,
            index: 0,
        }
        .run(MiddlewareRequest::new(context))
        .await
    }

    pub(crate) async fn run_handler(&self, context: HandlerContext) -> HandlerResult {
        let timer = METRICS.messages.processing_duration_seconds.start_timer();
        let result = self.handler.handler(context).await;
        timer.observe_duration();
        result
    }
}

pub(crate) struct LynnRouter(DashMap<u16, Arc<Route>>);

impl LynnRouter {
    pub(crate) fn new() -> Self {
        LynnRouter(DashMap::new())
    }

    pub(crate) fn add_router<Param>(&self, method_id: u16, handler: impl IntoSystem<Param>) {
        self.add_router_with_middleware(method_id, handler, MiddlewareChain::new());
    }

    pub(crate) fn add_router_with_middleware<Param>(
        &self,
        method_id: u16,
        handler: impl IntoSystem<Param>,
        chain: MiddlewareChain,
    ) {
        if self.0.contains_key(&method_id) {
            warn!(
                "Router - Duplicate method_id {} detected, existing handler will be overwritten",
                method_id
            );
        }
        self.0.insert(
            method_id,
            Arc::new(Route {
                handler: Arc::new(Box::new(handler.to_system())),
                middleware: (!chain.entries.is_empty()).then(|| chain.entries.into_boxed_slice()),
            }),
        );
    }

    pub(crate) fn freeze(&self, global: MiddlewareChain) {
        if global.entries.is_empty() {
            return;
        }
        for mut route in self.0.iter_mut() {
            let mut entries = global.entries.clone();
            if let Some(local) = &route.middleware {
                entries.extend(local.iter().cloned());
            }
            *route = Arc::new(Route {
                handler: route.handler.clone(),
                middleware: Some(entries.into_boxed_slice()),
            });
        }
    }

    pub(crate) fn get_handler_by_method_id(&self, method_id: &u16) -> Option<Arc<Route>> {
        self.0.get(method_id).map(|ref_| ref_.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::handler_result::HandlerResult;

    async fn noop() -> HandlerResult {
        HandlerResult::new_without_send()
    }

    #[test]
    fn register_and_lookup() {
        let router = LynnRouter::new();
        assert!(router.get_handler_by_method_id(&1).is_none());

        router.add_router(1, noop);
        assert!(router.get_handler_by_method_id(&1).is_some());
        assert!(router.get_handler_by_method_id(&2).is_none());
    }

    #[test]
    fn duplicate_registration_overwrites_without_panicking() {
        let router = LynnRouter::new();
        router.add_router(1, noop);
        router.add_router(1, noop); // triggers the duplicate warning path
        assert!(router.get_handler_by_method_id(&1).is_some());
    }

    #[test]
    fn empty_chain_freezes_without_a_middleware_allocation() {
        let router = LynnRouter::new();
        router.add_router(1, noop);
        let before = router.get_handler_by_method_id(&1).unwrap();
        router.freeze(MiddlewareChain::new());
        let after = router.get_handler_by_method_id(&1).unwrap();
        assert!(Arc::ptr_eq(&before, &after));
        assert!(after.middleware.is_none());
    }

    #[test]
    fn concurrent_registration_and_lookup() {
        let router = Arc::new(LynnRouter::new());
        let mut handles = Vec::new();
        for i in 0..8u16 {
            let router = router.clone();
            handles.push(std::thread::spawn(move || {
                router.add_router(i, noop);
                assert!(router.get_handler_by_method_id(&i).is_some());
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
