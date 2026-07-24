# launch

## Overview

JVM lifecycle: assembles argv from `LaunchMeta` + resolved paths + `LaunchIdentity`, extracts native jars, spawns via `tokio::process::Command` with `mc/` as cwd, streams stdout/stderr as `launch://log` events, tracks running instances in a slug-keyed `RunningRegistry`, kills on request, records `last_played` + `total_playtime_sec` on exit. `resolve_launch_identity`: offline or no account → offline identity; account present → keyring refresh token → `refresh_ms_token` + `xbox_chain` → real MC identity.

`RunningRegistry` (slug → `RunState`) is the source of truth. `RunStatus`: `Preparing` (serialized via `PrepSemaphore` — single permit; one prep at a time), `Running`, `Exited`/`Killed`/`Failed` (terminal, retained for query). Each `RunState` holds a log ring (`LOG_RING_CAP = 1000`). `TauriLaunchSink` emits `run://update` on every transition.

For fabric/quilt: `launch_instance` fetches the loader profile and merges before download. For forge/neoforge: runs the headless installer, loads the forge profile, and merges.

## CLI code

- `src-tauri/src/core/launch.rs` — `build_argv` (placeholder substitution including `${clientid}`, `${auth_xuid}`; `default_jvm_args` fallback for legacy manifests), `extract_natives` (traversal guard), `spawn_instance`, `monitor_child`, `kill_instance`, `RunningRegistry`, `KillHandle`; `RunStatus` (`Preparing/Running/Exited/Killed/Failed`, `is_terminal()`, `as_str()`); `RunState` (`status`, `exit_code`, started instant, `log_ring: VecDeque<LogLine>`, `push_log` capped at `LOG_RING_CAP`); `LogLine { stream, line }`; `RunInfo`; `PrepSemaphore = Arc<Semaphore>` (single permit); `mark_preparing`/`mark_failed`/`record_prep_log`/`list_running`/`get_run_state`/`get_run_logs` pub query API; `LaunchSink` trait; `CapturingLaunchSink` (test-only); `LaunchPaths`; `LaunchIdentity`; `resolve_launch_identity`; `rewrite_classpath_for_instance`; 46 tests in `launch_tests.rs`
- `src-tauri/src/core/forge_installer.rs` — headless NeoForge/Forge installer runner; `run_installer_core` (injectable closures), `run_installer`, `InstallerLoaderKind`, `InstallSink`; 14 tests in `forge_installer_tests.rs`
- `src-tauri/src/lib.rs` — `launch_instance` Tauri command (orchestration: load instance → resolve vanilla → loader profile merge or forge installer + profile merge → download → ensure_java → rewrite_classpath + materialize → extract_natives → resolve_launch_identity → build_argv → mark_preparing → acquire PrepSemaphore → spawn); `kill_instance` command; `TauriLaunchSink` (emits `launch://log` + `launch://exit` + `run://update`); `TauriInstallSink` (emits `install://log`); `list_running`/`get_run_state`/`get_run_logs` commands; `PrepSemaphore` + `RunningRegistry` registered as managed state

## Artifacts

- `src/routes/InstanceDetail.tsx` — Launch/Stop toggle, live log console (500-line cap in UI); run state + logs read from `useAppStore` (no local event subscriptions — all in `AppShell`)
- `src/lib/ipc.ts` — `launchInstance`/`killInstance`; `listRunning`/`getRunState`/`getRunLogs`; `LaunchLogPayload`/`LaunchExitPayload`/`RunUpdatePayload`/`RunInfoPayload`/`RunLogPayload`; `LAUNCH_LOG_EVENT`/`LAUNCH_EXIT_EVENT`/`RUN_UPDATE_EVENT`; `listenRunUpdate`

## Docs

- `docs/spec/vanilla-launch.md` — full spec, checkpoints CP1–CP4
- `docs/spec/neoforge-forge-launch.md` — headless installer contract, forge profile load, merge step
- `docs/spec/download-runner-rework/cp-6a-runstate-lifecycle.md` — `RunStatus`, `RunState`, `RunningRegistry`
- `docs/spec/download-runner-rework/cp-6b-log-ring.md` — `LogLine`, `LOG_RING_CAP`, `push_log`
- `docs/spec/download-runner-rework/cp-6c-prep-serialization.md` — `PrepSemaphore` pattern
- `docs/spec/download-runner-rework/cp-6d-runner-surface.md` — `list_running`/`get_run_state`/`get_run_logs`; `run://update` event

## Coupling

- `auth` domain — `resolve_launch_identity` imports `AccountStore`, `AuthHttpClient`, `refresh_ms_token`, `xbox_chain` from `auth.rs`.
- `resolver` domain — `launch_instance` calls `resolver::assemble` + `resolver::merge_loader_profile`.
- `metadata` domain — `launch_instance` calls `loader_profile::fetch_profile` (fabric/quilt) or `loader_profile::load_forge_profile` (forge/neoforge).
- `download` domain — `launch_instance` calls `execute_plan` (not cancellable — launch downloads are not task-queued).
- `java` domain — calls `java_resolve::resolve_effective_java`; calls `java::ensure_java` only when `effective.java_path` is `None`.
- `instances` domain — monitor task calls `instances::record_playtime` + `instances::read_manifest_pub` on exit; `launch_instance` calls `core::materialize::materialize` (step 6b).
- `frontend-shell` — `AppShell` subscribes to `run://update`, `launch://log`/`launch://exit`, `install://log`; hydrates via `listRunning()` + `getRunLogs(slug)` on mount.

## Conventions

- Registry keyed by **slug** (not UUID).
- `PrepSemaphore` has single permit: `mark_preparing` inserts `Preparing` entry, then semaphore acquired, then prep/download/materialize, then spawn. On failure: `mark_failed`. Semaphore released after spawn.
- `KillHandle.kill_tx` is `Option<oneshot::Sender<()>>`; `kill_instance` `take()`s the sender. Monitor task is the sole owner of registry removal.
- `LOG_RING_CAP = 1000` in `launch.rs`; frontend caps display at 500.
- `list_running` excludes terminal entries; `get_run_state`/`get_run_logs` include them (retained for replay).
- `record_prep_log` attributes installer log lines to the `Preparing` `RunState` — safe because `PrepSemaphore` guarantees at most one `Preparing` entry.
- Classpath separator: `:` on non-Windows, `;` on Windows.
- **Placeholder gotcha:** `build_argv` fails loud (`AssembleError::UnsubstitutedPlaceholders`) on any `${...}` token missing from the substitution table. Modern MS-auth version JSONs emit `--clientId ${clientid}` + `--xuid ${auth_xuid}`; both must be in the table.
