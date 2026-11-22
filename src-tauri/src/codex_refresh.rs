pub use polydeck_core::codex_refresh::production_executor;
use polydeck_core::codex_refresh::{Executor, RefreshOutcome};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Notify};

const REFRESH_EVENT: &str = "codex-daemon-refresh";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RefreshState {
    Idle,
    Refreshing,
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshStatus {
    pub generation: u64,
    pub state: RefreshState,
    pub message: Option<String>,
}

pub type StatusSink = Arc<dyn Fn(RefreshStatus) + Send + Sync>;

struct Inner {
    generation: u64,
    pending: bool,
    status: RefreshStatus,
}

pub struct CodexRefreshCoordinator {
    inner: Mutex<Inner>,
    notify: Notify,
    executor: Executor,
    sink: RwLock<StatusSink>,
}

impl CodexRefreshCoordinator {
    pub fn new(executor: Executor, sink: StatusSink) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                generation: 0,
                pending: false,
                status: RefreshStatus {
                    generation: 0,
                    state: RefreshState::Idle,
                    message: None,
                },
            }),
            notify: Notify::new(),
            executor,
            sink: RwLock::new(sink),
        })
    }

    pub fn set_sink(&self, sink: StatusSink) {
        *self.sink.write().expect("Codex refresh sink lock poisoned") = sink;
    }

    fn emit(&self, status: RefreshStatus) {
        if let Ok(sink) = self.sink.read() {
            (sink)(status);
        }
    }

    pub fn start(self: &Arc<Self>) {
        let coordinator = Arc::clone(self);
        tauri::async_runtime::spawn(async move { coordinator.run().await });
    }
    pub async fn request(&self) -> u64 {
        let status = {
            let mut inner = self.inner.lock().await;
            inner.generation += 1;
            inner.pending = true;
            inner.status = RefreshStatus {
                generation: inner.generation,
                state: RefreshState::Refreshing,
                message: None,
            };
            inner.status.clone()
        };
        self.emit(status);
        self.notify.notify_one();
        let inner = self.inner.lock().await;
        inner.generation
    }

    pub async fn status(&self) -> RefreshStatus {
        self.inner.lock().await.status.clone()
    }

    async fn run(self: Arc<Self>) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            let generation = {
                let mut inner = self.inner.lock().await;
                if inner.pending {
                    inner.pending = false;
                    Some(inner.generation)
                } else {
                    None
                }
            };

            let Some(generation) = generation else {
                notified.await;
                continue;
            };

            let outcome = (self.executor)().await;
            let mut emit = None;
            {
                let mut inner = self.inner.lock().await;
                if inner.generation == generation {
                    let (state, message) = match outcome {
                        RefreshOutcome::Success => (RefreshState::Success, None),
                        RefreshOutcome::Failed => (
                            RefreshState::Failed,
                            Some("Codex daemon 刷新失败，配置已写入但可能尚未生效".into()),
                        ),
                        RefreshOutcome::Skipped => (
                            RefreshState::Skipped,
                            Some("未检测到 Codex daemon，已跳过刷新".into()),
                        ),
                    };
                    inner.status = RefreshStatus {
                        generation,
                        state,
                        message,
                    };
                    emit = Some(inner.status.clone());
                }
            }
            if let Some(status) = emit {
                self.emit(status);
            }
        }
    }
}

pub fn event_sink(app: AppHandle) -> StatusSink {
    Arc::new(move |status| {
        let _ = app.emit(REFRESH_EVENT, status);
    })
}

pub fn event_sink_placeholder() -> StatusSink {
    Arc::new(|_| {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{mpsc, oneshot};
    use tokio::time::{timeout, Duration};

    fn sink() -> (StatusSink, mpsc::UnboundedReceiver<RefreshStatus>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let sink = Arc::new(move |status: RefreshStatus| {
            if status.state != RefreshState::Refreshing {
                let _ = tx.send(status);
            }
        });
        (sink, rx)
    }

    async fn next_result(statuses: &mut mpsc::UnboundedReceiver<RefreshStatus>) -> RefreshStatus {
        timeout(Duration::from_secs(5), statuses.recv())
            .await
            .expect("refresh worker did not report a result")
            .expect("refresh status channel closed")
    }

    #[test]
    fn refresh_worker_starts_from_synchronous_setup() {
        let finished = Arc::new(Notify::new());
        let finished_for_sink = Arc::clone(&finished);
        let coordinator = CodexRefreshCoordinator::new(
            Arc::new(|| Box::pin(async { RefreshOutcome::Success })),
            Arc::new(move |status| {
                if status.state == RefreshState::Success {
                    finished_for_sink.notify_one();
                }
            }),
        );
        coordinator.start();

        tauri::async_runtime::block_on(async {
            coordinator.request().await;
            tokio::time::timeout(Duration::from_secs(2), finished.notified())
                .await
                .expect("refresh worker did not complete");
        });
    }

    #[tokio::test]
    async fn requests_are_serial_and_converge_to_latest_generation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let started_tx = Arc::new(std::sync::Mutex::new(Some(started_tx)));
        let release_rx = Arc::new(std::sync::Mutex::new(Some(release_rx)));
        let started_for_executor = Arc::clone(&started_tx);
        let release_for_executor = Arc::clone(&release_rx);
        let calls_for_executor = Arc::clone(&calls);
        let executor: Executor = Arc::new(move || {
            let call = calls_for_executor.fetch_add(1, Ordering::SeqCst);
            let started = Arc::clone(&started_for_executor);
            let release = Arc::clone(&release_for_executor);
            Box::pin(async move {
                if call == 0 {
                    let _ = started
                        .lock()
                        .expect("test sender lock poisoned")
                        .take()
                        .expect("sender already taken")
                        .send(());
                    let receiver = release
                        .lock()
                        .expect("test receiver lock poisoned")
                        .take()
                        .expect("receiver already taken");
                    let _ = receiver.await;
                }
                RefreshOutcome::Success
            })
        });
        let (sink, mut statuses) = sink();
        let coordinator = CodexRefreshCoordinator::new(executor, sink);
        coordinator.start();

        let first = coordinator.request().await;
        started_rx.await.unwrap();
        let second = coordinator.request().await;
        release_tx.send(()).unwrap();
        let completed = next_result(&mut statuses).await;

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(coordinator.status().await.state, RefreshState::Success);
        assert_eq!(coordinator.status().await.generation, 2);
        assert_eq!(completed.generation, 2);
        assert_eq!(completed.state, RefreshState::Success);
    }

    #[tokio::test]
    async fn failed_refresh_does_not_block_a_following_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_executor = Arc::clone(&calls);
        let executor: Executor = Arc::new(move || {
            let call = calls_for_executor.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if call == 0 {
                    RefreshOutcome::Failed
                } else {
                    RefreshOutcome::Success
                }
            })
        });
        let (sink, mut statuses) = sink();
        let coordinator = CodexRefreshCoordinator::new(executor, sink);
        coordinator.start();
        let first = coordinator.request().await;
        let failed = next_result(&mut statuses).await;
        assert_eq!(failed.generation, first);
        assert_eq!(failed.state, RefreshState::Failed);
        let second = coordinator.request().await;
        let succeeded = next_result(&mut statuses).await;
        assert_eq!(succeeded.generation, second);
        assert_eq!(succeeded.state, RefreshState::Success);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
