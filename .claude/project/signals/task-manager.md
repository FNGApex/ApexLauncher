# task-manager

## What it does

Serial FIFO `TaskManager` (Approach C) that runs tasks one at a time in enqueue order. One worker tokio task drains an unbounded `mpsc` channel; it runs each `TaskJob` to a terminal status before picking up the next. The snapshot (`Arc<RwLock<Vec<Task>>>`) is the shared read surface — query commands and event emission read it; the worker (via `TaskContext`) is the sole writer. Per-task `CancelToken` (from `download::CancelToken`) trips running downloads and short-circuits queued tasks. Five `TaskKind` variants: `Synthetic` (tests), `PackInstall`, `PackUpdate`, `ModAdd`, `ModUpdate`. `TaskStatus` states: `Queued → Planning → Downloading → Applying → Done | Failed | Cancelled`. `finish_done_with_result` attaches a `serde_json::Value` result payload to the snapshot before the `Done` transition; the `task://update` event emitted at that point carries both the terminal status and the payload. 17 Rust tests (up from 11).

## CLI code

- `src-tauri/src/core/task_manager.rs` (516 lines) — `TaskKind`, `TaskStatus` (`#[serde(tag = "kind")]`), `ChildItem`, `Task` (snapshot row: id, kind, parent_label, status, children, current_child, done, total, optional result), `TaskProgress`, `TaskObserver` trait (`progress` / `status_changed`), `NoOpObserver`, `TaskContext` (per-task handle handed to jobs: `enter_planning`, `enter_downloading`, `start_child`, `finish_child`, `enter_applying`, `finish_done`, `finish_done_with_result`, `finish_failed`, `finish_cancelled`, `cancel_token`, `is_cancelled`; lock discipline: never hold `RwLock` across `.await`), `TaskJob` async trait (`run(self: Box<Self>, ctx: &TaskContext)`), `TaskSpec` (kind + parent_label + job), `TaskManager` (cloneable handle: `enqueue` → `u64`, `list` → `Vec<Task>`, `cancel`); `worker_loop` (private; FIFO drain; safety-net `finish_done` if job returns without terminal status); ends with `#[cfg(test)] #[path = "task_manager_tests.rs"] mod tests;` stub
- `src-tauri/src/core/task_manager_tests.rs` — 17 unit tests (all `#[tokio::test]`): FIFO ordering, cancel-queued, cancel-running, snapshot counts, `finish_done_with_result` payload, observer full-lifecycle; wired via `#[path]` stub
- `src-tauri/src/lib.rs` — `TauriTaskObserver` (`TaskObserver` impl: emits `task://progress` via `TaskProgressPayload` and `task://update` via `TaskUpdatePayload`); `list_tasks` Tauri command (returns `Vec<Task>` snapshot); `cancel_task` Tauri command (delegates to `manager.cancel(id)`); `TaskManager` registered via `.manage(TaskManager::new(Arc::new(TauriTaskObserver { app })))`; `ImportMrpackJob`, `ImportCfZipJob`, `UpdateModpackJob` (pack ops, CP-3), `ModAddJob`, `ModUpdateJob` (mod ops, CP-4) — all `TaskJob` implementors; `staging_dir_for(inst_dir, task_id)` → `<inst_dir>/.staging-<task_id>/`

## Artifacts

