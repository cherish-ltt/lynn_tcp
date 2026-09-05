use std::{any::type_name, marker::PhantomData, sync::Arc};

use crate::domain::model::input_buf_vo::InputBufVO;
use crate::domain::state::app_state::AppState;

use super::handler_system::{ClientsContext, HandlerContext, SystemParam, SystemParamState};

impl SystemParam for InputBufVO {
    type State = InputBufVO;
}

impl SystemParamState for InputBufVO {
    type Item = InputBufVO;

    fn init() -> Self {
        InputBufVO::new_none()
    }

    fn get_param(_state: &Self, context: &HandlerContext) -> Self::Item {
        context.input_buf_vo.clone()
    }
}

impl SystemParam for ClientsContext {
    type State = ClientsContext;
}

impl SystemParamState for ClientsContext {
    type Item = ClientsContext;

    fn init() -> Self {
        ClientsContext::new_none()
    }

    fn get_param(_state: &Self, context: &HandlerContext) -> Self::Item {
        context.clients_context.clone()
    }
}

/// Zero-sized per-system state for the `AppState<T>` extractor. The actual
/// value lives in the server's `StateRegistry` and is resolved per request,
/// so registration order relative to `add_router` does not matter.
pub(crate) struct AppStateParamState<T> {
    _marker: PhantomData<T>,
}

impl<T: Send + Sync + 'static> SystemParam for AppState<T> {
    type State = AppStateParamState<T>;
}

impl<T: Send + Sync + 'static> SystemParamState for AppStateParamState<T> {
    type Item = AppState<T>;

    fn init() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    fn get_param(_state: &Self, context: &HandlerContext) -> Self::Item {
        match context.states.get::<T>() {
            Some(value) => AppState::new(value),
            None => panic!(
                "AppState<{}> is not configured: call `LynnServer::with_state()` before `start()`",
                type_name::<T>()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::state::state_registry::StateRegistry;

    #[derive(Debug, PartialEq)]
    struct Repo {
        prefix: String,
    }

    type RepoParamState = <AppState<Repo> as SystemParam>::State;

    fn context_with(repo: Option<Repo>) -> HandlerContext {
        let states = Arc::new(StateRegistry::new());
        if let Some(repo) = repo {
            states.set(repo);
        }
        HandlerContext::new(InputBufVO::new_none(), ClientsContext::new_none(), states)
    }

    fn extract(context: &HandlerContext) -> AppState<Repo> {
        let param_state = RepoParamState::init();
        SystemParamState::get_param(&param_state, context)
    }

    #[test]
    fn app_state_extracts_the_registered_value() {
        let context = context_with(Some(Repo {
            prefix: "user-".into(),
        }));

        let repo = extract(&context).into_inner();
        assert_eq!(repo.prefix, "user-");
    }

    #[test]
    fn app_state_derefs_to_the_inner_value() {
        let context = context_with(Some(Repo {
            prefix: "cfg".into(),
        }));
        let state = extract(&context);
        assert_eq!(state.prefix, "cfg");
    }

    #[test]
    #[should_panic(expected = "AppState<")]
    fn app_state_panics_when_not_registered() {
        let context = context_with(None);
        let _ = extract(&context);
    }

    #[test]
    fn app_state_param_state_is_zero_sized() {
        // Init must succeed even when the registry is still empty: values are
        // resolved per request, so registration order does not matter.
        let _param_state = RepoParamState::init();
        let context = context_with(None);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = extract(&context);
        }))
        .is_err();
        assert!(
            panicked,
            "unregistered type must still fail at request time"
        );
    }
}
