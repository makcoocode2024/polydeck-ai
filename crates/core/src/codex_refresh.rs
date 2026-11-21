use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    Success,
    Failed,
    Skipped,
}

pub type ExecutorFuture = Pin<Box<dyn Future<Output = RefreshOutcome> + Send>>;
pub type Executor = Arc<dyn Fn() -> ExecutorFuture + Send + Sync>;
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
    sink: StatusSink,
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
            sink,
        })
    }

    pub fn start(self: &Arc<Self>) {
        let coordinator = Arc::clone(self);
        tokio::spawn(async move { coordinator.run().await });
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
        (self.sink)(status);
        self.notify.notify_one();
        let inner = self.inner.lock().await;
        inner.generation
    }

    pub async fn status(&self) -> RefreshStatus {
        self.inner.lock().await.status.clone()
    }

    async fn run(self: Arc<Self>) {
        loop {
            self.notify.notified().await;
            loop {
                let generation = {
                    let mut inner = self.inner.lock().await;
                    if !inner.pending {
                        break;
                    }
                    inner.pending = false;
                    inner.generation
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
                    (self.sink)(status);
                }
            }
        }
    }
}

pub fn production_executor() -> Executor {
    Arc::new(|| Box::pin(async { refresh_codex_daemon().await }))
}

async fn refresh_codex_daemon() -> RefreshOutcome {
    let Some(home) = crate::user_home_dir() else {
        return RefreshOutcome::Skipped;
    };
    let control_socket = home
        .join(".codex")
        .join("app-server-control")
        .join("app-server-control.sock");
    if !control_socket.exists() {
        return RefreshOutcome::Skipped;
    }

    let mut command = if cfg!(windows) {
        let mut command = tokio::process::Command::new("cmd.exe");
        command.args(["/C", "codex", "app-server", "daemon", "restart"]);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        if let (Some(path), Some(appdata)) = (std::env::var_os("PATH"), std::env::var_os("APPDATA"))
        {
            let npm_bin = std::path::PathBuf::from(appdata).join("npm");
            let mut entries: Vec<std::path::PathBuf> = std::env::split_paths(&path).collect();
            if npm_bin.is_dir() && !entries.iter().any(|entry| entry == &npm_bin) {
                entries.insert(0, npm_bin);
            }
            if let Ok(joined) = std::env::join_paths(entries) {
                command.env("PATH", joined);
            }
        }
        command
    } else {
        let mut command = tokio::process::Command::new("codex");
        command.args(["app-server", "daemon", "restart"]);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    match tokio::time::timeout(Duration::from_secs(20), command.status()).await {
        Ok(Ok(status)) if status.success() => RefreshOutcome::Success,
        Ok(_) | Err(_) => RefreshOutcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;
    use tokio::time::{sleep, Duration};

    fn sink() -> (StatusSink, Arc<Mutex<Vec<RefreshStatus>>>) {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let target = Arc::clone(&statuses);
        let sink: StatusSink = Arc::new(move |status| {
            let target = Arc::clone(&target);
            tokio::spawn(async move { target.lock().await.push(status) });
        });
        (sink, statuses)
    }

    #[tokio::test]
    async fn requests_are_serial_and_converge_to_latest_generation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let started_tx = Arc::new(Mutex::new(Some(started_tx)));
        let release_rx = Arc::new(Mutex::new(Some(release_rx)));
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
                        .await
                        .take()
                        .expect("sender already taken")
                        .send(());
                    let receiver = release.lock().await.take().expect("receiver already taken");
                    let _ = receiver.await;
                }
                RefreshOutcome::Success
            })
        });
        let (sink, statuses) = sink();
        let coordinator = CodexRefreshCoordinator::new(executor, sink);
        coordinator.start();

        let first = coordinator.request().await;
        started_rx.await.unwrap();
        let second = coordinator.request().await;
        release_tx.send(()).unwrap();
        sleep(Duration::from_millis(20)).await;

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(coordinator.status().await.state, RefreshState::Success);
        assert_eq!(coordinator.status().await.generation, 2);
        assert!(statuses
            .lock()
            .await
            .iter()
            .any(|s| s.generation == 2 && s.state == RefreshState::Success));
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
        let (sink, _) = sink();
        let coordinator = CodexRefreshCoordinator::new(executor, sink);
        coordinator.start();
        coordinator.request().await;
        sleep(Duration::from_millis(20)).await;
        assert_eq!(coordinator.status().await.state, RefreshState::Failed);
        coordinator.request().await;
        sleep(Duration::from_millis(20)).await;
        assert_eq!(coordinator.status().await.state, RefreshState::Success);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
