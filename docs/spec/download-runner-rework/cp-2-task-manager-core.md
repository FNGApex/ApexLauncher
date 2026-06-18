# CP-2 — TaskManager core

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-1 (uses the download cancel seam)

## Goal

New backend module: a **serial FIFO** Download Manager (Approach C — single worker drives execution, shared `Arc<RwLock<Snapshot>>` for reads). No real ops wired yet — drive with a synthetic task in tests.

## Context the implementer must honor

- **Approach C**: one worker task owns the queue and runs tasks one at a time in FIFO order; it writes a snapshot into `Arc<RwLock<…>>`; query/event paths read the snapshot. One writer (worker), many readers.
- **Task model**: id, kind (enum — extended in CP-3/4), parent label, status (Queued/Planning/Downloading/Applying/Done/Failed/Cancelled), child items, current child, `done`/`total` counts.
- **Events**: emit `task://progress` (taskId, current child label, done/total, bytes) and `task://update` (task lifecycle/status). Payloads camelCase. Child label derives from `DownloadItem.dest` basename (`download.rs:43,47`) — only add a `url → label` map if the basename is insufficient.
- **Managed state**: register the manager via `.manage(...)` in `lib.rs:run()` alongside the existing registries (`lib.rs:1917-1971`).
- Commands: `list_tasks` (snapshot read), `cancel_task(id)`, plus an internal `enqueue`.
- **Never hold the `RwLock`/`Mutex` across an `.await`** — extract/clone then release.

## Success criteria

- [ ] Two enqueued synthetic tasks: the snapshot **never shows two tasks in a non-terminal status at once**, and tasks leave `Queued` in FIFO order.
- [ ] `cancel_task` moves a `Queued` or running task to `Cancelled` (running uses the CP-1 cancel seam).
- [ ] Snapshot reflects current child label + done/total while a task runs.
- [ ] Manager is in Tauri managed state; `list_tasks` returns the snapshot.

## Files

- `src-tauri/src/core/task_manager.rs` (+ `task_manager_tests.rs`)
- `src-tauri/src/lib.rs` (manage wiring + event payload structs)

## Verifies

`scripts/build.sh test task_manager` — serialization invariant, FIFO, cancel, snapshot.

## Out of scope

Wiring real pack/mod ops (CP-3/4); staging (CP-3); frontend mirror of payloads (CP-7).
