//! Unit tests for the serial FIFO Download Manager ([`TaskManager`]).
//!
//! All tasks here are **synthetic** — no real network. The [`SyntheticJob`]
//! drives the real lifecycle (`Planning` → `Downloading` → per-child →
//! `Applying` → `Done`/`Cancelled`) through the [`TaskContext`], with a pair of
//! channels that let a test step the job phase-by-phase and observe the
//! snapshot at each pause. This is the seam CP-3/CP-4 plug real ops into:
//! implement [`TaskJob`] and enqueue a [`TaskSpec`] carrying it.

use super::*;
use std::sync::Mutex as StdMutex;
use tokio::sync::mpsc as tokmpsc;

// ---------------------------------------------------------------------------
// Synthetic job + controllable step gate
// ---------------------------------------------------------------------------

/// A signal a test sends to release the next phase of a [`SyntheticJob`].
enum Step {
    /// Release: enter Planning then Downloading with the given child labels.
    Plan(Vec<String>),
    /// Release: start + finish the next child (label).
    Child(String),
    /// Release: enter Applying then finish Done.
    Finish,
}

/// A [`TaskJob`] a test drives one phase at a time. Between phases it parks on
/// `rx.recv()`, so a test can read the snapshot at a known point. If the channel
/// closes (sender dropped), the job checks cancellation and finishes.
struct SyntheticJob {
    rx: tokmpsc::UnboundedReceiver<Step>,
}

#[async_trait::async_trait]
impl TaskJob for SyntheticJob {
    async fn run(mut self: Box<Self>, ctx: &TaskContext) {
        while let Some(step) = self.rx.recv().await {
            if ctx.is_cancelled() {
                ctx.finish_cancelled().await;
                return;
            }
            match step {
                Step::Plan(labels) => {
                    ctx.enter_planning().await;
                    let children = labels
                        .into_iter()
                        .map(|label| ChildItem { label })
                        .collect();
                    ctx.enter_downloading(children).await;
                }
                Step::Child(label) => {
                    if ctx.is_cancelled() {
                        ctx.finish_cancelled().await;
                        return;
                    }
                    ctx.start_child(&label, 0).await;
                    ctx.finish_child().await;
                }
                Step::Finish => {
                    ctx.enter_applying().await;
                    ctx.finish_done().await;
                    return;
                }
            }
        }
        // Channel closed without an explicit Finish: honor cancel, else Done.
        if ctx.is_cancelled() {
            ctx.finish_cancelled().await;
        } else {
            ctx.finish_done().await;
        }
    }
}

/// Build a synthetic spec + the step sender that drives it.
fn synthetic(label: &str) -> (TaskSpec, tokmpsc::UnboundedSender<Step>) {
    let (tx, rx) = tokmpsc::unbounded_channel();
    let spec = TaskSpec {
        kind: TaskKind::Synthetic,
        parent_label: label.to_owned(),
        job: Box::new(SyntheticJob { rx }),
    };
    (spec, tx)
}

/// An auto-completing synthetic spec: closing the step channel immediately lets
/// the job finish `Done` without any test stepping. Used for FIFO ordering.
fn auto_synthetic(label: &str) -> TaskSpec {
    let (tx, rx) = tokmpsc::unbounded_channel::<Step>();
    drop(tx); // channel closed → job's recv() loop ends → finishes Done
    TaskSpec {
        kind: TaskKind::Synthetic,
        parent_label: label.to_owned(),
        job: Box::new(SyntheticJob { rx }),
    }
}

// ---------------------------------------------------------------------------
// Capturing observer
// ---------------------------------------------------------------------------

/// Records every status change (ordered) and every progress update.
#[derive(Default)]
struct CapturingObserver {
    statuses: StdMutex<Vec<(u64, TaskStatus)>>,
    progress: StdMutex<Vec<TaskProgress>>,
}

