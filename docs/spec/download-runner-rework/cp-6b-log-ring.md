# CP-6b — Capped log ring + replay

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-6a (RunState)
**Part of the sculpted CP-6 runner extension.**

## Goal

Buffer recent log lines per running instance so a page that navigates away and back can replay them (Tauri events are unbuffered global broadcasts — lines emitted while no listener is attached are otherwise lost).

## Context the implementer must honor

- Add a **capped** `log_ring` to `RunState` (CP-6a): a `VecDeque<LogLine>` with a fixed cap (~1000 lines); pushing past the cap drops the oldest. `LogLine { stream, line }` (stream = stdout/stderr).
- `monitor_child` already drains per-stream channels and emits `launch://log` (`launch.rs` monitor loop). In addition to emitting, **write each line into the ring** of the retained entry. A single helper (e.g. `push_and_emit`) keeps emit + buffer in one place.
- Provide a **`pub(crate)` accessor** returning a snapshot (clone) of an instance's buffered lines for tests + CP-6d. Not a Tauri command yet.
- **Never hold the registry lock across an `.await`** — push under a short synchronous lock, release before awaiting the next channel item.
- Prep-phase `install://log` buffering is **out of scope here** — it arrives with the prep wiring in CP-6c (which records prep logs into the same ring).

## Success criteria

- [ ] `RunState.log_ring` caps at the chosen bound; pushing past it drops the oldest line (FIFO).
- [ ] `monitor_child` writes every emitted log line into the ring of the retained entry.
- [ ] An accessor replays the buffered lines (in order) for a slug, including lines emitted before a terminal exit.
- [ ] Existing launch tests pass.

## Files

- `src-tauri/src/core/launch.rs` (+ `launch_tests.rs`)

## Verifies

`scripts/build.sh test launch` — ring caps + drops oldest; buffered lines replay in order post-exit.

## Out of scope

Prep-phase log capture (6c), the Tauri `get_run_logs` command + payload (6d).
