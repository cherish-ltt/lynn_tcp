use std::{ops::Deref, sync::Arc};

/// An extractor for server-wide shared state, modeled after axum's
/// `State<T>`.
///
/// Register the value once via `LynnServer::with_state(value)`, then declare
/// it as a handler parameter — the framework injects an `Arc<T>` snapshot of
/// the shared value into every request:
///
/// ```rust,no_run
/// use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*, lynn_state::AppState};
///
/// struct UserRepo { /* e.g. a sea_orm::DatabaseConnection */ }
/// impl UserRepo {
///     fn find_user(&self, id: u64) -> String { format!("user-{id}") }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     LynnServer::new()
///         .await
///         .with_state(UserRepo {})
///         .add_router(1, find_user_handler)
///         .start()
///         .await;
/// }
///
/// async fn find_user_handler(repo: AppState<UserRepo>, input: InputBufVO) -> HandlerResult {
///     // `repo` dereferences to `&UserRepo`.
///     let name = repo.find_user(1);
///     let addr = input.get_input_addr().unwrap();
///     HandlerResult::new_with_send(1, name.into(), vec![addr])
/// }
/// ```
///
/// Multiple state types can coexist: register each type once with
/// `with_state`, and handlers may take several `AppState<T>` parameters.
/// `AppState<T>` dereferences to `T`, so state methods can be called
/// directly. Extraction panics with a descriptive message when the value was
/// never registered — configure states before `start()`.
pub struct AppState<T> {
    value: Arc<T>,
}

impl<T> Clone for AppState<T> {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
        }
    }
}

impl<T> AppState<T> {
    pub(crate) fn new(value: Arc<T>) -> Self {
        Self { value }
    }

    /// Consumes the extractor, returning the shared `Arc<T>`.
    pub fn into_inner(self) -> Arc<T> {
        self.value
    }
}

impl<T> Deref for AppState<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
