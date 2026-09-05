//! Global state (dependency injection) for server handlers.
//!
//! Mirrors axum's `State<T>`: users register any `Send + Sync + 'static`
//! value once via `LynnServer::with_state`, then handlers declare
//! `AppState<T>` parameters and receive an `Arc<T>` automatically.
//!
//! A [`StateRegistry`] is keyed by `TypeId`, so a single server can host
//! several independent states (e.g. a database handle plus configuration).

pub(crate) mod app_state;
pub mod state_registry;

pub(crate) use app_state::AppState;
pub use state_registry::StateRegistry;
