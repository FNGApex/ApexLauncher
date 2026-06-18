# CP-6c — Prep serialization + Preparing phase

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-6a (RunState + status)
**Part of the sculpted CP-6 runner extension.**

## Goal

Serialize the blocking prep phase of a launch (resolve → download → materialize → natives) across concurrent launches, while letting N packs **run** concurrently once spawned. Drive the `Preparing` status defined in CP-6a.

## Context the implementer must honor

- `launch_instance` (`lib.rs:485-655`) runs a long sequential prep, then `spawn_instance`. Today nothing serializes prep.
- Add a prep **`Semaphore(1)`** as managed state (a new managed type, e.g. `PrepSemaphore` + constructor; register it in `lib.rs:run()` alongside the existing registries `lib.rs:1917-1971`). `launch_instance` **acquires the permit before prep and holds it across the whole prep**, then **releases it once the JVM has spawned** (drop the permit right after `spawn_instance` returns) — so a second launch waits only for prep, not for the first pack to exit.
- On entering prep, mark the instance `Preparing` in the registry (CP-6a status). On successful spawn, transition `Preparing → Running` **preserving `started`** (and any prep-phase log lines).
- **Prep-failure cleanup:** any error during prep must leave no phantom `Preparing` entry — mark it `Failed`. Prefer an RAII guard (a `Drop` impl that marks `Failed` unless disarmed on success) over hand-editing every `?` site, to keep the prep body un-reindented and guarantee cleanup on every early return.
- Prep-phase logs: the forge/neoforge installer emits `install://log` (no correlation id). Since prep is now serialized there is exactly one `Preparing` instance, so record those lines into **that** instance's log ring (CP-6b). This attribution applies **only during prep**.
- **Never hold the registry lock across an `.await`.**

## Success criteria

- [ ] A prep `Semaphore(1)` is managed state; `launch_instance` holds it across prep and releases at spawn.
- [ ] Two near-simultaneous launches: the registry **never shows two instances `Preparing` at once**; the second proceeds once the first reaches `Running`; both then reach `Running` concurrently.
- [ ] A prep error marks the instance `Failed` and leaves no `Preparing` entry; the permit is released (next launch proceeds).
- [ ] Prep-phase `install://log` lines land in the preparing instance's log ring.
- [ ] Existing launch tests pass.

## Files

- `src-tauri/src/core/launch.rs` (+ `launch_tests.rs`)
- `src-tauri/src/lib.rs` (semaphore managed state + `launch_instance` wiring)

## Verifies

`scripts/build.sh test launch` (never-two-Preparing, both-reach-Running, prep-fail→Failed+permit-released) + `scripts/build.sh check` (lib.rs typechecks whole).

## Out of scope

The read commands + `run://update` events + payload structs (6d). Log ring internals (6b — reuse its accessor/helper).
