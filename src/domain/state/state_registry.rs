use std::{any::Any, sync::Arc};

use dashmap::DashMap;

/// A per-server, type-keyed container for shared state.
///
/// Handlers extract registered values through `AppState<T>` parameters; the
/// registry itself is shared with every request context via `Arc`.
pub struct StateRegistry {
    map: DashMap<std::any::TypeId, Arc<dyn Any + Send + Sync>>,
}

impl StateRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Stores (or replaces) the state value of type `T`.
    pub fn set<T: Send + Sync + 'static>(&self, value: T) {
        self.map
            .insert(std::any::TypeId::of::<T>(), Arc::new(value));
    }

    /// Stores (or replaces) an already shared state value of type `T`.
    pub fn set_arc<T: Send + Sync + 'static>(&self, value: Arc<T>) {
        self.map.insert(
            std::any::TypeId::of::<T>(),
            value as Arc<dyn Any + Send + Sync>,
        );
    }

    /// Returns a clone of the shared state value of type `T`, if registered.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.map
            .get(&std::any::TypeId::of::<T>())?
            .value()
            .clone()
            .downcast::<T>()
            .ok()
    }

    /// Returns `true` when a state value of type `T` is registered.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.map.contains_key(&std::any::TypeId::of::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Db {
        url: String,
    }

    #[derive(Debug)]
    struct Config {
        pool_size: usize,
    }

    #[test]
    fn set_and_get_roundtrip() {
        let registry = StateRegistry::new();
        assert!(!registry.contains::<Db>());

        registry.set(Db {
            url: "postgres://localhost/app".to_string(),
        });
        assert!(registry.contains::<Db>());

        let db = registry.get::<Db>().expect("state must be present");
        assert_eq!(db.url, "postgres://localhost/app");
    }

    #[test]
    fn shared_arc_is_refcounted_not_copied() {
        let registry = StateRegistry::new();
        registry.set(Config { pool_size: 8 });

        let a = registry.get::<Config>().unwrap();
        let b = registry.get::<Config>().unwrap();
        assert!(
            Arc::ptr_eq(&a, &b),
            "get must return clones of the same Arc"
        );
        assert_eq!(a.pool_size, 8);
    }

    #[test]
    fn multiple_types_coexist() {
        let registry = StateRegistry::new();
        registry.set(Db {
            url: "sqlite::memory:".to_string(),
        });
        registry.set(Config { pool_size: 4 });

        assert_eq!(registry.get::<Config>().unwrap().pool_size, 4);
        assert_eq!(registry.get::<Db>().unwrap().url, "sqlite::memory:");
    }

    #[test]
    fn set_overwrites_previous_value() {
        let registry = StateRegistry::new();
        registry.set(Config { pool_size: 1 });
        registry.set(Config { pool_size: 2 });

        assert_eq!(registry.get::<Config>().unwrap().pool_size, 2);
    }

    #[test]
    fn missing_type_returns_none() {
        let registry = StateRegistry::new();
        registry.set(Config { pool_size: 4 });
        assert!(registry.get::<Db>().is_none());
    }

    #[test]
    fn set_arc_shares_the_provided_arc() {
        let registry = StateRegistry::new();
        let shared = Arc::new(Db {
            url: "mem".to_string(),
        });
        registry.set_arc(shared.clone());

        let got = registry.get::<Db>().unwrap();
        assert!(Arc::ptr_eq(&shared, &got));
    }
}
