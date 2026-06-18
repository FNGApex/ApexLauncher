# CP-3 — Pack ops as tasks + stage-and-promote

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-1 (cancel seam), CP-2 (task manager)

## Goal

Add a stage-and-atomic-promote helper, then route the four pack commands through the task manager as hierarchical tasks.

## Context the implementer must honor

- **Stage-and-promote**: instance-bound downloads write into a **same-volume temp staging dir**, then **atomic-promote (rename)** into the instance on success. Cancel/fail discards the staging dir → the instance is never partially mutated. Same-volume is required for the rename to be atomic. Cache-bound shared artifacts (libraries, version jars, assets in `cache/`) are NOT staged — they keep in-place `.part` resume + dedupe.
- **Command-contract change**: `install_modpack`, `update_modpack`, `import_mrpack`, `import_curseforge_zip` now **enqueue a task and return a task id synchronously**. Two phases: PLAN (fetch manifest / resolve — sets parent label e.g. the pack name + builds the child queue) then EXECUTE (cancellable `execute_plan` from CP-1 into staging → promote).
- The **terminal `task://update` carries the same result fields these commands return inline today** (e.g. `ModpackInstallResult`, incl. the new instance id/slug).
- Reuse `instances::create`/`load_manifest`/`save_manifest`, `modpack::plan_pack_update`/`build_*_plan`, `download::execute_plan`, `CurseForgeProvider::get_file`.

## Success criteria

- [ ] Each of the four commands enqueues a task of the correct kind and returns a task id without awaiting completion.
- [ ] After the plan phase, the task exposes a labeled child queue (parent = pack name, children = mod file names).
- [ ] Cancelling mid-download leaves the instance tree unmodified (staging discarded).
- [ ] The terminal event payload **deserializes and asserts a representative result field present** (e.g. the new instance id/slug).
- [ ] Existing `modpack_tests.rs` pass.

## Files

- `src-tauri/src/lib.rs`
- `src-tauri/src/core/modpack.rs`
- `src-tauri/src/core/task_manager.rs` (task kinds)

## Verifies

`scripts/build.sh test modpack` + `task_manager` — enqueue + labeled queue + cancel-leaves-clean + terminal payload field asserted.

## Out of scope

Mod ops (CP-4); frontend consumption of the task id / terminal event (CP-7).
