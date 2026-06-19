//! Serial FIFO **Download Manager** — the task queue (Approach C).
//!
//! One worker task owns a FIFO queue and runs tasks **strictly serially** (one
//! at a time, in enqueue order). The worker is the sole writer of a shared
//! snapshot held in an `Arc<RwLock<Vec<Task>>>`; query commands (`list_tasks`)
//! and event emission read that snapshot. One writer, many readers — the
//! `RwLock` discipline stays simple.
//!
//! A task is **hierarchical**: a parent op (e.g. "Install ATM10") runs a
//! *plan phase* (status `Planning`) producing a child queue, an *execute phase*
//! (status `Downloading`) iterating child items, then an *apply phase* (status
//! `Applying`) before terminating `Done`. The snapshot surfaces the parent
//! label, the current child label, and `done`/`total` counts.
//!
//! No real pack/mod ops are wired here (that is CP-3/CP-4). The worker runs a
//! [`TaskJob`] trait object; the only implementation in this checkpoint is the
//! test-only synthetic job, which lets the lifecycle, FIFO, serialization, and
//! cancel invariants be exercised without a live network.
//!
//! ## Lock discipline
//!
//! **Never hold the `RwLock` across an `.await`.** Every snapshot mutation is a
//! synchronous closure (`with_snapshot_mut`) that takes the write lock, mutates,
//! and drops it before returning — the worker awaits only *outside* those
//! closures. Mirrors the rule the runner enforces in `launch.rs`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};

use crate::core::download::CancelToken;

// ---------------------------------------------------------------------------
// Task model
// ---------------------------------------------------------------------------

/// The kind of work a task performs. Serialized as a camelCase tag for the
/// frontend. CP-4 adds `ModAdd` / `ModUpdate`; the `Synthetic` variant exists
/// for tests + the no-real-ops contract from CP-2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TaskKind {
    /// Test / placeholder work with no real side effects.
    Synthetic,
    /// A new modpack install (import_mrpack, import_curseforge_zip, install_modpack).
    PackInstall,
    /// An update to an already-installed modpack (update_modpack).
    PackUpdate,
    /// Installing a mod (and its required transitive deps) into an instance.
    ModAdd,
    /// Updating a single installed mod to its newest compatible version.
    ModUpdate,
}

/// Lifecycle status of a task. Matches the design's state diagram.
///
/// Non-terminal: `Queued`, `Planning`, `Downloading`, `Applying`.
/// Terminal: `Done`, `Failed`, `Cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskStatus {
    /// Enqueued, not yet picked up by the worker.
    Queued,
    /// Worker is building the child queue (fetch manifest / resolve deps).
    Planning,
    /// Worker is downloading child items.
    Downloading,
    /// All children downloaded; applying (overlay / promote).
    Applying,
    /// Completed successfully.
    Done,
    /// Failed; `message` carries the reason.
    Failed { message: String },
    /// Cancelled before completion (from `Queued` or any running status).
    Cancelled,
}

impl TaskStatus {
    /// `true` for the three terminal statuses (`Done` / `Failed` / `Cancelled`).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Done | TaskStatus::Failed { .. } | TaskStatus::Cancelled
        )
    }
}

/// A single child item within a task (one downloadable file).
///
/// `label` is the human-facing name shown as the "current child" — derived from
/// the [`DownloadItem.dest`](crate::core::download::DownloadItem) basename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChildItem {
    pub label: String,
}

/// A snapshot of one task's full state. Cloned out of the snapshot for reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Unique, monotonic task id (assigned at enqueue).
    pub id: u64,
    /// What kind of work this is.
    pub kind: TaskKind,
    /// Human-facing parent label (e.g. "Install ATM10").
    pub parent_label: String,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Child items, populated during the `Planning` phase.
    pub children: Vec<ChildItem>,
    /// Label of the child currently being processed; `None` outside the
    /// `Downloading` phase.
    pub current_child: Option<String>,
    /// Number of children completed so far.
    pub done: u32,
    /// Total number of children (set once the plan is built).
    pub total: u32,
    /// Terminal result payload set by [`TaskContext::finish_done_with_result`].
    /// `None` until the task reaches `Done` via that method; stays `None` for
    /// tasks that finish via `finish_done` (no result), `finish_failed`, or
    /// `finish_cancelled`. Serialized as `null` when absent.
    ///
    /// Stored as `serde_json::Value` at runtime for flexibility; the specta type
    /// override points at [`crate::TaskResult`] so the generated `bindings.ts`
    /// carries the typed union instead of an opaque `any`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(type = Option<crate::TaskResult>)]
    pub result: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Progress observation