impl TaskObserver for CapturingObserver {
    fn progress(&self, update: TaskProgress) {
        self.progress.lock().unwrap().push(update);
    }
    fn status_changed(&self, task: &Task) {
        self.statuses
            .lock()
            .unwrap()
            .push((task.id, task.status.clone()));
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Poll the snapshot until `pred` holds or a deadline elapses. Returns the
/// snapshot that satisfied the predicate, or panics with the last snapshot.
async fn wait_until(mgr: &TaskManager, pred: impl Fn(&[Task]) -> bool, what: &str) -> Vec<Task> {
    for _ in 0..2000 {
        let snap = mgr.list().await;
        if pred(&snap) {
            return snap;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    let snap = mgr.list().await;
    panic!("timed out waiting for: {what}; snapshot={snap:?}");
}

fn find<'a>(snap: &'a [Task], id: u64) -> &'a Task {
    snap.iter().find(|t| t.id == id).expect("task present")
}

/// Count tasks in a *running* (actively-executing) status. The serial invariant
/// is that this is ≤ 1 — a task in `Queued` is non-terminal but NOT running, so
/// it does not count against serialization (the worker hasn't picked it up).
fn running_count(snap: &[Task]) -> usize {
    snap.iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Planning | TaskStatus::Downloading | TaskStatus::Applying
            )
        })
        .count()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Regression: `TaskManager::new` must construct cleanly when called from a
/// *synchronous* context with no Tokio runtime entered on the current thread.
/// That is exactly app startup — `new` runs inside Tauri's `.setup()` hook,
/// which is not an async command, so the worker spawn cannot rely on a current
/// Tokio reactor. A plain `#[test]` (no `#[tokio::test]`) reproduces that
/// context; before the fix the inner `tokio::spawn` panicked with "there is no
/// reactor running, must be called from the context of a Tokio 1.x runtime".
#[test]
fn new_constructs_outside_tokio_runtime() {
    let _mgr = TaskManager::new(Arc::new(NoOpObserver));
}

/// enqueue inserts a `Queued` entry synchronously and `list` returns it.
#[tokio::test]
async fn enqueue_inserts_queued_and_list_reads_snapshot() {
    let mgr = TaskManager::new(Arc::new(NoOpObserver));
    let (spec, _tx) = synthetic("pack-a");
    let id = mgr.enqueue(spec).await;

    let snap = mgr.list().await;
    let t = find(&snap, id);
    assert_eq!(t.parent_label, "pack-a");
    assert_eq!(t.kind, TaskKind::Synthetic);
    // Status is Queued at enqueue (worker may pick it up; we asserted it exists).
}

