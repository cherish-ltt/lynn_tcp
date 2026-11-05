use std::future::Future;

use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone)]
pub(crate) struct ServerTasks {
    cancelled: watch::Receiver<()>,
}

impl ServerTasks {
    pub(crate) fn new() -> (watch::Sender<()>, Self) {
        let (owner, cancelled) = watch::channel(());
        (owner, Self { cancelled })
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.has_changed().is_err()
    }

    pub(crate) fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) -> JoinHandle<()> {
        let mut cancelled = self.cancelled.clone();
        tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.changed() => {},
                _ = task => {},
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn owner_drop_cancels_existing_and_late_tasks() {
        let (owner, tasks) = ServerTasks::new();
        let state = Arc::new(());
        let weak = Arc::downgrade(&state);
        let pending = tasks.spawn(async move {
            let _state = state;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        drop(owner);
        pending.await.unwrap();
        assert!(weak.upgrade().is_none());
        tasks
            .spawn(async { panic!("cancelled task must not start") })
            .await
            .unwrap();
    }
}
