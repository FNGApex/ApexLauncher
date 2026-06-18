# CP-4 — Mod ops as tasks + FS fast-path

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-2 (task manager), CP-3 (stage-and-promote helper)

## Goal

Route download-bearing mod ops through the task queue; keep cheap FS ops instant and off the queue.

## Context the implementer must honor

- **Download-bearing** — `add_mod`, `update_mod`: enqueue a task (reuse the CP-3 stage-and-promote helper), return a task id synchronously, terminal result via `task://update`.
- **Instant fast-path** — `set_mod_enabled`, `remove_mod`: no download, so they **do NOT enter the serial queue**. They run immediately even while a download task is busy, and emit a store-visible state change (a `task://update`-style notification or a dedicated lightweight signal — implementer's call, but it must reach the store without blocking on the queue).
- **`pack_locked` guard** stays enforced on all four ops (`instances::ensure_not_locked`).

## Success criteria

- [ ] `add_mod`/`update_mod` enqueue the correct task kind and return a task id.
- [ ] `set_mod_enabled`/`remove_mod` complete **without enqueuing a task** and are **not blocked** by a busy download queue.
- [ ] A `pack_locked` instance rejects the guarded ops as before.
- [ ] Existing `mod_install_tests.rs` pass.

## Files

- `src-tauri/src/lib.rs`
- `src-tauri/src/core/mod_install.rs`
- `src-tauri/src/core/task_manager.rs` (task kinds)

## Verifies

`scripts/build.sh test mod_install` + `task_manager` — add/update enqueue; enable/disable/remove bypass the queue; pack-lock rejection.

## Out of scope

Frontend wiring (CP-7); pack ops (CP-3).