// ---------------------------------------------------------------------------

/// Per-child progress observation emitted while a task downloads.
///
/// The Tauri sink maps this to the `task://progress` event; tests use a
/// capturing sink. Lifecycle/status changes are observed separately via the
/// [`TaskObserver::status_changed`] hook (→ `task://update`).
#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub task_id: u64,
    pub current_child: Option<String>,
    pub done: u32,
    pub total: u32,
    /// Bytes received so far for the current child (best-effort).
    pub bytes: u64,
}

/// Sink the worker reports through. The Tauri implementation lives in `lib.rs`
/// and emits `task://progress` / `task://update`; tests pass a capturing one.
///
/// `progress` fires per child transition; `status_changed` fires once per
/// lifecycle transition with a fresh [`Task`] snapshot.
pub trait TaskObserver: Send + Sync {
    fn progress(&self, _update: TaskProgress) {}
    fn status_changed(&self, _task: &Task) {}
}

/// A [`TaskObserver`] that discards everything.
pub struct NoOpObserver;
impl TaskObserver for NoOpObserver {}

// ---------------------------------------------------------------------------
// Job seam
// ---------------------------------------------------------------------------

/// Handle handed to a [`TaskJob`] so it can drive its own task's snapshot
/// through the worker. The job mutates exactly its own task; the worker
/// guarantees serial execution, so there is never contention between jobs.
///
/// Every mutator here takes the write lock, applies a synchronous change, drops
/// the lock, then (for status transitions) notifies the observer — all without
/// holding the lock across an `.await`.
pub struct TaskContext {
    task_id: u64,
    snapshot: Arc<RwLock<Vec<Task>>>,
    observer: Arc<dyn TaskObserver>,
    cancel: CancelToken,
}

impl TaskContext {
    /// The cancel token for this task. The job (or `execute_plan_cancellable`)
    /// checks `is_cancelled()`; `cancel_task` trips it from outside.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// `true` if this task has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Transition to `Planning`.
    pub async fn enter_planning(&self) {
        self.set_status(TaskStatus::Planning).await;
    }

    /// Record the resolved child queue and transition to `Downloading`.
    pub async fn enter_downloading(&self, children: Vec<ChildItem>) {
        let total = children.len() as u32;
        self.with_snapshot_mut(|t| {
            t.children = children;
            t.total = total;
            t.done = 0;
            t.current_child = None;
            t.status = TaskStatus::Downloading;
        })
        .await;
        self.notify().await;
    }

    /// Mark a child as the current in-flight item and emit progress.
    pub async fn start_child(&self, label: &str, bytes: u64) {
        let (done, total) = self
            .with_snapshot_mut(|t| {
                t.current_child = Some(label.to_owned());
                (t.done, t.total)
            })
            .await
            .unwrap_or((0, 0));
        self.observer.progress(TaskProgress {
            task_id: self.task_id,
            current_child: Some(label.to_owned()),
            done,
            total,
            bytes,
        });
    }

    /// Mark the current child finished (bump `done`) and emit progress.
    pub async fn finish_child(&self) {
        let (done, total, current) = self
            .with_snapshot_mut(|t| {
                t.done += 1;
                (t.done, t.total, t.current_child.clone())
            })
            .await
            .unwrap_or((0, 0, None));
        self.observer.progress(TaskProgress {
            task_id: self.task_id,
            current_child: current,
            done,
            total,
            bytes: 0,
        });
    }

    /// Transition to `Applying`.
    pub async fn enter_applying(&self) {
        self.with_snapshot_mut(|t| {
            t.current_child = None;
            t.status = TaskStatus::Applying;
        })
        .await;
        self.notify().await;
    }

    /// Terminal: `Done`.
    pub async fn finish_done(&self) {
        self.set_status(TaskStatus::Done).await;
    }

    /// Terminal: `Done` with a result payload attached to the task snapshot.
    ///
    /// The result is set before the status transitions to `Done`, so the
    /// `task://update` event the observer fires carries both the terminal status
    /// and the payload. The frontend CP-7 reads `task.result` on the Done event.
    ///
    /// Lock discipline: write lock taken synchronously, dropped before the status
    /// notification (observer call). No lock is held across `.await`.
    pub async fn finish_done_with_result(&self, value: serde_json::Value) {
        // Write the result into the snapshot, then transition to Done.
        self.with_snapshot_mut(|t| {
            t.result = Some(value);
        })
        .await;
        self.set_status(TaskStatus::Done).await;
    }

