# launch

## What it does

Assembles JVM argv from `LaunchMeta` + resolved paths + `LaunchIdentity`, extracts native jars into a per-instance `natives/` dir, spawns the JVM via `tokio::process::Command` with `mc/` as cwd, streams stdout/stderr as `launch://log` events, tracks running instances in a slug-keyed `RunningRegistry` (Tauri managed state), kills on request, and records `last_played` + `total_playtime_sec` on exit (natural or killed). Identity is resolved at launch time via `resolve_launch_identity`: offline flag or no active account → offline identity; active account present → MS refresh token from keyring → `refresh_ms_token` + `xbox_chain` → real MC identity. For fabric/quilt instances `launch_instance` fetches the loader profile and merges before download; for forge/neoforge `launch_instance` runs the headless installer, loads the forge profile, and merges before download.

The `RunningRegistry` (slug → `RunState`) is the source of truth for run lifecycle. `RunStatus` variants: `Preparing` (prep serialized via `PrepSemaphore` — single permit, Tauri managed state; only one instance can be in prep at a time), `Running` (N-concurrent), `Exited`/`Killed`/`Failed` (terminal, retained in registry for query). Each `RunState` holds a capped ring buffer of log lines (`LOG_RING_CAP = 1000`). `list_running` / `get_run_state` / `get_run_logs` are queryable from `lib.rs` commands. `TauriLaunchSink` emits `run://update` events on every `RunStatus` transition so the frontend store stays live.

## Artifacts

- `src/routes/InstanceDetail.tsx` — Launch/Stop toggle, running badge, live log console (capped at 500 lines in the UI); **run state and logs are read from the Zustand store** (`useAppStore(s => s.runs.get(slug))` and `useAppStore(s => s.runLogs.get(slug))`); no local event subscriptions in `InstanceDetail` — all subscriptions live in `AppShell`
- `src/lib/ipc.ts` — `launchInstance`/`killInstance` invoke wrappers; `listRunning()`/`getRunState(slug)`/`getRunLogs(slug)` for hydration; `LaunchLogPayload`/`LaunchExitPayload`/`InstallLogPayload` mirror types; `RunUpdatePayload`/`RunInfoPayload`/`RunLogPayload` interfaces; `LAUNCH_LOG_EVENT`/`LAUNCH_EXIT_EVENT`/`INSTALL_LOG_EVENT`/`RUN_UPDATE_EVENT` constants; `listenRunUpdate` subscribe helper

## CLI code

- `src-tauri/src/core/launch.rs` (1155 lines) — core implementation: `build_argv` (placeholder substitution table including `${clientid}`, `${auth_xuid}`, `${library_directory}`, `default_jvm_args` fallback), `extract_natives` (traversal guard), `spawn_instance` + `monitor_child` + `kill_instance` + `RunningRegistry` + `KillHandle`; **`RunStatus` enum** (Preparing/Running/Exited/Killed/Failed; `is_terminal()`, `as_str()`); **`RunState`** (status, exit_code, started instant, `log_ring: VecDeque<LogLine>`, `push_log` capped at `LOG_RING_CAP = 1000`); **`LogLine`** (`stream: String`, `line: String`); **`RunInfo`** (queryable scalars: slug, status, exit_code, elapsed_ms); `RunningRegistry = Mutex<HashMap<String, RunState>>`; `PrepSemaphore = Arc<Semaphore>` (single permit, serializes prep phase); `mark_preparing`/`mark_failed`/`record_prep_log`/`list_running`/`get_run_state`/`get_run_logs` pub query API; `LaunchSink` trait (`log`, `exited`, `status` hooks); `CapturingLaunchSink` (test-only); `LaunchPaths`; `LaunchIdentity`; `resolve_launch_identity`; `rewrite_classpath_for_instance`; `offline_uuid()` / `OFFLINE_PLAYER_NAME`
- `src-tauri/src/core/forge_installer.rs` — headless NeoForge/Forge installer runner; `run_installer_core` (injectable closures), `run_installer` (live), `InstallerLoaderKind`, `InstallSink` trait + `CapturingInstallSink`; 14 unit tests
- `src-tauri/src/lib.rs` — `launch_instance` Tauri command (orchestration: load instance → resolve vanilla → loader profile merge (fabric/quilt) or forge installer + profile merge → download → ensure_java → rewrite_classpath + materialize → extract_natives → resolve_launch_identity → build_argv → `mark_preparing` → acquire `PrepSemaphore` → spawn); `kill_instance` Tauri command; `TauriLaunchSink` (emits `launch://log` + `launch://exit` + `run://update`); `TauriInstallSink` (emits `install://log`); `list_running`/`get_run_state`/`get_run_logs` Tauri commands delegating to `launch::` query API; `PrepSemaphore` registered via `.manage(new_prep_semaphore())`; `RunningRegistry` registered via `.manage(Arc<RunningRegistry>)`
- `src-tauri/src/core/instances.rs` — `record_playtime` + `read_manifest_pub` called by the monitor task on child exit

## Docs

