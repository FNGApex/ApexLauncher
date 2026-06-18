# TypeScript type generation from Rust (tauri-specta v2)

## Goal

Generate `src/lib/bindings.ts` (command signatures, DTOs, event payloads + typed listeners)
from the Rust backend using `tauri-specta` v2, retiring the hand-mirrored types in
`src/lib/ipc.ts` and `src/lib/store.ts`. Wire shapes stay camelCase-identical (no frontend
component churn). Decision + rationale: `docs/design/ts-type-generation.md`.

## Non-goals

- Generating the React components or TanStack Query hooks (only the type/IPC layer).
- Changing any wire shape, command name, or event channel name.
- Replacing the Zustand store behavior (reducers stay; only its type decls move to generated).
- Moving off the rc crate to a stable release (none exists yet).

## Success criteria

- [ ] `src/lib/bindings.ts` is generated from the 35 `#[tauri::command]`s and 8 event channels,
      committed to the repo, and never hand-edited.
- [ ] Generated DTO shapes match the current camelCase wire format (serde `rename_all`,
      `tag = "kind"` unions preserved) — no consuming component changes shape.
- [ ] `store.ts` type declarations are deleted and re-imported from `bindings.ts`; store
      reducers unchanged.
- [ ] `ipc.ts` becomes a thin adapter over generated `commands`/`events`; all hand-declared
      IPC interfaces removed.
- [ ] The `u64`-task-id deferred-result contract is preserved (task commands → `Promise<number>`;
      `TaskResult` union reachable on the `taskUpdate` event payload).
- [ ] Bindings regenerate under `scripts/build.sh test`; `scripts/build.sh check` is green at
      every checkpoint.
- [ ] Rust test count unchanged in spirit (the export test is additive); test layout follows
      the sibling `<stem>_tests.rs` convention.

## Approaches

See `docs/design/ts-type-generation.md` → "Approaches considered". Chosen: **tauri-specta v2**,
pinned `=2.0.0-rc.25` (specta `=2.0.0-rc.25`), export from a gated `#[test]`. (rc.21 does not
compose — tauri-specta rc.21 internally pins specta rc.22; both crates resolve cleanly only at
rc.25. Confirmed at CP-1.)

## Resolved decisions (confirmed with user, 2026-06-18)

- **Tool / rc:** accept tauri-specta v2 rc — full command + event + DTO generation; rc risk
  contained by exact pin + committing the generated `bindings.ts`.
- **Regeneration mechanism** *(revised at CP-2 — the original WSL-native assumption was wrong)*:
  the export goes through `make_builder()`, which constructs a `tauri_specta::Builder::<tauri::Wry>`.
  That type pulls in Tauri's windowing stack, which on Linux needs `webkit2gtk-4.1` — **not**
  installed on WSL — so the codegen does **not** compile WSL-native, contrary to the original
  plan. Actual mechanism: a `#[cfg(debug_assertions)]` `builder.export(...)` block in `run()`
  writes `src/lib/bindings.ts` at app startup. **Regenerate on Windows via `scripts/build.sh dev`**
  (start it, wait for the `[bindings] exported` line, stop it). A Linux-only `#[cfg(all(test,
  not(target_os = "windows")))]` export test (`bindings_export_tests.rs`) exports through the
  *same* `make_builder()` for future Linux CI — there is no second command list or export path.
- **File layout:** single committed generated `bindings.ts` + a thin hand-written `ipc.ts`
  adapter (invoke wrappers + event listeners) importing from it.

## Checkpoints

