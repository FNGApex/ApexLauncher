# Download Manager feedback + reliability (spec)

Status: **COMPLETE (2026-06-19) — all of D-F1..D-F4 shipped on `ui-overhaul`.** Commits: F1-1 4bc4359,
F1-2/F1-3 6b93920, F2-1 e4b6167 (deadlock caught+fixed), F2-2 687c145, F3 169894e, F4-1 e6bd7cd. F4-2
(concurrent task worker) deferred. **Pending: dev-window smoke-test.** Source: investigation 2026-06-19
(see `docs/handoff/post-ui-priorities.md` P2). Addressed: instant false "mod installed", silent failures,
frozen progress, thin logging, CF opt-out not surfaced on single-mod add, slow installs.

Root cause: `addMod`/`installModpack`/`updateMod` are enqueue commands (return a task id immediately);
results arrive async via `task://update`. The install UI treats enqueue as success and never observes the
terminal status, AND the backend `ModAddJob` reports `Done` even when nothing installed.

## Phases (each independently shippable)

### D-F1 — Honest task outcomes + failure/partial feedback (THE core bug)

**Terminal-status decision (locked 2026-06-19)** — `ModAddJob` builds `AddModResult { added, failed, manual, … }`, then:
- **Hard fail:** `added.is_empty()` AND `!failed.is_empty()` → `finish_failed(msg)` where msg names the count +
  first error. (Real download errors with nothing installed = red, honest.)
- **All-manual:** `added.is_empty()` AND `failed.is_empty()` AND `!manual.is_empty()` →
  `finish_done_with_result(result)` (NOT failed — the structured `manual` list with `page_url` must reach the
  frontend so the toast can offer "Open page"). The toast renders this as amber "needs manual download".
- **Partial / clean:** `!added.is_empty()` → `finish_done_with_result(result)` as today.

| CP | Deliverable | Files |
|----|-------------|-------|
| F1-1 | Apply the terminal-status decision above in `ModAddJob` (~lib.rs:1576-1594). Mirror the "fail when nothing happened" check in the pack/update siblings where they can no-op-fail (ModUpdate/ImportMrpack/ImportCfZip/UpdateModpack — check each: a pack import that resolved 0 files / all-failed should `finish_failed`). Add tests asserting failed-on-zero-added and done-on-all-manual. | `src-tauri/src/lib.rs` + sibling `lib_tests.rs` |
| F1-2 | `Toasts.tsx`: add a `failed` branch (red toast, `task.status.message`); for `done` results with `failed.length>0 || manual.length>0`, an amber "partial" toast ("added X · N failed · M need manual download"); when `manual` entries exist, an **Open page** action that opens each `manual[].pageUrl` (`@tauri-apps/plugin-opener` `openUrl`, pattern in `Browse.tsx`). Keep the existing green success toast for clean results. | `src/components/Toasts.tsx` |
| F1-3 | Folded into F1-2 (the global Toasts surface reads the terminal `AddModResult` and handles manual page-opening) — single-mod add no longer needs per-button result inspection. Verify the ModpackInstall/CfImport `manual` results also surface via the same toast. | `src/components/Toasts.tsx` (verify all result DTOs) |

### D-F2 — Live feedback (progress + button state)
| CP | Deliverable | Files |
|----|-------------|-------|
| F2-1 | Replace `NoOpSink` in the task download phase with a sink that drives `TaskContext::start_child`/`finish_child` (or emits `task://progress`) so `done/total` + the progress bar advance. | `src-tauri/src/lib.rs` (5 job sites: ~1531/1788/2051/2327/2762), `core/download.rs` sink wiring |
| F2-2 | Install buttons reflect LIVE task status from the store (spinner/"installing" until terminal; invalidate queries only on `Done`). Subscribe `ModSearchCard`/`ModRow` to the tasks slice by the returned task id (needs the task to carry the instance slug, or a client-side id map). | `src/routes/InstanceDetail.tsx`, maybe `src/lib/store.ts` |

### D-F3 — Logging (diagnostics)
| CP | Deliverable | Files |
|----|-------------|-------|
| F3-1 | Per-file download start/result logging with file name + mod/project id; log `manual` entries by name; add request/retry logging in `download_item`. | `src-tauri/src/core/download.rs` (~284, 744), `src-tauri/src/lib.rs` ModAddJob (~1586) |

### D-F4 — Slowness
| CP | Deliverable | Files |
|----|-------------|-------|
| F4-1 | Parallelize the dependency BFS: resolve each frontier of `required` deps concurrently instead of one-by-one sequential `get_versions` round-trips. | `src-tauri/src/core/mod_install.rs` (~180-261) + tests |
| F4-2 | (Optional) allow the task worker to run independent installs concurrently instead of strictly serial. | `src-tauri/src/core/task_manager.rs` (~470-522) — bigger change, defer unless needed |

## Follow-ups (tracked, not in current scope)
- **Failed-import rollback:** `ImportMrpackJob`/`ImportCfZipJob` create the instance early; on hard-fail
  (F1-1) the empty instance remains on disk (and listed in Home). Pre-existing (it was listed before too,
  just labeled Done). Consider deleting/rolling back the newly-created instance when an import hard-fails.
- Minor `clippy::useless_format` at a few job-site failure messages (non-blocking; gate is `cargo check`).

## Notes
- Backend changes follow the sibling `<stem>_tests.rs` convention; DTO changes (e.g. a "partial" result or
  task slug field) require regenerating `src/lib/bindings.ts` on the Windows dev window.
- D-F1 + D-F3 directly kill the "silent failure" + "false green" + "can't diagnose" complaints and are the
  highest value. D-F2 makes it feel live. D-F4 is the slowness tail.
