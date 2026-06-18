# TypeScript type generation from Rust

## Goal

Retire the standing IPC type-drift risk. Today `src/lib/ipc.ts` (~1040 lines) and
`src/lib/store.ts` hand-mirror every Rust IPC struct in camelCase (driven by serde
`rename_all = "camelCase"`). Any Rust struct change is a manual two-place edit that no
tool checks. We want the wire types (command args, command returns, event payloads)
**generated from the Rust source of truth** so the compiler catches drift, while keeping
the project's "always build via `scripts/build.sh`" rule and the existing camelCase wire
shapes intact (zero churn to consuming components).

## Decision

**Adopt `tauri-specta` v2 (pinned `=2.0.0-rc.25`, paired specta `=2.0.0-rc.25` — rc.21/rc.22
don't compose, confirmed at CP-1), generating a single `src/lib/bindings.ts` from a
`#[test]`/debug export hook.** The hand-written
`ipc.ts` wrappers and `store.ts` mirrors are replaced by (a) generated `commands.*` +
`events.*` typed surfaces and (b) thin hand-written adapters that preserve the project's
existing ergonomic call sites.

### Why tauri-specta over ts-rs

The deciding factor is **events**. This app's whole async contract rides Tauri event
channels — `task://progress`, `task://update`, `run://update`, `launch://log`,
`launch://exit`, `install://log`, `download://progress`, `auth://device-code` — and the
task-queue contract (commands return a `u64` task id; the real result arrives later on
`task://update`). `ts-rs` generates types for plain structs/enums only; it has **no model
of commands or events** (verdict from docs.rs — "doesn't address framework-specific
integration"). With ts-rs we would still hand-maintain: which command takes/returns which
type, which event carries which payload, and the `invoke()` wrappers. That leaves the
exact drift surface we are trying to kill (the binding between a name string and its type)
unprotected.

`tauri-specta` v2 models **both commands and events end to end**: `collect_commands!` ties
each `#[tauri::command]` to its generated arg/return types, and `collect_events!` +
`#[derive(Event)]` generates a typed `events.taskUpdate.listen(cb)` surface. specta (its
type engine) honors serde `rename_all` and `tag`-based enums, so the generated shapes
match the current wire format. This collapses the drift surface to **zero hand-maintained
type↔name bindings**.

`ts-rs` would be the pick if we only needed plain DTO types and wanted a stable, non-rc
crate. We don't — the event + command surface dominates, and re-deriving the command/event
wiring by hand defeats the purpose.

### The rc cost, accepted

tauri-specta v2 is a **release candidate** (`2.0.0-rc.25` latest as of research), not a
stable release, and specta's low-level APIs are flagged "still experimental." This is the
real downside. It is acceptable here because:

- Generated output is committed to the repo (`bindings.ts` is checked in), so a future
  crate yank/break never blocks the frontend build — only regeneration.
- The version is **pinned exactly** (`=2.0.0-rc.21` for both crates) per upstream guidance,
  so `cargo update` cannot silently churn it.
- rc.20+ **unlocked the Tauri version dependency**, so it composes with our open
  `tauri = "2"` without forcing a `tauri = "=2.x"` pin (this was a hard blocker in earlier
  rc's and is the reason for the specific floor).
- It is dev/build tooling, not shipped runtime code; a regression degrades the type-gen
  workflow, not the app.

We pin `rc.21` as the floor (first rc at/after the Tauri-unlock that the docs example
targets); the builder may bump to the newest rc that still composes if `rc.21` fails to
resolve against the installed Tauri 2 minor.

## Evidence trail

| Claim | Source | Verdict |
|-------|--------|---------|
| tauri-specta latest is `2.0.0-rc.25`, an rc (not stable) | github.com/specta-rs/tauri-specta/releases | supported |
| tauri-specta v2 supports typed events (since rc.16, "generic events") | release notes | supported |
| rc.20 unlocked the Tauri version dep (composes with open `tauri="2"`) | release notes | supported |
| specta paired version is `=2.0.0-rc.x` (locked to tauri-specta rc) | release notes | supported |
| Builder pattern: `.commands(collect_commands![...]).events(collect_events![...])` + `Builder::export(Typescript::default(), path)` | docs.rs/tauri-specta/2.0.0-rc.21 | supported |
| Event types use `#[derive(..., Type, Event)]`; generates `events.x.listen(cb)` TS surface | docs.rs/tauri-specta/2.0.0-rc.21 | supported |
| Export can run from a `#[test]` (cargo-test idiom) or a `#[cfg(debug_assertions)]` setup hook | tauri-specta v2 docs + crates.io examples | supported |
| ts-rs latest is `12.0.1` (2026-01-31), MSRV 1.88 | crates.io/docs.rs | supported |
| ts-rs honors serde `rename_all`, `tag`, `content`, `untagged` via default `serde-compat` | docs.rs/ts-rs | supported |
| ts-rs `#[ts(export)]` emits a generated test that writes bindings on `cargo test` | docs.rs/ts-rs | supported |
| ts-rs has no command/event model (plain types only) | docs.rs/ts-rs ("doesn't address framework-specific integration") | supported |
| Repo: 35 `#[tauri::command]`s, 28 payload/result structs in lib.rs, 15 core files with `rename_all`, ~146 derive sites | grep over `src-tauri/src` | supported |

## What gets generated vs hand-written

**Generated** (into `src/lib/bindings.ts`, committed, never hand-edited):

- Every IPC DTO: `Instance`, `ModEntry`, `ProjectSummary`, `ProjectVersion`, `Task`,
  `TaskStatus`, `TaskResult` union, `AddModResult`, `MrpackImportResult`, `CfImportResult`,
  `ModpackInstallResult` (the `tag = "kind"` union), `PackUpdateResult`, `LaunchMeta`,
  `DownloadPlan`/`DownloadItem`, etc.
- The 35 command signatures: a generated `commands.addMod(...)` etc. that wraps `invoke`
  with the correct arg object and return type.
- The 8 event payload types + typed listeners: `events.taskProgress`, `events.taskUpdate`,
  `events.runUpdate`, `events.launchLog`, `events.launchExit`, `events.installLog`,
  `events.downloadProgress`, `events.authDeviceCode`.

**Hand-written** (thin, behavioral only — no type re-declaration):

- A small `src/lib/ipc.ts` **adapter** that re-exports generated commands under the existing
  function names (`searchMods`, `addMod`, …) where call-site ergonomics differ from the
  generated shape (e.g. positional args vs the generated single-object arg, the
  `?? null` optional-coalescing the current wrappers do). Goal: consuming components in
  `routes/` and `components/` import the same names and keep compiling.
- `src/lib/store.ts` **behavior**: the Zustand slices, reducers (`upsertTask`,
  `patchTaskProgress`, `upsertRun`, …) stay. Only the **type declarations** at the top
  (`Task`, `TaskStatus`, `TaskResult`, `RunState`, `TaskProgressUpdate`, …) are deleted and
  re-imported from `bindings.ts`.

The task-queue contract is preserved structurally: the `u64`-returning commands generate
as `Promise<number>`; the deferred result type (`TaskResult` union) is generated from the
Rust enum and reachable via the generated `Task` type on the `taskUpdate` event. No special
handling — it falls out of generating both halves from the same source.

## Build wiring

Export runs as a **gated `#[test]`** (e.g. `export_bindings` in a sibling
`bindings_export_tests.rs`, per the repo's test-layout convention) that calls
`Builder::export(Typescript::default(), "../src/lib/bindings.ts")`. This fits the existing
toolchain with no new build step:

- `scripts/build.sh test` already runs the full Rust suite on the native Windows toolchain;
  the export test runs there and writes `src/lib/bindings.ts` (path relative to
  `src-tauri/`). On WSL the file lands in the mirrored Windows tree; the builder copies/
  confirms it back into the WSL working tree (rsync mirror is one-way WSL→Windows, so the
  generated file must be regenerated or pulled back — see Open question 1).
- `scripts/build.sh check` (`cargo check` + `tsc --noEmit`) verifies the generated file
  type-checks against its consumers. Drift now surfaces as a `tsc` error, which was the
  whole objective.
- The `Builder` is also installed into the real app via `.invoke_handler(builder.invoke_handler())`
  and `builder.mount_events(app)` so the runtime registration and the generated types come
  from the *same* `collect_commands!`/`collect_events!` lists — single source of truth.

This is preferred over the `#[cfg(debug_assertions)]` setup-hook variant because the test
form runs deterministically under `scripts/build.sh test` (CI-friendly, no app launch) and
matches the repo's existing test-as-codegen patterns.

## Migration approach

Incremental, compiler-guarded — never a big-bang rewrite:

1. Add deps + the `Builder` + `#[specta::specta]`/`Type`/`Event` derives. Wire all 35
   commands + 8 events into the collect macros. Generate `bindings.ts`. At this point both
   the generated file and the legacy `ipc.ts`/`store.ts` coexist.
2. Repoint `store.ts` type imports to `bindings.ts`; delete the duplicated type decls. `tsc`
   proves the generated shapes match what the store consumed.
3. Convert `ipc.ts` into an adapter over generated `commands`/`events`, one domain block at
   a time, deleting hand-declared interfaces as each is replaced. Each step is `tsc`-green
   before the next.
4. Delete the now-dead hand-declared types and the standalone `listen*` wrappers superseded
   by `events.*.listen`.

Because every step ends `scripts/build.sh check`-green, the migration can stop/resume at any
checkpoint without a broken frontend.

## Approaches considered

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | tauri-specta v2 (commands + events generated) | `Builder` + `collect_commands!`/`collect_events!`, export from test | med | med — rc crate, but pinned + committed output |
| B | ts-rs for DTOs; keep hand-written command/event wiring | `#[ts(export)]` on structs; wrappers stay by hand | med | high — leaves the name↔type binding (the actual drift) unprotected |
| C | Status quo (hand-mirror) | nothing | none now | high ongoing — silent drift, the problem statement |
| D | Custom build.rs codegen | parse/emit ourselves | high | high — reinvents specta, more to maintain |

**Chosen: A.** Only A protects the full surface (commands + events + the task-id deferred-
result contract) that defines this app's IPC. B's residual hand-wiring is exactly the drift
we're paid to kill. C is the problem. D is unjustified cost.

## Open questions for approval

1. **WSL generated-file round-trip.** `scripts/build.sh` mirrors WSL→Windows one-way and the
   export test writes on the Windows side. Do we (a) add a rsync-back of `src/lib/bindings.ts`
   to the WSL tree after `test`, (b) make the export test write to a path that round-trips,
   or (c) regenerate WSL-native via `cargo test export_bindings` (compiles fine for codegen)?
   This needs a one-line `build.sh` decision before CP-1.
2. **rc pin floor.** OK to pin `=2.0.0-rc.21` (and bump only if it won't resolve against the
   installed Tauri 2 minor), accepting rc instability for the drift-safety win?
3. **Single file vs split.** One `bindings.ts`, or split generated DTOs from the adapter? Spec
   assumes a single committed `bindings.ts` + a thin `ipc.ts` adapter.

## Change log

### 2026-06-17 — Initial design

Authored the tool decision (tauri-specta v2 over ts-rs, driven by the events + command
surface), evidence trail (crate versions verified against release notes / docs.rs / crates.io
as of research), generated-vs-hand-written split, test-time export wiring, and the
incremental compiler-guarded migration. No implementation yet.
