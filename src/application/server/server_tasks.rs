use std::{
    future::Future,
    sync::{
        Arc, RwLock, RwLockReadGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::{sync::watch, task::JoinHandle};

use super::server_shutdown::{Activity, ActivityGuard};

#[derive(Clone)]
pub(crate) struct ServerTasks(Arc<TaskState>);

struct TaskState {
    cancelled: watch::Receiver<()>,
    accepting: RwLock<bool>,
    draining: watch::Sender<bool>,
    requests: Activity,
    connections: Activity,
    active: Activity,
    failed: AtomicBool,
}

pub(crate) struct RequestWork {
    _active: ActivityGuard,
    tasks: ServerTasks,
    completed: bool,
}

impl RequestWork {
    pub(crate) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for RequestWork {
    fn drop(&mut self) {
        if !self.completed {
            self.tasks.record_failure();
        }
    }
}

impl ServerTasks {
    pub(crate) fn new() -> (watch::Sender<()>, Self) {
        let (owner, cancelled) = watch::channel(());
        (
            owner,
            Self(Arc::new(TaskState {
                cancelled,
                accepting: RwLock::new(true),
                draining: watch::channel(false).0,
                requests: Activity::new(),
                connections: Activity::new(),
                active: Activity::new(),
                failed: AtomicBool::new(false),
            })),
        )
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.cancelled.has_changed().unwrap_or(true)
    }

    pub(crate) fn try_admit(&self) -> Option<RwLockReadGuard<'_, bool>> {
        let admission = self.0.accepting.read().expect("admission lock poisoned");
        (*admission && !self.is_cancelled()).then_some(admission)
    }

    pub(crate) fn begin_draining(&self) {
        let mut accepting = self.0.accepting.write().expect("admission lock poisoned");
        *accepting = false;
        self.0.draining.send_replace(true);
    }

    pub(crate) fn is_draining(&self) -> bool {
        *self.0.draining.borrow()
    }

    pub(crate) async fn draining(&self) {
        let mut draining = self.0.draining.subscribe();
        let _ = draining.wait_for(|draining| *draining).await;
    }

    pub(crate) fn request(&self) -> RequestWork {
        RequestWork {
            _active: self.0.requests.enter(),
            tasks: self.clone(),
            completed: false,
        }
    }

    pub(crate) fn connection(&self) -> ActivityGuard {
        self.0.connections.enter()
    }

    pub(crate) async fn requests_finished(&self) {
        self.0.requests.wait().await;
    }

    pub(crate) async fn drained(&self) {
        self.0.requests.wait().await;
        self.0.connections.wait().await;
    }

    pub(crate) async fn finished(&self) {
        self.0.active.wait().await;
    }

    pub(crate) fn record_failure(&self) {
        if self.is_draining() {
            self.0.failed.store(true, Ordering::Release);
        }
    }

    pub(crate) fn has_failed(&self) -> bool {
        self.0.failed.load(Ordering::Acquire)
    }

    pub(crate) fn track<T: Send + 'static>(
        &self,
        task: impl Future<Output = T> + Send + 'static,
    ) -> JoinHandle<T> {
        // Tuple fields drop in order, even when cancellation precedes the first poll.
        let tracked = (task, self.0.active.enter());
        tokio::spawn(async move {
            let task_and_guard = tracked;
            let (task, _active) = task_and_guard;
            task.await
        })
    }

    pub(crate) fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) -> JoinHandle<()> {
        let mut cancelled = self.0.cancelled.clone();
        self.track(async move {
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
    use std::{
        pin::Pin,
        sync::Arc,
        task::{Context, Poll, Waker},
        time::Duration,
    };

    struct ResourceTask {
        tasks: ServerTasks,
        polled: Arc<AtomicBool>,
        destroyed: Arc<AtomicBool>,
        finished_before_drop: Arc<AtomicBool>,
        ready: bool,
        panic: bool,
    }

    impl Future for ResourceTask {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            self.polled.store(true, Ordering::Release);
            assert!(!self.panic, "intentional tracked future panic");
            if self.ready {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    impl Drop for ResourceTask {
        fn drop(&mut self) {
            let mut finished = std::pin::pin!(self.tasks.finished());
            let mut context = Context::from_waker(Waker::noop());
            self.finished_before_drop.store(
                finished.as_mut().poll(&mut context).is_ready(),
                Ordering::Release,
            );
            self.destroyed.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn completion_follows_resource_drop_for_never_polled_and_pending_tasks() {
        for poll_first in [false, true] {
            let (_owner, tasks) = ServerTasks::new();
            let polled = Arc::new(AtomicBool::new(false));
            let destroyed = Arc::new(AtomicBool::new(false));
            let finished_before_drop = Arc::new(AtomicBool::new(false));
            let task = tasks.track(ResourceTask {
                tasks: tasks.clone(),
                polled: polled.clone(),
                destroyed: destroyed.clone(),
                finished_before_drop: finished_before_drop.clone(),
                ready: false,
                panic: false,
            });
            if poll_first {
                tokio::task::yield_now().await;
            }
            assert_eq!(polled.load(Ordering::Acquire), poll_first);
            task.abort();
            tokio::time::timeout(Duration::from_secs(1), tasks.finished())
                .await
                .unwrap();
            assert!(task.await.unwrap_err().is_cancelled());
            assert!(destroyed.load(Ordering::Acquire));
            assert!(
                !finished_before_drop.load(Ordering::Acquire),
                "completion was observable before the task's resource destructor (polled={poll_first})"
            );
        }
    }

    #[tokio::test]
    async fn completion_follows_resource_drop_for_a_completed_task() {
        let (_owner, tasks) = ServerTasks::new();
        let polled = Arc::new(AtomicBool::new(false));
        let destroyed = Arc::new(AtomicBool::new(false));
        let finished_before_drop = Arc::new(AtomicBool::new(false));
        let task = tasks.track(ResourceTask {
            tasks: tasks.clone(),
            polled: polled.clone(),
            destroyed: destroyed.clone(),
            finished_before_drop: finished_before_drop.clone(),
            ready: true,
            panic: false,
        });
        tokio::time::timeout(Duration::from_secs(1), tasks.finished())
            .await
            .unwrap();
        task.await.unwrap();
        assert!(polled.load(Ordering::Acquire));
        assert!(destroyed.load(Ordering::Acquire));
        assert!(!finished_before_drop.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn completion_follows_resource_drop_for_a_panicking_task() {
        let (_owner, tasks) = ServerTasks::new();
        let destroyed = Arc::new(AtomicBool::new(false));
        let finished_before_drop = Arc::new(AtomicBool::new(false));
        let task = tasks.track(ResourceTask {
            tasks: tasks.clone(),
            polled: Arc::new(AtomicBool::new(false)),
            destroyed: destroyed.clone(),
            finished_before_drop: finished_before_drop.clone(),
            ready: false,
            panic: true,
        });
        tokio::time::timeout(Duration::from_secs(1), tasks.finished())
            .await
            .unwrap();
        assert!(task.await.unwrap_err().is_panic());
        assert!(destroyed.load(Ordering::Acquire));
        assert!(!finished_before_drop.load(Ordering::Acquire));
    }

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

    #[test]
    fn stop_boundary_waits_for_in_flight_admission_and_prevents_later_requests() {
        let (_owner, tasks) = ServerTasks::new();
        let before = tasks.try_admit().unwrap();
        let stopped = tasks.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            stopped.begin_draining();
            finished_tx.send(()).unwrap();
        });
        entered_rx.recv().unwrap();
        assert!(
            finished_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        let request = tasks.request();
        drop(before);
        finished_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(tasks.try_admit().is_none());
        request.complete();
        thread.join().unwrap();
    }
}
