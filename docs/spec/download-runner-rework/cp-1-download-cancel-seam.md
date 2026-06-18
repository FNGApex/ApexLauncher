# CP-1 — Download engine cancel seam

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** — (independent; first wave)

## Goal

Give `execute_plan` (`src-tauri/src/core/download.rs:546`) a cancellation seam so a higher layer (CP-2 task worker) can stop an in-progress plan.

## Context the implementer must honor

- `execute_plan` builds a `FuturesUnordered` from **all** items upfront (`download.rs:592-609`), each acquiring a semaphore permit before downloading.
- Cancellation contract is **behavioral, not type-prescriptive** — do NOT mandate a specific return type. Required behavior: once the cancel signal trips, **no new permit is acquired and no new item download starts**. Items that already hold a permit (in-flight) MAY finish — that is acceptable. The caller must be able to **distinguish a cancelled run from a fully-completed one**.
- Leave existing `.part` resume + dedupe behavior intact for non-cancelled runs.

## Success criteria

- [ ] `execute_plan` accepts a cancel signal (implementer picks the mechanism).
- [ ] A plan tripped after the first item starts issues no further item downloads.
- [ ] The result lets the caller tell "cancelled" apart from "completed".
- [ ] All existing 37 `download_tests.rs` tests pass unchanged in behavior.

## Files

- `src-tauri/src/core/download.rs`
- `src-tauri/src/core/download_tests.rs`

## Verifies

`scripts/build.sh test core::download` — new unit test proves no further downloads after cancel + result distinguishability; existing 37 pass.

## Out of scope

Task queue, staging, events, any caller wiring. This CP only adds the seam + its test.