- `src/lib/bindings.ts` (generated) — `Task`, `TaskStatus`, `TaskKind`, `ChildItem`, `TaskResult`, `TaskProgressPayload`, `TaskUpdatePayload` generated from Rust via tauri-specta; `events.taskProgress` + `events.taskUpdate` typed listeners. This is the authoritative type source — do not hand-edit.
- `src/lib/store.ts` — re-exports `Task`/`TaskKind`/`TaskStatus`/`ChildItem`/`TaskResult` straight from `bindings.ts` (no hand-declared task types); `RunState` composed as `RunUpdatePayload & { elapsedMs?: number | null }`; `TasksSlice` (`tasks: Map<number, Task>`, `upsertTask`, `patchTaskProgress`) and `RunsSlice` composed into `useAppStore`
- `src/lib/ipc.ts` — thin adapter: `listTasks` / `cancelTask` wrappers routing through `commands.*`; no hand-declared task type interfaces
- `src/components/DownloadManager.tsx` — `DownloadManagerButton` + `DownloadManagerPanel` + `TaskRow` + `StatusBadge`; reads tasks from `useAppStore(s => s.tasks)`; calls `cancelTask` for active tasks
- `src/components/AppShell.tsx` — subscribes to `task://progress` + `task://update` via `events.taskProgress.listen` / `events.taskUpdate.listen` (generated surface); hydrates store via `listTasks()` on mount

## Docs

- `docs/spec/download-runner-rework/README.md` — feature overview: CP-1 through CP-9, success criteria, design decisions
- `docs/spec/download-runner-rework/cp-2-task-manager-core.md` — CP-2 spec: `TaskManager`, worker loop, `TaskJob` trait, `TaskContext` API, snapshot contract, lock discipline
- `docs/spec/download-runner-rework/cp-3-pack-ops-stage-promote.md` — CP-3 spec: pack job implementations
- `docs/spec/download-runner-rework/cp-4-mod-ops-fast-path.md` — CP-4 spec: mod job implementations
- `docs/design/download-runner-rework.md` — overall design: Approach C rationale, FIFO contract, staging/promote pattern, observer pattern, state diagram

## Coupling

- **download domain** — `TaskContext.cancel_token()` returns a `CancelToken` from `download::CancelToken`; `execute_plan_cancellable` takes this token. `task_manager.rs` imports `CancelToken` directly from `crate::core::download`.
- **modpack domain** — `ImportMrpackJob`, `ImportCfZipJob`, `UpdateModpackJob` are `TaskJob` implementors defined in `lib.rs`; they use `remap_to_staging`/`promote_staging`/`extract_overrides` from `core/modpack.rs`
- **mod-install domain** — `ModAddJob`, `ModUpdateJob` are `TaskJob` implementors in `lib.rs`; they use `mod_install::resolve_install` / `fetch_newest_compatible` and `remap_to_staging`/`promote_staging` from `core/modpack.rs`
- **frontend-shell domain** — `AppShell` subscribes to `task://progress` + `task://update` and drives the `tasks` slice of the Zustand store; `DownloadManagerButton`/`DownloadManagerPanel` read that slice; `Toasts` fires on `Done` tasks with a `result` field
- **lib.rs** — `TaskManager` is a Tauri managed-state singleton; all command handlers that enqueue work call `manager.enqueue(TaskSpec { ... })` and return the resulting `u64` task id

## Conventions worth knowing

- Lock discipline: `TaskContext` mutators take the `RwLock` write lock synchronously, mutate, drop the lock, then call the observer — no lock held across `.await`. This mirrors the discipline in `launch.rs` for `RunningRegistry`.
- Worker serializes tasks: `spec.job.run(&ctx).await` must complete before `rx.recv()` is called again. Jobs must not `tokio::spawn` their own sub-tasks that outlive `run()`.
- `TaskManager` is `Clone` — all clones share the same snapshot, id counter, cancel-token table, and worker channel. Tauri manages one instance; job impls receive a clone via Tauri state.
- `finish_done_with_result` writes the result into the snapshot first, then transitions to `Done`, then notifies the observer — so the `task://update` event always carries both `status: {kind: "done"}` and a populated `result` field.
- `TaskJob` is `#[async_trait]` — `run(self: Box<Self>, ctx: &TaskContext)` consumes the job. Jobs must not assume any particular tokio runtime flavor beyond what Tauri provides.
- `Synthetic` kind is test-only; the worker does not special-case it.
- Known test flake in `download_tests.rs` (`cp4_concurrency_bound_not_exceeded`) is a pre-existing timing issue unrelated to this domain.