    /// Terminal: `Failed`.
    pub async fn finish_failed(&self, message: impl Into<String>) {
        self.set_status(TaskStatus::Failed {
            message: message.into(),
        })
        .await;
    }

    /// Terminal: `Cancelled`.
    pub async fn finish_cancelled(&self) {
        self.set_status(TaskStatus::Cancelled).await;
    }

    async fn set_status(&self, status: TaskStatus) {
        self.with_snapshot_mut(|t| t.status = status).await;
        self.notify().await;
    }

    async fn notify(&self) {
        if let Some(task) = self.with_snapshot(|t| t.clone()).await {
            self.observer.status_changed(&task);
        }
    }

    /// Take the write lock, mutate this task in place, drop the lock, return the
    /// closure's value. The lock is never held across `.await`.
    async fn with_snapshot_mut<R>(&self, f: impl FnOnce(&mut Task) -> R) -> Option<R> {
        let mut guard = self.snapshot.write().await;
        let r = guard.iter_mut().find(|t| t.id == self.task_id).map(f);
        drop(guard);
        r
    }

    async fn with_snapshot<R>(&self, f: impl FnOnce(&Task) -> R) -> Option<R> {
        let guard = self.snapshot.read().await;
        let r = guard.iter().find(|t| t.id == self.task_id).map(f);
        drop(guard);
        r
    }
}

/// The executable behavior of a task. The worker calls [`run`](TaskJob::run)
/// exactly once, just after the task is enqueued as `Queued`. CP-3/CP-4 add
/// real implementations (pack install/update, mod add/update); this checkpoint
/// ships only the test-only synthetic job.
///
/// The job drives the full lifecycle through the [`TaskContext`] —
/// `enter_planning` → `enter_downloading` → per-child → `enter_applying` →
/// `finish_done`/`finish_failed`/`finish_cancelled`. The worker records the
/// terminal status (`Done`) only as a safety net if the job returns without
/// setting one.
#[async_trait::async_trait]
pub trait TaskJob: Send {
    async fn run(self: Box<Self>, ctx: &TaskContext);
}

/// A queued spec: what to enqueue. The `job` carries the behavior; `kind` and
/// `parent_label` are the snapshot metadata.
pub struct TaskSpec {
    pub kind: TaskKind,
    pub parent_label: String,
    pub job: Box<dyn TaskJob>,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Message the worker consumes off its FIFO channel.
enum WorkerMsg {
    Enqueue { task_id: u64, spec: TaskSpec },
}

/// The serial Download Manager. Managed in Tauri state. Cloneable handle —
/// clones share the snapshot, the id counter, the cancel-token table, and the
/// worker channel.
#[derive(Clone)]
pub struct TaskManager {
    snapshot: Arc<RwLock<Vec<Task>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    /// Per-task cancel tokens, keyed by id. A task's token is created at enqueue
    /// and removed once terminal. `cancel_task` trips it.
    cancels: Arc<std::sync::Mutex<std::collections::HashMap<u64, CancelToken>>>,
    tx: mpsc::UnboundedSender<WorkerMsg>,
    observer: Arc<dyn TaskObserver>,
}

impl TaskManager {
    /// Construct a manager and spawn its single worker task. The worker lives
    /// for the process lifetime, draining the FIFO channel and running each
    /// task to a terminal status before taking the next.
    pub fn new(observer: Arc<dyn TaskObserver>) -> Self {
        let snapshot: Arc<RwLock<Vec<Task>>> = Arc::new(RwLock::new(Vec::new()));
        let cancels: Arc<std::sync::Mutex<std::collections::HashMap<u64, CancelToken>>> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let (tx, rx) = mpsc::unbounded_channel::<WorkerMsg>();

        let worker_snapshot = Arc::clone(&snapshot);
        let worker_cancels = Arc::clone(&cancels);
        let worker_observer = Arc::clone(&observer);
        // `tauri::async_runtime::spawn`, not `tokio::spawn`: `new` is called from
        // Tauri's synchronous `.setup()` hook at startup, where no Tokio runtime
        // is entered on the current thread. Tauri's async_runtime spawns onto its
        // own managed runtime regardless of the caller's context, so the worker
        // starts whether `new` runs in `.setup()` or inside an async command.
        tauri::async_runtime::spawn(async move {
            worker_loop(rx, worker_snapshot, worker_cancels, worker_observer).await;
        });

        Self {
            snapshot,
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            cancels,
            tx,
            observer,
        }
    }

