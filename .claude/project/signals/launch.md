# launch

## What it does

Assembles JVM argv from `LaunchMeta` + resolved paths + a `LaunchIdentity` (online or offline), extracts native jars into a per-instance `natives/` dir, spawns the JVM via `tokio::process::Command` with `mc/` as cwd, streams stdout/stderr as `launch://log` events, tracks running instances in a slug-keyed `RunningRegistry` (Tauri managed state), kills on request, and records `last_played` + `total_playtime_sec` on exit (natural or killed). Online identity is resolved at launch via `resolve_launch_identity` which refreshes the stored MS token → runs the Xbox chain → returns a fresh MC access token. Offline fallback: name `Player`, UUID v3 of `"OfflinePlayer:Player"` (`2e5dcd13-3805-3256-b49c-819167bf4871`).

## Artifacts

- `src/routes/InstanceDetail.tsx` — Launch/Stop toggle, running badge, live log console (capped at 500 lines), `last_played`/`total_playtime_sec` display rows; subscribes to `launch://log` + `launch://exit` filtered by slug
- `src/lib/ipc.ts` — `launchInstance`/`killInstance` invoke wrappers; `LaunchLogPayload`/`LaunchExitPayload` mirror types; `LAUNCH_LOG_EVENT`/`LAUNCH_EXIT_EVENT` constants

## CLI code

- `src-tauri/src/core/launch.rs` — core implementation: `build_argv` (CP1, placeholder substitution table + `default_jvm_args` fallback for legacy manifests + `apply_logging_config`), `extract_natives` (CP2, zip-slip traversal guard), `spawn_instance` + `monitor_child` + `kill_instance` + `RunningRegistry` + `KillHandle` (CP3), `LaunchSink` trait + `CapturingLaunchSink` (test-only); `LaunchPaths` struct; `LaunchIdentity` struct (player_name, uuid, access_token, xuid, user_type); `LaunchIdentity::offline()` constructor; `resolve_launch_identity` (CP4, async, injectable `AuthHttpClient` seam — reads keyring refresh token, runs `refresh_ms_token` → `xbox_chain`, updates store); `offline_uuid()`; `OFFLINE_PLAYER_NAME`
- `src-tauri/src/lib.rs` — `launch_instance` Tauri command (9-step orchestration: load instance → resolve → download with outcome inspection → `ensure_java` → `extract_natives` → `resolve_launch_identity` → `build_argv` → spawn); `kill_instance` Tauri command; `TauriLaunchSink` (emits `launch://log` + `launch://exit` events); registry and `SharedAccountStore` both registered as Tauri managed state
- `src-tauri/src/core/instances.rs` — `record_playtime` (ln 206) + `read_manifest_pub` (ln 228) called by the monitor task on child exit

## Docs

- `docs/spec/vanilla-launch.md` — full spec: goal, success criteria, checkpoints (CP1–CP4), decisions, implementation log; shipped 2026-06-07, merged 2026-06-08
- `docs/design/vanilla-launch.md` — design doc: D1/D2 fork resolution, approach rationale, risk table, open questions

## Coupling

- **resolver domain:** `launch_instance` calls `resolver::resolve_vanilla` internally; `LaunchMeta` is defined in `resolver.rs` and consumed directly — adding fields to `LaunchMeta` (e.g. `version_type` was added in slice D) requires coordinating both modules.
- **download domain:** `launch_instance` calls `execute_plan` from `core::download` and inspects `ItemOutcome` results before spawn; a failed download now errors before spawning (fixed in iter 4 — previously outcomes were silently dropped).
- **java domain:** `launch_instance` calls `java::ensure_java` to obtain the `java`/`java.exe` path; `JavaInstallation.path` is passed to `spawn_instance`.
- **instances domain:** monitor task calls `instances::record_playtime` and `instances::read_manifest_pub` on exit; `instance.json` schema changes (e.g. new fields) require both domains to stay aligned. `InstanceDetail.tsx` is shared between the instances domain (mods list) and the launch domain (Launch/Stop + log console).
- **auth domain:** `resolve_launch_identity` takes `&mut AccountStore` + `&dyn AuthHttpClient` (both from `core::auth`); `launch_instance` in `lib.rs` locks `SharedAccountStore` (the same Arc managed by the auth commands) for the duration of the identity resolution. A change to `AccountStore::get_refresh_token`, `add_account`, or `AuthHttpClient` trait affects this domain.
- **frontend-shell / ipc.ts:** `LaunchLogPayload`, `LaunchExitPayload`, `launchInstance`, `killInstance` are hand-mirrored in `ipc.ts`; no type generation — drift is a manual risk.

## Conventions worth knowing

- Registry keyed by **slug** (not UUID); `launch_instance` and `kill_instance` both take `slug: String`.
- `KillHandle.kill_tx` is `Option<oneshot::Sender<()>>`; `kill_instance` `take()`s the sender, leaving the map entry in place. **Monitor task is the sole owner of registry removal** — both the natural-exit and kill paths deregister only after the child has actually exited (eliminates TOCTOU relaunch window).
- `LaunchSink` trait (`log` + `exited`) makes the CP3 core Tauri-free; `TauriLaunchSink` in `lib.rs` emits events; `CapturingLaunchSink` is test-only.
- Natives extracted flat into `<instances>/<slug>/natives/`; `META-INF/` entries and directory entries are skipped; traversal guard uses `canonicalize` + lexical `normalize_path_launch` (copied from `java.rs` to avoid cross-module coupling).
- Legacy manifests (empty `jvm_args`) receive `default_jvm_args` supplying `-Djava.library.path`, `-cp`, classpath. Modern manifests supply their own `jvm_args`.
- `${path}` (log4j config arg) is dropped when `logging_config` is `None`, not passed raw to the JVM.
- Classpath separator: `:` on non-Windows, `;` on Windows (`#[cfg(target_os = "windows")]`).
- Test count: 26 tests in `launch.rs` — previous 20 unit + 2 async (CP1–CP3 + playtime) plus 4 new CP4 tests: `cp4_resolve_offline_flag_returns_offline_identity`, `cp4_resolve_no_active_account_returns_offline`, `cp4_resolve_active_account_refresh_at_launch`, plus `cp4_online_identity_in_argv`, `cp4_offline_identity_in_argv`, `cp4_auth_xuid_placeholder_is_substituted`. No live HTTP in any test; mock `AuthHttpClient` + `FakeKeyring` in all identity tests.
- Open follow-ups: `vanilla-launch-f-1` (legacy asset virtual tree for pre-1.7 MC, risk) and `vanilla-launch-f-2` (`-Dminecraft.client.jar=` gets asset index id instead of jar path, nit) — both in `.claude/project/followups/`.
