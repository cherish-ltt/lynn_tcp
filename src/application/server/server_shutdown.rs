use tokio::sync::watch;

/// The result of an explicit, bounded server shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownOutcome {
    /// Accepted work, queued responses and transport shutdown finished successfully.
    /// This is not a business delivery acknowledgement.
    Drained,
    /// A handler, response enqueue or transport operation failed during draining.
    /// The underlying error is logged; all server tasks have been released.
    Incomplete,
    /// The shared grace period expired; cancellable tasks were forcibly released.
    TimedOut,
}

#[derive(Clone)]
pub(crate) struct Activity(watch::Sender<usize>);

pub(crate) struct ActivityGuard(watch::Sender<usize>);

impl Activity {
    pub(crate) fn new() -> Self {
        Self(watch::channel(0).0)
    }

    pub(crate) fn enter(&self) -> ActivityGuard {
        self.0.send_modify(|active| *active += 1);
        ActivityGuard(self.0.clone())
    }

    pub(crate) async fn wait(&self) {
        let mut active = self.0.subscribe();
        let _ = active.wait_for(|active| *active == 0).await;
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.0.send_modify(|active| *active -= 1);
    }
}
