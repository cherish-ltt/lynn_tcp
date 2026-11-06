use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytes::Bytes;
use tokio::sync::{
    mpsc::{WeakSender, error::TrySendError},
    watch,
};

use crate::{
    Result, application::server::server_tasks::ServerTasks,
    infrastructure::metrics::metrics::METRICS,
};

use super::{
    handler_result::HandlerResult,
    lynn_user::{LynnUser, LynnUserSignal},
};

/// Opaque connection identity, distinct even across server instances.
///
/// Equality is meaningful within this process; this is not an authenticated user ID.
#[derive(Clone)]
pub struct SessionId(Arc<()>);

impl PartialEq for SessionId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SessionId {}

impl Hash for SessionId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SessionId")
            .field(&Arc::as_ptr(&self.0))
            .finish()
    }
}

/// Observes final local transport release without keeping the session alive.
#[derive(Clone, Debug)]
pub struct SessionObserver {
    id: SessionId,
    closed: watch::Receiver<()>,
    #[cfg(feature = "kcp")]
    driver: Option<watch::Receiver<Option<String>>>,
}

impl SessionObserver {
    pub(crate) fn new() -> (watch::Sender<()>, Self) {
        let (completion, closed) = watch::channel(());
        (
            completion,
            Self {
                id: SessionId(Arc::new(())),
                closed,
                #[cfg(feature = "kcp")]
                driver: None,
            },
        )
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn is_closed(&self) -> bool {
        let closed = self.closed.has_changed().is_err();
        #[cfg(feature = "kcp")]
        let closed = closed
            && self
                .driver
                .as_ref()
                .is_none_or(|driver| driver.has_changed().is_err());
        closed
    }

    /// Waits for both transport halves and any KCP driver to be released, including
    /// writer shutdown after normal EOF. Concurrent, repeated and cancelled waits work.
    ///
    /// Do not await this inside a handler or middleware holding this session's
    /// request permit: normal EOF waits for that request to finish first.
    /// This does not acknowledge delivery or completion of all handler side effects.
    pub async fn closed(&self) {
        let mut closed = self.closed.clone();
        while closed.changed().await.is_ok() {}
        #[cfg(feature = "kcp")]
        if let Some(mut driver) = self.driver.clone() {
            while driver.changed().await.is_ok() {}
        }
    }

    #[cfg(feature = "kcp")]
    pub(crate) fn with_driver(mut self, driver: watch::Receiver<Option<String>>) -> Self {
        self.driver = Some(driver);
        self
    }
}

/// A non-waiting enqueue result, not a delivery acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionSendOutcome {
    Enqueued,
    Full,
    Closed,
}

/// Sends to the original request's connection, never to a replacement at its address.
///
/// Clones retain neither the server owner, transport nor application state. Granting
/// this capability is an application authorization decision; pushes bypass inbound
/// middleware. Use [`Self::observer`] to grant observation without sending capability.
#[derive(Clone)]
pub struct SessionHandle {
    observer: SessionObserver,
    sender: WeakSender<LynnUserSignal>,
    removed: watch::Receiver<bool>,
    tasks: ServerTasks,
    marks: (u16, u16),
}

impl SessionHandle {
    pub(crate) fn new(
        user: &LynnUser,
        observer: SessionObserver,
        removed: watch::Receiver<bool>,
        tasks: ServerTasks,
        marks: (u16, u16),
    ) -> Self {
        Self {
            observer,
            sender: user.response_sender().downgrade(),
            removed,
            tasks,
            marks,
        }
    }

    pub fn id(&self) -> &SessionId {
        self.observer.id()
    }

    pub fn observer(&self) -> SessionObserver {
        self.observer.clone()
    }

    /// Attempts one bounded enqueue without waiting for capacity.
    ///
    /// Concurrent closure may discard an accepted frame. Once closure has been
    /// observed, a valid-size send returns `Closed`. Normal handler replies retain
    /// their existing waiting backpressure. Server draining also returns `Closed`.
    ///
    /// # Errors
    /// Returns `LynnError::Protocol` for payloads exceeding 10 MiB minus 5 bytes.
    pub fn try_send(&self, method_id: u16, payload: Bytes) -> Result<SessionSendOutcome> {
        let mut result = HandlerResult::new_with_send(method_id, payload, vec![])?;
        let admission = self.tasks.try_admit();
        let outcome = if admission.is_none()
            || self.observer.is_closed()
            || *self.removed.borrow()
            || self.removed.has_changed().is_err()
        {
            SessionSendOutcome::Closed
        } else if let Some(sender) = self.sender.upgrade() {
            match sender.try_reserve() {
                Ok(permit) => {
                    result.set_marks(self.marks.0, self.marks.1);
                    permit.send(LynnUserSignal::SendResponse(
                        result.get_response_data().expect("send result has a frame"),
                    ));
                    SessionSendOutcome::Enqueued
                },
                Err(TrySendError::Full(())) => SessionSendOutcome::Full,
                Err(TrySendError::Closed(())) => SessionSendOutcome::Closed,
            }
        } else {
            SessionSendOutcome::Closed
        };
        drop(admission);
        if outcome != SessionSendOutcome::Enqueued {
            METRICS.messages.dropped_total.inc();
        }
        Ok(outcome)
    }

    #[cfg(test)]
    pub(crate) fn closed_for_test() -> Self {
        let (_, observer) = SessionObserver::new();
        let (_, removed) = watch::channel(false);
        let (_, tasks) = ServerTasks::new();
        let (sender, _) = tokio::sync::mpsc::channel(1);
        Self {
            observer,
            sender: sender.downgrade(),
            removed,
            tasks,
            marks: (9177, 7719),
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