/// Invariant (1): the snapshot NEVER shows two tasks in a non-terminal status
/// at once — the worker is strictly serial. We hold task A in Downloading while
/// task B is enqueued, then sample the snapshot many times: B must stay Queued
/// (non-terminal count == 1) until A terminates.
#[tokio::test]
async fn snapshot_never_shows_two_non_terminal_at_once() {
    let mgr = TaskManager::new(Arc::new(NoOpObserver));

    let (spec_a, tx_a) = synthetic("A");
    let id_a = mgr.enqueue(spec_a).await;
    let (spec_b, tx_b) = synthetic("B");
    let id_b = mgr.enqueue(spec_b).await;

    // Drive A into Downloading and park there.
    tx_a.send(Step::Plan(vec!["a1.jar".into(), "a2.jar".into()]))
        .unwrap();
    wait_until(
        &mgr,
        |s| find(s, id_a).status == TaskStatus::Downloading,
        "A → Downloading",
    )
    .await;

    // While A is parked in Downloading, repeatedly sample: at most ONE task is
    // in a running status (serial), and B must still be Queued (the worker can't
    // pick it up until A terminates).
    for _ in 0..50 {
        let snap = mgr.list().await;
        assert!(
            running_count(&snap) <= 1,
            "at most one running task while A runs; snap={snap:?}"
        );
        assert_eq!(
            find(&snap, id_b).status,
            TaskStatus::Queued,
            "B waits in queue"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }

    // Let A finish; B then runs and we let it finish too.
    tx_a.send(Step::Child("a1.jar".into())).unwrap();
    tx_a.send(Step::Child("a2.jar".into())).unwrap();
    tx_a.send(Step::Finish).unwrap();
    wait_until(
        &mgr,
        |s| find(s, id_a).status == TaskStatus::Done,
        "A → Done",
    )
    .await;

    tx_b.send(Step::Plan(vec!["b1.jar".into()])).unwrap();
    tx_b.send(Step::Child("b1.jar".into())).unwrap();
    tx_b.send(Step::Finish).unwrap();
    wait_until(
        &mgr,
        |s| find(s, id_b).status == TaskStatus::Done,
        "B → Done",
    )
    .await;
}

/// Invariant (2): tasks leave `Queued` in FIFO order. Three auto-completing
/// tasks; the order in which each first leaves `Queued` (status change) matches
/// enqueue order.
#[tokio::test]
async fn tasks_leave_queued_in_fifo_order() {
    let obs = Arc::new(CapturingObserver::default());
    let mgr = TaskManager::new(obs.clone());

    let id1 = mgr.enqueue(auto_synthetic("first")).await;
    let id2 = mgr.enqueue(auto_synthetic("second")).await;
    let id3 = mgr.enqueue(auto_synthetic("third")).await;

    wait_until(
        &mgr,
        |s| s.iter().all(|t| t.status.is_terminal()),
        "all three terminal",
    )
    .await;

    // The first non-Queued status change observed per id reflects when it left
    // the queue. Collect ids in the order they first transitioned off Queued.
    let statuses = obs.statuses.lock().unwrap().clone();
    let mut left_queued_order = Vec::new();
    for (id, status) in statuses {
        if status != TaskStatus::Queued && !left_queued_order.contains(&id) {
            left_queued_order.push(id);
        }
    }
    assert_eq!(
        left_queued_order,
        vec![id1, id2, id3],
        "tasks leave Queued in enqueue (FIFO) order"
    );
}

/// Invariant (3a): `cancel_task` on a still-`Queued` task moves it to
/// `Cancelled` and the worker never runs it.
#[tokio::test]
async fn cancel_queued_task_moves_to_cancelled() {
    let mgr = TaskManager::new(Arc::new(NoOpObserver));

    // Task A parks in Downloading so B stays Queued behind it.
    let (spec_a, tx_a) = synthetic("A");
    let id_a = mgr.enqueue(spec_a).await;
    let (spec_b, _tx_b) = synthetic("B");
    let id_b = mgr.enqueue(spec_b).await;

    tx_a.send(Step::Plan(vec!["a1.jar".into()])).unwrap();
    wait_until(
        &mgr,
        |s| find(s, id_a).status == TaskStatus::Downloading,
        "A → Downloading",
    )
    .await;

    // B is Queued; cancel it.
    assert_eq!(
        mgr.list()
            .await
            .iter()
            .find(|t| t.id == id_b)
            .unwrap()
            .status,
        TaskStatus::Queued
    );
    mgr.cancel(id_b).await;

    let snap = mgr.list().await;
    assert_eq!(
        find(&snap, id_b).status,
        TaskStatus::Cancelled,
        "queued task cancelled before it ran"
    );

    // Unblock A so the worker reaches B; B must NOT run (already Cancelled).
    tx_a.send(Step::Child("a1.jar".into())).unwrap();
    tx_a.send(Step::Finish).unwrap();
    wait_until(
        &mgr,
        |s| find(s, id_a).status == TaskStatus::Done,
        "A → Done",
    )
    .await;

    // Give the worker a moment to drain B's message; it must stay Cancelled.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(find(&mgr.list().await, id_b).status, TaskStatus::Cancelled);
}

/// Invariant (3b): `cancel_task` on a *running* task trips the CP-1 cancel
/// token; the job observes it at its next checkpoint and finishes `Cancelled`.
#[tokio::test]
async fn cancel_running_task_moves_to_cancelled() {
    let mgr = TaskManager::new(Arc::new(NoOpObserver));

    let (spec, tx) = synthetic("running");
    let id = mgr.enqueue(spec).await;

    tx.send(Step::Plan(vec!["x.jar".into(), "y.jar".into()]))
        .unwrap();
    wait_until(
        &mgr,
        |s| find(s, id).status == TaskStatus::Downloading,
        "→ Downloading",
    )
    .await;

    // Cancel while running (Downloading). Token trips; next Child step the job
    // sees is_cancelled() and bails to Cancelled.
    mgr.cancel(id).await;
    tx.send(Step::Child("x.jar".into())).unwrap();

    let snap = wait_until(&mgr, |s| find(s, id).status.is_terminal(), "→ terminal").await;
    assert_eq!(
        find(&snap, id).status,
        TaskStatus::Cancelled,
        "running task cancelled via the CP-1 token"
    );
}

/// Invariant (4): the snapshot reflects the current child label + done/total
/// while a task runs.
#[tokio::test]
async fn snapshot_reflects_current_child_and_counts() {
    let obs = Arc::new(CapturingObserver::default());
    let mgr = TaskManager::new(obs.clone());

    let (spec, tx) = synthetic("counts");
    let id = mgr.enqueue(spec).await;

    tx.send(Step::Plan(vec![
        "one.jar".into(),
        "two.jar".into(),
        "three.jar".into(),
    ]))
    .unwrap();
    let snap = wait_until(
        &mgr,
        |s| find(s, id).status == TaskStatus::Downloading,
        "→ Downloading",
    )
    .await;
    let t = find(&snap, id);
    assert_eq!(t.total, 3, "total set from plan");
    assert_eq!(t.done, 0);
    assert_eq!(t.current_child, None);

    // First child in-flight.
    tx.send(Step::Child("one.jar".into())).unwrap();
    let snap = wait_until(&mgr, |s| find(s, id).done == 1, "one.jar finished").await;
    let t = find(&snap, id);
    assert_eq!(t.done, 1);
    assert_eq!(t.total, 3);

    // Finish remaining children.
    tx.send(Step::Child("two.jar".into())).unwrap();
    tx.send(Step::Child("three.jar".into())).unwrap();
    let snap = wait_until(&mgr, |s| find(s, id).done == 3, "all children done").await;
    assert_eq!(find(&snap, id).done, 3);

    tx.send(Step::Finish).unwrap();
    wait_until(&mgr, |s| find(s, id).status == TaskStatus::Done, "→ Done").await;

    // Progress observer saw a current_child label while downloading.
    let progress = obs.progress.lock().unwrap();
    assert!(
        progress
            .iter()
            .any(|p| p.current_child.as_deref() == Some("one.jar")),
        "progress carried the current child label"
    );
    assert!(
        progress.iter().any(|p| p.task_id == id && p.total == 3),
        "progress carried done/total counts"
    );
}

// ---------------------------------------------------------------------------
// CP-3 tests: TaskKind variants + result carrier
// ---------------------------------------------------------------------------

/// `PackInstall` and `PackUpdate` are distinct from `Synthetic` and from each other.
#[test]
fn cp3_task_kind_variants_are_distinct() {
    assert_ne!(TaskKind::PackInstall, TaskKind::Synthetic);
    assert_ne!(TaskKind::PackUpdate, TaskKind::Synthetic);
    assert_ne!(TaskKind::PackInstall, TaskKind::PackUpdate);
}

// ---------------------------------------------------------------------------
// CP-4 tests: ModAdd / ModUpdate kind variants
// ---------------------------------------------------------------------------

/// `ModAdd` and `ModUpdate` are distinct variants.
#[test]
fn cp4_task_kind_mod_variants_are_distinct() {
    assert_ne!(TaskKind::ModAdd, TaskKind::Synthetic);
    assert_ne!(TaskKind::ModUpdate, TaskKind::Synthetic);
    assert_ne!(TaskKind::ModAdd, TaskKind::PackInstall);
    assert_ne!(TaskKind::ModUpdate, TaskKind::PackUpdate);
    assert_ne!(TaskKind::ModAdd, TaskKind::ModUpdate);
}

/// `TaskKind::ModAdd` serialises as the camelCase tag `"modAdd"` for the frontend.
#[test]
fn cp4_mod_add_kind_serializes_camel_case() {
    let v = serde_json::to_value(TaskKind::ModAdd).expect("serialize");
    assert_eq!(v, serde_json::json!("modAdd"));
}

/// `TaskKind::ModUpdate` serialises as `"modUpdate"`.
#[test]
fn cp4_mod_update_kind_serializes_camel_case() {
    let v = serde_json::to_value(TaskKind::ModUpdate).expect("serialize");
    assert_eq!(v, serde_json::json!("modUpdate"));
}

/// `TaskKind::PackInstall` serialises as the camelCase tag `"packInstall"` for the frontend.
#[test]
fn cp3_pack_install_kind_serializes_camel_case() {
    let v = serde_json::to_value(TaskKind::PackInstall).expect("serialize");
    assert_eq!(v, serde_json::json!("packInstall"));
}

/// `TaskKind::PackUpdate` serialises as `"packUpdate"`.
#[test]
fn cp3_pack_update_kind_serializes_camel_case() {
    let v = serde_json::to_value(TaskKind::PackUpdate).expect("serialize");
    assert_eq!(v, serde_json::json!("packUpdate"));
}

/// `finish_done_with_result` attaches an arbitrary JSON payload to the task snapshot.
/// The terminal `task://update` (observer fires on the `Done` Task) carries the result.
#[tokio::test]
async fn cp3_finish_done_with_result_attaches_payload() {
    let obs = Arc::new(CapturingObserver::default());
    let mgr = TaskManager::new(obs.clone());

    // Use a synthetic job that calls finish_done_with_result.
    struct ResultJob;
    #[async_trait::async_trait]
    impl TaskJob for ResultJob {
        async fn run(self: Box<Self>, ctx: &TaskContext) {
            ctx.enter_planning().await;
            ctx.enter_downloading(vec![]).await;
            ctx.enter_applying().await;
            let payload = serde_json::json!({ "slug": "my-pack", "installed": 3 });
            ctx.finish_done_with_result(payload).await;
        }
    }

    let spec = TaskSpec {
        kind: TaskKind::PackInstall,
        parent_label: "My Pack".to_owned(),
        job: Box::new(ResultJob),
    };
    let id = mgr.enqueue(spec).await;

    wait_until(&mgr, |s| find(s, id).status == TaskStatus::Done, "→ Done").await;

    let snap = mgr.list().await;
    let task = find(&snap, id);
    assert_eq!(task.kind, TaskKind::PackInstall);
    let result = task.result.as_ref().expect("result must be set after finish_done_with_result");
    assert_eq!(result["slug"], "my-pack", "slug field must survive round-trip");
    assert_eq!(result["installed"], 3, "installed count must survive round-trip");
}

/// The terminal Done task emitted via the observer (→ `task://update`) carries the result field.
#[tokio::test]
async fn cp3_terminal_update_carries_result_field() {
    struct ResultCapture {
        tasks: StdMutex<Vec<Task>>,
    }
    impl TaskObserver for ResultCapture {
        fn status_changed(&self, task: &Task) {
            self.tasks.lock().unwrap().push(task.clone());
        }
    }

    let obs = Arc::new(ResultCapture { tasks: StdMutex::new(Vec::new()) });
    let mgr = TaskManager::new(obs.clone());

    struct ResultJob2;
    #[async_trait::async_trait]
    impl TaskJob for ResultJob2 {
        async fn run(self: Box<Self>, ctx: &TaskContext) {
            ctx.enter_planning().await;
            ctx.enter_downloading(vec![]).await;
            ctx.enter_applying().await;
            ctx.finish_done_with_result(serde_json::json!({ "slug": "cf-pack" })).await;
        }
    }

    let spec = TaskSpec {
        kind: TaskKind::PackUpdate,
        parent_label: "CF Pack".to_owned(),
        job: Box::new(ResultJob2),
    };
    let id = mgr.enqueue(spec).await;

    wait_until(&mgr, |s| find(s, id).status == TaskStatus::Done, "→ Done").await;

    let all_tasks = obs.tasks.lock().unwrap().clone();
    let done_task = all_tasks.iter()
        .filter(|t| t.id == id)
        .find(|t| t.status == TaskStatus::Done)
        .expect("observer must have received a Done task");

    let result = done_task.result.as_ref()
        .expect("Done task emitted by observer must carry the result");
    assert_eq!(result["slug"], "cf-pack");
}

/// A status change observer fires for the full lifecycle (→ task://update).
#[tokio::test]
async fn observer_sees_full_lifecycle() {
    let obs = Arc::new(CapturingObserver::default());
    let mgr = TaskManager::new(obs.clone());

    let (spec, tx) = synthetic("life");
    let id = mgr.enqueue(spec).await;
    tx.send(Step::Plan(vec!["a.jar".into()])).unwrap();
    tx.send(Step::Child("a.jar".into())).unwrap();
    tx.send(Step::Finish).unwrap();
    wait_until(&mgr, |s| find(s, id).status == TaskStatus::Done, "→ Done").await;

    let statuses: Vec<TaskStatus> = obs
        .statuses
        .lock()
        .unwrap()
        .iter()
        .filter(|(tid, _)| *tid == id)
        .map(|(_, s)| s.clone())
        .collect();
    // Queued → Planning → Downloading → Applying → Done, in order.
    assert_eq!(
        statuses,
        vec![
            TaskStatus::Queued,
            TaskStatus::Planning,
            TaskStatus::Downloading,
            TaskStatus::Applying,
            TaskStatus::Done,
        ],
    );
}
