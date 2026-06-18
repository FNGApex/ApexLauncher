# CP-6a — RunState model + status lifecycle + terminal retention

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** — (independent; first wave)
**Part of the sculpted CP-6 runner extension (6a→6b/6c→6d).**

## Goal

Replace the registry's `KillHandle` value with a richer `RunState` carrying a status, and implement the **running-process status lifecycle** — including recording the terminal status **in place** so a later page can recover it. This is the slice that **supersedes the existing "monitor is sole owner of registry removal" contract**; isolate it here so the change is reviewable on its own.

## Context the implementer must honor

- Today: `RunningRegistry = Mutex<HashMap<String, KillHandle>>` (`launch.rs:553`); `KillHandle { kill_tx }` (`launch.rs:536`); `monitor_child` **removes** the entry once the child exits (`launch.rs:778-781`).
- Change the value to `RunState { kill_tx, status, exit_code, started }` + a `RunStatus` enum. **Define all five variants now** — `Preparing`, `Running`, `Exited`, `Killed`, `Failed` (with `is_terminal`/`as_str`) — but this slice only drives `Running` (set at spawn) → `Exited`/`Killed`/`Failed`. `Preparing` is wired later in CP-6c; leave it defined-but-unused here.
- **Terminal retention (the superseded contract):** on child exit `monitor_child` now records the terminal status + exit code **into the retained entry** instead of removing it. Distinguish a killed exit (`Killed`) from a natural exit (`Exited`). Recovery of that state is what CP-6d exposes.
- **Reject-if-running** (`launch.rs:595-600`, `lib.rs:503-508`) keys off **status**, not mere key presence: reject only when an entry is `Running` (or `Preparing`); a terminal leftover is reusable for relaunch (overwrite/reset).
- Provide a **`pub(crate)` accessor** (e.g. `run_status(&registry, slug) -> Option<RunStatus>` and exit code) for tests + CP-6d to read — NOT a Tauri command yet (commands are CP-6d).
- **Never hold the registry lock across an `.await`** (`launch.rs:552`). Extract/clone, release, then await.
- Log ring, prep semaphore, new Tauri commands, `run://update` events → **out of scope** (6b/6c/6d).
- Existing test scaffolding: `CapturingLaunchSink` (module-scope `#[cfg(test)]`). Reuse it.

## Success criteria

- [ ] Registry value is `RunState`; `RunStatus` enum defined with all five variants + `is_terminal`.
- [ ] A natural child exit records `Exited` + the exit code in the **retained** entry (entry not removed).
- [ ] A killed child records `Killed` in the retained entry.
- [ ] After a terminal status, the same slug can be relaunched (reject-if-running blocks only `Running`/`Preparing`).
- [ ] A `pub(crate)` accessor returns the current status + exit code for a slug.
- [ ] Existing launch tests that asserted removal-on-exit are updated to the retention contract; all 36 prior launch tests pass (as adjusted).

## Files

- `src-tauri/src/core/launch.rs` (+ `launch_tests.rs`)

## Verifies

`scripts/build.sh test launch` — exit→Exited+retained, kill→Killed, relaunch-after-terminal, accessor reads status; existing tests green.

## Out of scope

Log ring (6b), prep semaphore + `Preparing` wiring (6c), Tauri commands + `run://update` events + payloads + managed-state changes (6d). No `lib.rs` command changes here beyond what the value-type swap forces to compile.