    /// Enqueue a task. Inserts a `Queued` entry into the snapshot synchronously
    /// (so an immediately-following `list_tasks` sees it) and hands the job to
    /// the worker. Returns the assigned task id.
    pub async fn enqueue(&self, spec: TaskSpec) -> u64 {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let cancel = CancelToken::new();
        self.cancels.lock().unwrap().insert(id, cancel);

        let task = Task {
            id,
            kind: spec.kind,
            parent_label: spec.parent_label.clone(),
            status: TaskStatus::Queued,
            children: Vec::new(),
            current_child: None,
            done: 0,
            total: 0,
            result: None,
        };
        {
            let mut guard = self.snapshot.write().await;
            guard.push(task.clone());
        }
        self.observer.status_changed(&task);

        // Unbounded send to the worker; the worker drains in FIFO order.
        let _ = self.tx.send(WorkerMsg::Enqueue { task_id: id, spec });
        id
    }

    /// Read the current snapshot (clone of every task).
    pub async fn list(&self) -> Vec<Task> {
        self.snapshot.read().await.clone()
    }

    /// Cancel a task by id. Trips the task's [`CancelToken`] (so a running
    /// download stops acquiring new permits) and, if the task is still `Queued`,
    /// marks it `Cancelled` immediately so it never starts.
    ///
    /// A running task observes the tripped token at its next checkpoint and the
    /// job calls `finish_cancelled`. A queued task is short-circuited here.
    pub async fn cancel(&self, id: u64) {
        // Trip the token first; idempotent and safe even if already terminal.
        if let Some(tok) = self.cancels.lock().unwrap().get(&id) {
            tok.cancel();
        }
        // If still Queued, flip it terminal now so the worker skips it.
        let mut changed: Option<Task> = None;
        {
            let mut guard = self.snapshot.write().await;
            if let Some(t) = guard.iter_mut().find(|t| t.id == id) {
                if t.status == TaskStatus::Queued {
                    t.status = TaskStatus::Cancelled;
                    changed = Some(t.clone());
                }
            }
        }
        if let Some(t) = changed {
            self.observer.status_changed(&t);
        }
    }
}

/// The single worker loop. Drains the FIFO channel; for each message runs the
/// job to a terminal status before consuming the next — this is what makes the
/// queue strictly serial. The snapshot is mutated only via [`TaskContext`].
async fn worker_loop(
    mut rx: mpsc::UnboundedReceiver<WorkerMsg>,
    snapshot: Arc<RwLock<Vec<Task>>>,
    cancels: Arc<std::sync::Mutex<std::collections::HashMap<u64, CancelToken>>>,
    observer: Arc<dyn TaskObserver>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            WorkerMsg::Enqueue { task_id, spec } => {
                // Look up this task's cancel token (created at enqueue).
                let cancel = cancels
                    .lock()
                    .unwrap()
                    .get(&task_id)
                    .cloned()
                    .unwrap_or_default();

                let ctx = TaskContext {
                    task_id,
                    snapshot: Arc::clone(&snapshot),
                    observer: Arc::clone(&observer),
                    cancel: cancel.clone(),
                };

                // If cancelled before the worker picked it up, mark terminal and
                // skip running the job entirely.
                if cancel.is_cancelled() {
                    ctx.finish_cancelled().await;
                } else {
                    spec.job.run(&ctx).await;
                    // Safety net: if the job returned without a terminal status,
                    // record Done (the job owns the real transition).
                    let needs_terminal = {
                        let guard = snapshot.read().await;
                        let v = guard
                            .iter()
                            .find(|t| t.id == task_id)
                            .map(|t| !t.status.is_terminal())
                            .unwrap_or(false);
                        drop(guard);
                        v
                    };
                    if needs_terminal {
                        ctx.finish_done().await;
                    }
                }

                // Drop the per-task token now that it is terminal.
                cancels.lock().unwrap().remove(&task_id);
            }
        }
    }
}

#[cfg(test)]
#[path = "task_manager_tests.rs"]
mod tests;