- `docs/spec/vanilla-launch.md` — full spec: goal, success criteria, checkpoints (CP1–CP4), decisions, implementation log
- `docs/design/vanilla-launch.md` — design doc: D1/D2 fork resolution, approach rationale, risk table
- `docs/spec/fabric-quilt-launch.md` — Phase 4 slice A spec; `launch_instance` loader-branch contract
- `docs/design/fabric-quilt-launch.md` — design rationale for loader profile overlay
- `docs/spec/neoforge-forge-launch.md` — Phase 4 slice B spec; headless installer contract, forge profile load, merge step
- `docs/design/neoforge-forge-launch.md` — design rationale; Maven URL patterns, installer argv, idempotency
- `docs/spec/download-runner-rework/cp-6a-runstate-lifecycle.md` — CP-6a spec: `RunStatus`, `RunState`, `RunningRegistry` extensions
- `docs/spec/download-runner-rework/cp-6b-log-ring.md` — CP-6b spec: `LogLine`, `LOG_RING_CAP`, `push_log`, `get_run_logs` query
- `docs/spec/download-runner-rework/cp-6c-prep-serialization.md` — CP-6c spec: `PrepSemaphore` single-permit pattern, `mark_preparing`/`mark_failed`
- `docs/spec/download-runner-rework/cp-6d-runner-surface.md` — CP-6d spec: `list_running`/`get_run_state`/`get_run_logs` commands; `RunUpdatePayload`/`RunInfoPayload`/`RunLogPayload` IPC types; `run://update` event

## Coupling

- **frontend-shell domain:** `AppShell` subscribes to `run://update` (`listenRunUpdate` → `upsertRun`), `launch://log`/`launch://exit` (→ `appendLog`), `install://log` (→ `appendLog` on the preparing slug). `InstanceDetail` reads run state from store. `AppShell` hydrates via `listRunning()` + `getRunLogs(slug)` on mount. Changes to `RunUpdatePayload`/`RunInfoPayload`/`RunLogPayload` require updates to `store.ts` `RunState`/`RunLogLine` types.
- **auth domain:** `resolve_launch_identity` imports `AccountStore`, `AuthHttpClient`, `refresh_ms_token`, `xbox_chain` from `core::auth`. Any change to `AccountMeta` or keyring API must be reflected here.
- **resolver domain:** `launch_instance` calls `resolver::assemble` + `resolver::merge_loader_profile`; `LaunchMeta` is defined in `resolver.rs`. Adding fields to `LaunchMeta` requires coordinating both modules.
- **metadata domain:** `launch_instance` calls `loader_profile::fetch_profile` for fabric/quilt; `loader_profile::load_forge_profile` for forge/neoforge. Changes to `LoaderProfile` propagate here.
- **download domain:** `launch_instance` calls `execute_plan` (not `execute_plan_cancellable` — launch downloads are not task-queued); checks `ItemOutcome` before spawn.
- **java domain:** `launch_instance` calls `java::ensure_java` for the JVM path; forge/neoforge reuses the `java_inst_opt` from the installer step.
- **instances domain:** monitor task calls `instances::record_playtime` and `instances::read_manifest_pub` on exit; `launch_instance` calls `core::materialize::materialize` (step 6b) to hardlink artifacts.

## Conventions worth knowing

- Registry keyed by **slug** (not UUID); `launch_instance` and `kill_instance` both take `slug: String`.
- `PrepSemaphore` has a single permit; `launch_instance` calls `mark_preparing` (inserts `Preparing` entry into registry, emits `run://update`), then acquires the semaphore, runs prep/download/materialize, then spawns. If prep fails, `mark_failed` transitions the entry to `Failed`. The semaphore is released after spawn (whether successful or failed), allowing the next queued launch to proceed.
- `KillHandle.kill_tx` is `Option<oneshot::Sender<()>>`; `kill_instance` `take()`s the sender. **Monitor task is the sole owner of registry removal** — deregisters only after child actually exits.
- `LaunchSink` trait (`log` + `exited` + `status`) is Tauri-free; `TauriLaunchSink` emits `launch://log`, `launch://exit`, `run://update`; `CapturingLaunchSink` is test-only.
- `LOG_RING_CAP = 1000` (in `launch.rs`); frontend caps display at 500 lines (local slice in `InstanceDetail`).
- `RunStatus::as_str()` → lowercase strings (`"preparing"`, `"running"`, `"exited"`, `"killed"`, `"failed"`); matches `RunState.status` string in `store.ts`.
- `list_running` excludes terminal entries; `get_run_state`/`get_run_logs` return terminal entries too (retained for replay).
- `record_prep_log` attributes installer log lines to whichever `RunState` has status `Preparing` — safe because `PrepSemaphore` guarantees at most one `Preparing` entry at a time.
- Natives extracted flat into `<instances>/<slug>/natives/`; traversal guard uses `canonicalize` + lexical `normalize_path_launch`.
- Legacy manifests (empty `jvm_args`) receive `default_jvm_args` supplying `-Djava.library.path`, `-cp`, classpath.
- `${path}` (log4j config arg) is dropped when `logging_config` is `None`.
- Classpath separator: `:` on non-Windows, `;` on Windows.
- **Placeholder gotcha:** `build_argv` fails loud (`AssembleError::UnsubstitutedPlaceholders`) on any `${...}` token missing from the substitution table. Modern MS-auth version JSONs emit `--clientId ${clientid}` + `--xuid ${auth_xuid}`; both must be in the table.
- Test count: 36 Rust tests in `launch_tests.rs` + 14 in `forge_installer_tests.rs` = 50 launch-domain tests.
- Open follow-ups: `vanilla-launch-f-1` (legacy asset virtual tree), `vanilla-launch-f-2` (`-Dminecraft.client.jar=` nit), `fabric-quilt-launch-f-1` (loader-library hash verify) — all in `.claude/project/followups/`.
