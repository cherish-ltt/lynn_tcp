use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::mpsc::error::TrySendError;

use super::server_tasks::ServerTasks;
use crate::{
    LynnError, Result,
    domain::model::{
        handler_result::HandlerResult,
        lynn_user::{ClientsStructType, LynnUser, LynnUserSignal},
    },
    infrastructure::metrics::metrics::METRICS,
};

/// Per-connection queue outcomes; acceptance does not guarantee delivery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BroadcastReport {
    pub enqueued: usize,
    pub full: usize,
    pub closed: usize,
}

/// An instance-scoped broadcast capability that does not keep the server alive.
#[derive(Clone)]
pub struct BroadcastHandle {
    clients: Weak<DashMap<std::net::SocketAddr, LynnUser>>,
    started: Arc<AtomicBool>,
    tasks: ServerTasks,
    header: u16,
    tail: u16,
}

impl BroadcastHandle {
    pub(crate) fn new(
        clients: &ClientsStructType,
        started: Arc<AtomicBool>,
        tasks: ServerTasks,
        marks: (u16, u16),
    ) -> Self {
        Self {
            clients: Arc::downgrade(clients),
            started,
            tasks,
            header: marks.0,
            tail: marks.1,
        }
    }

    /// Snapshots current connections and attempts one non-waiting enqueue per target.
    ///
    /// Concurrent connections/disconnections may or may not appear in the snapshot.
    /// Full or closed queues do not prevent attempts for other targets. An in-flight
    /// call can race with server cancellation; accepted frames may then be discarded.
    ///
    /// # Errors
    /// Returns `LynnError::Server` before startup, during draining or after cancellation, and
    /// `LynnError::Protocol` for payloads exceeding 10 MiB minus 5 bytes.
    pub fn broadcast(&self, method_id: u16, payload: Bytes) -> Result<BroadcastReport> {
        let mut result = HandlerResult::new_with_send(method_id, payload, vec![])?;
        let _admission = self
            .tasks
            .try_admit()
            .ok_or_else(|| LynnError::server("broadcast server is draining or closed"))?;
        if !self.started.load(Ordering::Acquire) {
            return Err(LynnError::server("broadcast server has not started"));
        }
        let clients = self
            .clients
            .upgrade()
            .ok_or_else(|| LynnError::server("broadcast server is closed"))?;
        let targets: Vec<_> = clients.iter().map(|user| user.response_sender()).collect();
        if targets.is_empty() {
            return Ok(BroadcastReport::default());
        }
        result.set_marks(self.header, self.tail);
        let frame = result.get_response_data().expect("send result has a frame");
        let mut report = BroadcastReport::default();
        for sender in targets {
            // Reserve first so a full queue never copies a potentially large frame.
            match sender.try_reserve() {
                Ok(permit) => {
                    permit.send(LynnUserSignal::SendResponse(frame.clone()));
                    report.enqueued += 1;
                },
                Err(TrySendError::Full(())) => report.full += 1,
                Err(TrySendError::Closed(())) => report.closed += 1,
            }
        }
        METRICS
            .messages
            .dropped_total
            .inc_by((report.full + report.closed) as f64);
        Ok(report)
    }
}
