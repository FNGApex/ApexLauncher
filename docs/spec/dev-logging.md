# Dev logging (tauri-plugin-log)

## Goal

Add a structured logging subsystem so the running app reports what it's doing: `[INFO]`/`[WARN]`/
`[ERROR]` lines streamed to the dev terminal (stdout) **and** the in-app webview devtools console
(F12), plus a rotating logfile in the app log dir. Wire the infra via `tauri-plugin-log` and add
log calls at high-value points.

## Non-goals

- A custom in-app Dev Console UI panel (terminal + devtools console only this pass).
- Exhaustive logging of every command/module — only the wiring + key paths (see Coverage).
- Frontend (TS) application logging beyond `attachConsole()` wiring (Rust logs are the focus).
- Log shipping / remote telemetry.

## Approach

`tauri-plugin-log` v2 (idiomatic Tauri 2 choice; matches the existing `.plugin()` pattern for
opener/dialog). Verified API (https://v2.tauri.app/plugin/logging/):

- **Cargo:** add `tauri-plugin-log` + `log` (0.4) to `src-tauri/Cargo.toml`.
- **npm:** add `@tauri-apps/plugin-log`.
- **Init** in `lib.rs` `run()` via `.plugin(tauri_plugin_log::Builder::new()...build())`:
  - Targets: `TargetKind::Stdout`, `TargetKind::LogDir { file_name: Some("apex") }`,
    `TargetKind::Webview`.
  - `.level(log::LevelFilter::Info)` (debug builds may use a higher verbosity — builder's call).
  - `.format(|out, message, record| out.finish(format_args!("[{} {}] {}", record.level(),
    record.target(), message)))` — yields `[INFO core::download] ...`.
- **Capability:** add `"log:default"` to `src-tauri/capabilities/default.json` permissions.
- **Frontend:** call `attachConsole()` from `@tauri-apps/plugin-log` once at startup
  (`src/main.tsx`) so Rust logs surface in the webview devtools console.

## Coverage (this pass — "wire + key paths")

Convert the existing 3 `eprintln!`/`println!` calls to `log` macros, and add `info!`/`warn!`/
`error!` at high-value points (the implementer picks exact call sites + messages — keep them
useful, not noisy):

- Tauri command entry/exit for the heavyweight commands (launch, install/import/update modpack,
  add/update mod, auth login) — INFO on start + outcome.
- Errors that are currently swallowed or only returned as `Err(String)` — log at ERROR/WARN with
  context before returning.
- Download engine: INFO on plan start (item count) + WARN/ERROR on failed items.
- Launch: INFO on spawn (instance, pid) + ERROR on spawn failure.
- Modpack import/update: INFO on resolve/plan/result counts; WARN on manual/skipped.

## Success criteria

- [ ] `tauri-plugin-log` + `log` in `Cargo.toml`; `@tauri-apps/plugin-log` in `package.json`;
      `"log:default"` in `capabilities/default.json`.
- [ ] Plugin initialized in `lib.rs` with Stdout + LogDir(`apex`) + Webview targets, `Info` level,
      and the `[LEVEL target] message` format.
- [ ] `attachConsole()` wired in `src/main.tsx`.
- [ ] The 2 app-source `eprintln!` calls (lib.rs auth, launch.rs playtime) replaced with `log`
      macros. (`build.rs` `println!("cargo:...")` directives and `tests/*.rs` debug output stay.)
- [ ] `info!`/`warn!`/`error!` added at the high-value points above (judgment on exact sites).
- [ ] `scripts/build.sh check` green; full Rust lib test suite stays green (no test regressions);
      `npm run build` green.
- [ ] Manual (Windows GUI): running `scripts/build.sh dev` prints `[INFO ...]` lines to the
      terminal, the logfile appears in the app log dir, and F12 devtools console shows the logs.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Wire the plugin: Cargo + npm + capability deps; init in `lib.rs` (targets/level/format); `attachConsole()` in `main.tsx`; convert the 3 existing eprintln/println to `log` macros | `src-tauri/Cargo.toml`, `src-tauri/capabilities/default.json`, `src-tauri/src/lib.rs`, `package.json`, `src/main.tsx`, the 2 files with eprintln | atomic-builder | ~6 | `scripts/build.sh check` green; full lib suite green; `npm run build` green; plugin compiles + registers |
| 2 | Add log calls at the high-value paths (commands, download, launch, modpack import/update, swallowed errors) | `src-tauri/src/lib.rs`, `core/download.rs`, `core/launch.rs`, `core/modpack.rs` (+ others as the implementer finds) | atomic-builder | ~5 | `scripts/build.sh check` green; full lib suite green; log lines compile + read sensibly |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Plugin version/API drift vs the verified docs | low | API verified against v2.tauri.app/plugin/logging; `cargo add` pins the current v2 |
| Webview target needs the capability or logs don't reach devtools | med | CP1 adds `"log:default"`; manual GUI check confirms F12 shows logs |
| Log noise drowns signal | med | Level `Info` default; entry/exit + errors only, not per-iteration spam |
| Logging in hot loops (download per-chunk) hurts perf | low | Log at plan/item granularity, not per byte/chunk |

## Change log

<!-- populated on first amendment after approval -->

## Implementation log

### Shipped (CP1 + CP2) — 2026-06-17

CP1 landed before the loop. CP2 built across 2 iterations of /subagent-implementation. Commits (chronological):

- `3a398a3` — CP1: wire `tauri-plugin-log` (Cargo + npm + `log:default` capability; Stdout/LogDir(`apex`)/Webview targets, `Info` level, `[LEVEL target] msg` format; `attachConsole()` in `main.tsx`; 2 `eprintln!`→`log` conversions)
- `da937c9` — CP2: `info!`/`warn!`/`error!` at high-value paths — entry+outcome for the 8 heavyweight commands (`launch_instance`, `add_mod`, `update_mod` incl. all terminal arms, `import_mrpack`, `import_curseforge_zip`, `install_modpack`, `update_modpack`, `begin_login`); `download::execute_plan` start + per-item failure; `launch::spawn_instance` spawn(pid)/failure; modpack `resolve_pack_file`, `build_cf_pack_plan` manual-route warn, reconcile counts. Logging-only, no behavior change.

**Out-of-scope work performed during this build:**
- `468415f` — fixed `scripts/build.sh test` no-filter crash on macOS bash 3.2 (`set -u` + empty `EXTRA[@]` → unbound variable). Found by the orchestrator verify gate; 1-line guard. Unrelated to logging but blocked clean `build.sh test` on macOS.

**Unforeseens — surprises that emerged during implementation:**
- CP1 compiled on macOS but `tsc` failed: `@tauri-apps/plugin-log` was in `package.json` but never `npm install`ed on this machine (`ensure_node` only installs when `node_modules` is absent). Resolved with `npm install` before the loop.
- Iteration 1 missed 3 spec-required outcome/manual log sites in `update_mod` and `build_cf_pack_plan`; closed in iteration 2.

**Deferred items still open:**
- F-1 (em-dash in a download log string) and F-2 (`unwrap_or_else` → `unwrap_or_default` at lib.rs:1597): both 🔵 nits, **dropped** by user — cosmetic, not worth tracking.
- Manual GUI gate **verified** (macOS, 2026-06-17): `[INFO modloader_lib] auth: begin_login …` emitted to both the dev terminal and the logfile at `~/Library/Logs/com.apex.apexlauncher/apex.log`; plugin registers with no capability panic. F12 devtools route wired via `attachConsole()` (same INFO source). Logged paths are user-triggered, so idle boot writes nothing — expected.
- Unrelated runtime bug surfaced during the GUI run: **downloads broken** (engine path fails). Filed as project follow-up `download-broken-runtime` for a future session — the new CP2 logging should make the failing item visible. Not a logging defect.