| # | Checkpoint | Files touched | Verifies | Done when |
|---|------------|---------------|----------|-----------|
| 1 | **Deps + resolution proof.** ✅ DONE. Add `specta = "=2.0.0-rc.25"` and `tauri-specta = { version = "=2.0.0-rc.25", features = ["derive", "typescript"] }` to `Cargo.toml` and prove they resolve + compile against Tauri 2. Builder swap deferred to CP-2 (coupled to the `#[specta::specta]` command macros). `generate_handler!` left intact. | `src-tauri/Cargo.toml` | `scripts/build.sh check` | Compiles green; deps resolve at rc.25; command registration unchanged |
| 2 | **Builder swap + command derives + export.** ✅ DONE. Swapped `tauri::generate_handler!` for the tauri-specta `Builder` via a single-source `make_builder()` (`collect_commands![34 cmds]` → `invoke_handler(builder.invoke_handler())`). `#[derive(specta::Type)]` on the IPC DTOs, `#[specta::specta]` on all 34 commands, `.dangerously_cast_bigints_to_number()` so u64 task ids → `number`. Export via the `#[cfg(debug_assertions)]` `builder.export()` block in `run()` (regen on Windows `scripts/build.sh dev`) + a Linux-only test through the *same* builder. Generated `src/lib/bindings.ts` (805 lines). camelCase parity ✓, `kind`-tagged unions ✓. **Deferred:** `Task.result: Option<serde_json::Value>` is `#[specta(skip)]` (type-erased) → typed result union is CP-3; `getRunState`/`AccountMeta` nominal split artifacts → CP-5. | `src-tauri/src/lib.rs`, `src-tauri/src/core/*.rs`, `bindings_export_tests.rs`, `src/lib/bindings.ts` | `scripts/build.sh check` + `test` (521 pass); regen via `scripts/build.sh dev` | `bindings.ts` has all 34 typed `commands.*`; camelCase matches `ipc.ts`; u64→number; build green |
| 3 | **Events + typed `TaskResult`.** ✅ DONE. Add `#[derive(specta::Type, tauri_specta::Event)]` to the 8 event payload types (`TaskProgressPayload`, `TaskUpdatePayload`, `RunUpdatePayload`, `LaunchLogPayload`, `LaunchExitPayload`, `InstallLogPayload`, `ProgressPayload`/download, `DeviceCodePayload`); populate `collect_events![...]`; switch the Tauri*Sink emitters to the generated `Event::emit` where it keeps the exact channel name (else keep `app.emit` with the channel literal + the generated payload type). **Also (folded from CP-2 review):** replace the `Task.result: Option<serde_json::Value>` carrier with a typed `TaskResult` enum (variants: the 6 result types `MrpackImportResult`/`CfImportResult`/`ModpackInstallResult`/`PackUpdateResult`/`AddModResult`/`UpdateModResult`), so `derive(specta::Type)` on those types becomes reachable, the `taskUpdate` payload carries a typed union (success criterion), and the `#[specta(skip)]` + `Task_Serialize|Task_Deserialize` split are retired. Regenerate via `scripts/build.sh dev`. | `src-tauri/src/lib.rs`, `src-tauri/src/core/task_manager.rs`, generated `src/lib/bindings.ts` | `scripts/build.sh check` + `test` | `events.*` typed listeners for all 8 channels; channel names unchanged; `TaskResult` union reachable on the taskUpdate payload; no `(task as any).result` needed; tests green |
| 4 | **Migrate `store.ts` to generated types.** ✅ DONE. Delete the hand-declared `Task`, `TaskKind`, `TaskStatus`, `ChildItem`, `TaskResult`, `TaskProgressUpdate`, `RunState`, `RunLogLine` decls; re-import the equivalents from `bindings.ts`. Keep all slices/reducers. Adapt field-name diffs if any (expect none — both camelCase). | `src/lib/store.ts` | `scripts/build.sh check` | `tsc` green with store types sourced from `bindings.ts`; reducers unchanged |
| 5 | **Convert `ipc.ts` to an adapter (commands).** ✅ DONE. Replace the hand-written `invoke<…>(...)` wrappers with calls into generated `commands.*`, preserving the existing exported function names + positional/`?? null` ergonomics that call sites depend on. Delete the now-redundant hand-declared command arg/return interfaces; re-export generated DTOs that components import by name. **Absorb the CP-2 nominal-split artifacts here:** `getRunState` returns an anonymous inline struct while `listRunning` returns named `RunInfoPayload` (specta inlines `Option<T>` returns); and `AccountMeta`/`Task` come through as `_Serialize|_Deserialize` unions (serde `alias`/asymmetry). The adapter normalizes these to the names call sites use (re-export aliases or thin wrappers) so consumers compile unchanged. | `src/lib/ipc.ts` | `scripts/build.sh check` | All command wrappers route through generated `commands`; no hand-declared command DTOs remain; routes/components compile unchanged |
| 6 | **Convert `ipc.ts` event helpers + final sweep.** Replace `listenDeviceCode`/`listenInstallLog`/`listenTaskProgress`/`listenRunUpdate`/`listenTaskUpdate` and the `*_EVENT` name constants with the generated `events.*.listen` surface (or thin re-exports preserving the old names for `AppShell`). Delete every remaining hand-declared IPC interface/payload type in `ipc.ts`. Confirm no orphaned type decls anywhere. | `src/lib/ipc.ts`, `src/components/AppShell.tsx` (if listener call shape changes) | `scripts/build.sh check` then `scripts/build.sh test` | Zero hand-declared IPC types remain; AppShell subscriptions use generated events; full check + test green |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| ~~rc won't resolve against installed Tauri 2 minor~~ — resolved | — | Resolved at CP-1: both crates pinned `=2.0.0-rc.25` (rc.21/rc.22 don't compose); compiles green against Tauri 2 |
| specta camelCase output differs subtly from current wire (e.g. enum repr) | med | CP-2/CP-3 diff generated `bindings.ts` against current `ipc.ts` shapes; `tsc` at CP-4/5 catches consumer mismatches |
| ~~WSL-native regen~~ — assumption wrong | resolved | Tauri `Builder<Wry>` needs webkit2gtk to compile → no WSL-native codegen. Regen on Windows via `scripts/build.sh dev` (debug-startup export). Linux-only test for future CI. See revised "Regeneration mechanism". |
| `tag="kind"` union (`ModpackInstallResult`) generates differently than the hand union | med | CP-2 verifies the generated discriminated union; adapter in CP-5 absorbs any cosmetic diff |
| Generated typed-event emit changes a channel name string | low | CP-3 keeps `app.emit` with the literal where the generated emit would rename; only the payload *type* is generated |
| rc crate yanked/broken later | low | Generated `bindings.ts` is committed → frontend build never depends on the crate resolving; only regeneration does |

