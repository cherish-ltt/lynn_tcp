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
        self.map
            .insert(method_id, Arc::new(Box::new(handler.to_system())));
    }

    pub(crate) fn get_handler_by_method_id(&self, method_id: &u16) -> Option<Arc<AsyncFunc>> {
        self.map.get(method_id).map(|ref_| ref_.clone())
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
