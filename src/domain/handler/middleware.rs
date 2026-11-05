use std::{future::Future, pin::Pin, sync::Arc};

use crate::{
    Result,
    domain::{model::handler_result::HandlerResult, routing::router::Route},
};

use super::request_context::MiddlewareRequest;

type MiddlewareFuture = Pin<Box<dyn Future<Output = Result<HandlerResult>> + Send + 'static>>;

pub(crate) trait Middleware: Send + Sync + 'static {
    fn call(&self, request: MiddlewareRequest, next: Next) -> MiddlewareFuture;
}

impl<F, Fut> Middleware for F
where
    F: Fn(MiddlewareRequest, Next) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<HandlerResult>> + Send + 'static,
{
    fn call(&self, request: MiddlewareRequest, next: Next) -> MiddlewareFuture {
        Box::pin(self(request, next))
    }
}

/// An ordered route-local chain. Earlier registrations wrap later ones.
#[derive(Default)]
pub struct MiddlewareChain {
    pub(crate) entries: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with<F, Fut>(mut self, middleware: F) -> Self
    where
        F: Fn(MiddlewareRequest, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<HandlerResult>> + Send + 'static,
    {
        self.entries.push(Arc::new(middleware));
        self
    }
}

/// The remaining middleware and handler, consumable at most once.
///
/// A second use cannot compile:
/// ```compile_fail
/// use lynn_tcp::lynn_middleware::{MiddlewareRequest, Next};
/// async fn twice(first: MiddlewareRequest, second: MiddlewareRequest, next: Next) {
///     let _ = next.run(first).await;
///     let _ = next.run(second).await;
/// }
/// ```
/// Nor can the continuation be cloned:
/// ```compile_fail
/// use lynn_tcp::lynn_middleware::Next;
/// fn duplicate(next: Next) { let _ = next.clone(); }
/// ```
pub struct Next {
    pub(crate) route: Arc<Route>,
    pub(crate) index: usize,
}

impl Next {
    /// Runs the remaining chain. The result has not been queued or sent yet.
    pub async fn run(mut self, request: MiddlewareRequest) -> Result<HandlerResult> {
        let middleware = self
            .route
            .middleware
            .as_ref()
            .and_then(|entries| entries.get(self.index))
            .cloned();
        match middleware {
            Some(middleware) => {
                self.index += 1;
                middleware.call(request, self).await
            },
            None => Ok(self.route.run_handler(request.into_context()).await),
        }
    }
}
