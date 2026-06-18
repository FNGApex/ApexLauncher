# CP-6d — Runner surface: commands + events + payloads

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-6a (status accessor), CP-6b (log accessor), CP-6c (prep/Preparing)
**Part of the sculpted CP-6 runner extension — the frontend-facing surface.**

## Goal

Expose the runner state built in 6a/6b/6c to the frontend: read commands, a status event, and the serde payload structs CP-7 will mirror into `ipc.ts`.

## Context the implementer must honor

- Wrap the `pub(crate)` accessors from 6a/6b as **Tauri commands**: `list_running() -> RunInfoPayload[]`, `get_run_state(slug) -> RunInfoPayload | null`, `get_run_logs(slug) -> RunLogPayload[] | null`. Register them in the `invoke_handler` in `lib.rs:run()`.
- Add a **`run://update`** event emitted on every status transition (Preparing/Running/Exited/Killed/Failed). Emit from the transition points in 6a (spawn/monitor) and 6c (prep) — route emission through the existing `LaunchSink` seam by adding a **default-noop `status` method** to the `LaunchSink` trait (so prep/spawn/monitor emit through one path; the Tauri sink implements it, `CapturingLaunchSink` captures it for tests).
- **Payload structs (serde `rename_all = "camelCase"`):**
  - `RunUpdatePayload { slug, status, exitCode? }`
  - `RunInfoPayload { slug, status, exitCode?, elapsedMs }`
  - `RunLogPayload { stream, line }`
  - `status` tag values: `"preparing" | "running" | "exited" | "killed" | "failed"`.
- This CP adds the Rust side only. The matching `ipc.ts` mirror + store wiring is **CP-7** — but list the exact command signatures + payload field names in your report so CP-7 has them.

## Success criteria

- [ ] `list_running` enumerates current (non-removed) instances with status + elapsed.
- [ ] `get_run_state`/`get_run_logs` return the retained state / buffered lines for a slug, `null` for unknown.
- [ ] A status transition emits `run://update` with the correct tag + exit code.
- [ ] `LaunchSink` gained a default-noop `status` method; the capturing test sink records transitions.
- [ ] `scripts/build.sh check` passes (lib.rs whole); existing launch tests pass.

## Files

- `src-tauri/src/core/launch.rs` (+ `launch_tests.rs`)
- `src-tauri/src/lib.rs` (commands, payloads, `invoke_handler` registration, event emission)

## Verifies

`scripts/build.sh test launch` (list/get/logs return shapes, status event captured) + `scripts/build.sh check`.

## Out of scope

`ipc.ts` mirror + Zustand store + any frontend (CP-7). Report the command signatures + payload names for CP-7.
