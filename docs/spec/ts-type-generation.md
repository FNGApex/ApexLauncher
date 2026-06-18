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
- **WSL generated-file round-trip:** **regenerate WSL-native** via `cargo test export_bindings`
  (codegen only parses/derives — no GTK/WebKit, so it compiles WSL-native even though a real
  build can't). The export test writes `src/lib/bindings.ts` directly into the WSL source tree;
  no rsync-back from the Windows mirror. The authoritative full build/test still goes through
  `scripts/build.sh` on Windows.
- **File layout:** single committed generated `bindings.ts` + a thin hand-written `ipc.ts`
  adapter (invoke wrappers + event listeners) importing from it.

## Checkpoints

| # | Checkpoint | Files touched | Verifies | Done when |
|---|------------|---------------|----------|-----------|
| 1 | **Deps + resolution proof.** ✅ DONE. Add `specta = "=2.0.0-rc.25"` and `tauri-specta = { version = "=2.0.0-rc.25", features = ["derive", "typescript"] }` to `Cargo.toml` and prove they resolve + compile against Tauri 2. Builder swap deferred to CP-2 (coupled to the `#[specta::specta]` command macros). `generate_handler!` left intact. | `src-tauri/Cargo.toml` | `scripts/build.sh check` | Compiles green; deps resolve at rc.25; command registration unchanged |
| 2 | **Builder swap + derive + collect commands; export test.** Replace the bare `tauri::generate_handler!` with the tauri-specta `Builder` (`collect_commands![...]` → `builder.invoke_handler(...)`); this is where the Builder swap lands (folded from CP-1, since it needs the command macros). Add `#[derive(specta::Type)]` to every IPC DTO (28 lib.rs payload/result structs + the core structs reachable as command args/returns) and `#[specta::specta]` to all 34 commands; populate `collect_commands![...]` with the full list from the old `generate_handler!`. Add a gated export `#[test]` (sibling `lib_tests.rs` or new `bindings_export_tests.rs` per test-layout convention) calling `builder.export(Typescript::default(), "../src/lib/bindings.ts")`. Regenerate WSL-native: `cargo test export_bindings` (writes into the WSL tree). | `src-tauri/src/lib.rs`, `src-tauri/src/core/*.rs` (derive adds), new/edited `*_tests.rs`, generated `src/lib/bindings.ts` | WSL-native `cargo test export_bindings` (emits file) then `scripts/build.sh check` | `bindings.ts` exists with all 35 typed `commands.*`; camelCase shapes match current `ipc.ts`; cargo tests green |
| 3 | **Events: derive + collect + typed listeners.** Add `#[derive(specta::Type, tauri_specta::Event)]` to the 8 event payload types (`TaskProgressPayload`, `TaskUpdatePayload`, `RunUpdatePayload`, `LaunchLogPayload`, `LaunchExitPayload`, `InstallLogPayload`, `ProgressPayload`/download, `DeviceCodePayload`); populate `collect_events![...]`; switch the Tauri*Sink emitters to the generated `Event::emit` where it keeps the exact channel name (else keep `app.emit` with the channel literal and rely only on the generated payload type). Regenerate WSL-native (`cargo test export_bindings`). | `src-tauri/src/lib.rs`, generated `src/lib/bindings.ts` | WSL-native `cargo test export_bindings` then `scripts/build.sh check` | `events.*` typed listeners present for all 8 channels; channel name strings unchanged; tests green |
| 4 | **Migrate `store.ts` to generated types.** Delete the hand-declared `Task`, `TaskKind`, `TaskStatus`, `ChildItem`, `TaskResult`, `TaskProgressUpdate`, `RunState`, `RunLogLine` decls; re-import the equivalents from `bindings.ts`. Keep all slices/reducers. Adapt field-name diffs if any (expect none — both camelCase). | `src/lib/store.ts` | `scripts/build.sh check` | `tsc` green with store types sourced from `bindings.ts`; reducers unchanged |
| 5 | **Convert `ipc.ts` to an adapter (commands).** Replace the hand-written `invoke<…>(...)` wrappers with calls into generated `commands.*`, preserving the existing exported function names + positional/`?? null` ergonomics that call sites depend on. Delete the now-redundant hand-declared command arg/return interfaces; re-export generated DTOs that components import by name. | `src/lib/ipc.ts` | `scripts/build.sh check` | All command wrappers route through generated `commands`; no hand-declared command DTOs remain; routes/components compile unchanged |
| 6 | **Convert `ipc.ts` event helpers + final sweep.** Replace `listenDeviceCode`/`listenInstallLog`/`listenTaskProgress`/`listenRunUpdate`/`listenTaskUpdate` and the `*_EVENT` name constants with the generated `events.*.listen` surface (or thin re-exports preserving the old names for `AppShell`). Delete every remaining hand-declared IPC interface/payload type in `ipc.ts`. Confirm no orphaned type decls anywhere. | `src/lib/ipc.ts`, `src/components/AppShell.tsx` (if listener call shape changes) | `scripts/build.sh check` then `scripts/build.sh test` | Zero hand-declared IPC types remain; AppShell subscriptions use generated events; full check + test green |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| ~~rc won't resolve against installed Tauri 2 minor~~ — resolved | — | Resolved at CP-1: both crates pinned `=2.0.0-rc.25` (rc.21/rc.22 don't compose); compiles green against Tauri 2 |
| specta camelCase output differs subtly from current wire (e.g. enum repr) | med | CP-2/CP-3 diff generated `bindings.ts` against current `ipc.ts` shapes; `tsc` at CP-4/5 catches consumer mismatches |
| WSL one-way mirror loses the generated `bindings.ts` on the WSL side | resolved | Regenerate WSL-native via `cargo test export_bindings` (codegen compiles WSL-native); test writes into the WSL tree directly — no rsync-back. See Resolved decisions. |
| `tag="kind"` union (`ModpackInstallResult`) generates differently than the hand union | med | CP-2 verifies the generated discriminated union; adapter in CP-5 absorbs any cosmetic diff |
| Generated typed-event emit changes a channel name string | low | CP-3 keeps `app.emit` with the literal where the generated emit would rename; only the payload *type* is generated |
| rc crate yanked/broken later | low | Generated `bindings.ts` is committed → frontend build never depends on the crate resolving; only regeneration does |

## Change log

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

### 2026-06-18 — CP-1 done

Pinned both crates `=2.0.0-rc.25` (not rc.21: tauri-specta rc.21 internally requires specta
rc.22, and rc.22 of tauri-specta doesn't exist — the only cleanly-composing pair is rc.25).
`scripts/build.sh check` green; only a pre-existing `private_interfaces` warning remains.
CP-1 re-scoped to deps-only (resolution proof); the Builder swap moved into CP-2 because it is
coupled to the `#[specta::specta]` command macros. `generate_handler!` left intact this CP.