## Change log

### 2026-06-18 — CP-5 done

`ipc.ts` rewritten as a thin adapter over generated `commands.*`. Every command
wrapper now routes through the generated surface; all hand-declared command DTOs
deleted and re-exported from `bindings.ts` under the names call sites import.
Exported function names, positional args, and `?? null` ergonomics preserved — no
route/component changed shape. Key adaptations:

- **`unwrap` helper** restores the historical reject-on-error contract: generated
  fallible commands return a `{ status: "ok" | "error" }` Result (they don't reject
  for non-`Error` payloads), so `unwrap` re-throws the `error` arm. The 5 infallible
  commands (`appInfo`/`listRunning`/`getRunState`/`getRunLogs`/`cancelLogin`) return a
  bare `Promise<T>` and are called directly.
- **`LoaderKind` stays frontend-owned** — specta erases the enum to `string` on
  `Loader.kind`, so the narrow union is kept and `LoaderOption` is re-narrowed over the
  generated shape (`getLoaders` casts). All other `*.kind` consumers (`labelLoader`,
  the `loaderKind: string` props, `Home`) already accept `string`.
- **Nominal-split artifacts absorbed (CP-2 carryover):** `getRunState`'s anonymous
  inline-struct return is annotated back to the named `RunInfoPayload | null`;
  `AccountMeta` is normalized to `AccountMeta_Serialize` (the generated union's
  serde-alias arm `{ mc_token_expires }` is malformed and never reaches the webview —
  the wire shape is always the serialize form); `InstanceSource` (generated as `Source`,
  unused by any component) dropped from the surface.
- **Event layer untouched** — payload interfaces, `*_EVENT` constants, and `listen*`
  helpers remain hand-written; CP-6 migrates them to generated `events.*`.

`scripts/build.sh check` green (`cargo check` + `tsc --noEmit`); Rust untouched (tests
unchanged at 521 — full suite runs at CP-6).

### 2026-06-18 — CP-4 done

`store.ts` type declarations migrated to generated `bindings.ts`. Deleted the
hand-mirrored `TaskKind`, `TaskStatus`, `ChildItem`, `TaskResult`, `Task`,
`TaskProgressUpdate`, `RunState`, `RunLogLine` decls (and the `@/lib/ipc` result-type
imports that fed the hand `TaskResult` union). Re-export `Task`/`TaskKind`/`TaskStatus`/
`ChildItem`/`TaskResult` straight from `bindings.ts` under the names consumers import
(AppShell, DownloadManager, Toasts compile unchanged). `RunState` and `RunLogLine` have
no single generated equivalent, so they are *composed* from generated types rather than
re-declared: `RunState = RunUpdatePayload & { elapsedMs?: number | null }` (the `run://update`
event omits `elapsedMs`; only `list_running` hydration carries it), `RunLogLine = RunLogPayload`.
The internal `patchTaskProgress` signature now takes the generated `TaskProgressPayload`
(the old `TaskProgressUpdate` alias had no external importers). All slices/reducers unchanged.
`scripts/build.sh check` green (`cargo check` + `tsc --noEmit`).

