use crate::{app::AsyncFunc, handler::IntoSystem};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub(super) struct LynnRouter {
    pub(crate) map: *mut HashMap<u16, Arc<AsyncFunc>>,
    lock: Mutex<()>,
}

unsafe impl Send for LynnRouter {}
unsafe impl Sync for LynnRouter {}

impl LynnRouter {
    pub(super) fn new() -> Self {
        let map = HashMap::new();
        LynnRouter {
            map: Box::into_raw(Box::new(map)),
            lock: Mutex::new(()),
        }
    }

    pub(super) fn add_router<Param>(&self, method_id: u16, handler: impl IntoSystem<Param>) {
        if let Ok(_mutex) = self.lock.lock() {
            if let Some(map) = unsafe { self.map.as_mut() } {
                map.insert(method_id, Arc::new(Box::new(handler.to_system())));
            }
        }
    }

    pub(crate) fn get_handler_by_method_id(&self, method_id: &u16) -> Option<Arc<AsyncFunc>> {
        if let Some(map) = unsafe { self.map.as_ref() } {
            if map.contains_key(method_id) {
                return Some(map.get(method_id).unwrap().clone());
            }
        }
        None
    }
}