### 2026-06-18 — CP-3 done

Events + typed `TaskResult` landed. The 8 event payloads (`DeviceCodePayload`,
`ProgressPayload`, `LaunchLogPayload`, `LaunchExitPayload`, `RunUpdatePayload`,
`TaskProgressPayload`, `TaskUpdatePayload`, `InstallLogPayload`) gained
`#[derive(Deserialize, tauri_specta::Event)]` + `#[tauri_specta(event_name = "…")]`
(channel literals preserved); `collect_events![…]` populated on the single-source
`make_builder()`; `mount_events` wired in `run()`'s setup closure (builder cloned so
setup + invoke_handler each own a copy). Emitters left on `app.emit` with channel
literals — no channel rename. `Task.result` keeps its runtime `Option<serde_json::Value>`
carrier but the `#[specta(skip)]` is replaced by `#[specta(type = Option<crate::TaskResult>)]`,
and a new `#[serde(untagged)] TaskResult` enum (6 result variants) makes the typed union
reachable on the `taskUpdate` payload. `bindings.ts` regenerated: `events.*` typed
listeners for all 8 channels, `TaskResult` union present, `Task.result: TaskResult | null`,
no `(task as any).result` casts remain. `scripts/build.sh check` + `test` green (521 lib
tests). Deferred to CP-5 as planned: the `Task_Serialize | Task_Deserialize` nominal split
persists — it stems from `skip_serializing_if` asymmetry on `result`, not from the now-retired
`#[specta(skip)]`; fully collapsing it would change the wire shape, so the CP-5 adapter
normalizes it instead.

### 2026-06-17 — Initial spec

Authored the 6-checkpoint plan (deps/Builder → command derives+export test → events →
store migration → ipc command adapter → ipc event adapter + sweep), each gated on
`scripts/build.sh check`/`test`. Tool decision and rationale live in
`docs/design/ts-type-generation.md`. No implementation yet.

### 2026-06-18 — Resolved open questions

User confirmed all three pre-build decisions: accept the tauri-specta v2 rc, regenerate
`bindings.ts` WSL-native via `cargo test export_bindings` (no rsync-back), and keep a single
generated `bindings.ts` + thin `ipc.ts` adapter. Recorded under "Resolved decisions"; WSL
risk row closed. Ready for implementation.

### 2026-06-18 — CP-2 done

Builder swap + command derives + export landed. Single-source `make_builder()` (one
`collect_commands!`) drives the app's invoke handler, the debug-startup export, and the
Linux export test — no duplicate command list (a CP-2 review fix: the first cut had a second
`collect_functions!` list, removed). `bindings.ts` (805 lines): 34 typed commands, camelCase
parity with `ipc.ts`, u64→`number`, `kind`-tagged unions. `scripts/build.sh check` + `test`
green (521 pass). WSL-native regen assumption corrected (see Regeneration mechanism). Two
review findings deferred with explicit CP homes: typed `TaskResult` union → CP-3 (retires the
`serde_json::Value` skip and the `Task` S/D split); `getRunState`/`AccountMeta` nominal-split
artifacts → CP-5 adapter. Frontend (`ipc.ts`/`store.ts`) untouched — `bindings.ts` generated,
not yet consumed.

### 2026-06-18 — CP-1 done

Pinned both crates `=2.0.0-rc.25` (not rc.21: tauri-specta rc.21 internally requires specta
rc.22, and rc.22 of tauri-specta doesn't exist — the only cleanly-composing pair is rc.25).
`scripts/build.sh check` green; only a pre-existing `private_interfaces` warning remains.
CP-1 re-scoped to deps-only (resolution proof); the Builder swap moved into CP-2 because it is
coupled to the `#[specta::specta]` command macros. `generate_handler!` left intact this CP.
