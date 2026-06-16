# launch

## What it does

Assembles JVM argv from `LaunchMeta` + resolved paths + `LaunchIdentity`, extracts native jars into a per-instance `natives/` dir, spawns the JVM via `tokio::process::Command` with `mc/` as cwd, streams stdout/stderr as `launch://log` events, tracks running instances in a slug-keyed `RunningRegistry` (Tauri managed state), kills on request, and records `last_played` + `total_playtime_sec` on exit (natural or killed). Identity is resolved at launch time via `resolve_launch_identity`: offline flag or no active account → offline identity (`Player` + UUID v3 of `"OfflinePlayer:Player"`, token `"0"`); active account present → MS refresh token from keyring → `refresh_ms_token` + `xbox_chain` → real MC identity. For fabric/quilt instances with a pinned loader version, `launch_instance` fetches the loader profile and calls `resolver::merge_loader_profile` before download. For forge/neoforge instances, `launch_instance` runs the headless installer via `forge_installer::run_installer` (idempotent), then calls `loader_profile::load_forge_profile` + `resolver::merge_loader_profile` before download — vanilla path unchanged.

## Artifacts

- `src/routes/InstanceDetail.tsx` — Launch/Stop toggle, running badge, live log console (capped at 500 lines), `last_played`/`total_playtime_sec` display rows; subscribes to `launch://log` + `launch://exit` filtered by slug, and `install://log` (not slug-filtered — installer runs at most once at a time) prefixed `[install:<stream>]`
- `src/lib/ipc.ts` — `launchInstance`/`killInstance` invoke wrappers; `LaunchLogPayload`/`LaunchExitPayload`/`InstallLogPayload` mirror types; `LAUNCH_LOG_EVENT`/`LAUNCH_EXIT_EVENT`/`INSTALL_LOG_EVENT` constants; `listenInstallLog` subscribe helper

## CLI code

- `src-tauri/src/core/launch.rs` — core implementation: `build_argv` (CP1, placeholder substitution table including `${library_directory}` + `${clientid}` + `default_jvm_args` fallback for legacy manifests + `apply_logging_config`), `extract_natives` (CP2, zip-slip traversal guard), `spawn_instance` + `monitor_child` + `kill_instance` + `RunningRegistry` + `KillHandle` (CP3), `LaunchSink` trait + `CapturingLaunchSink` (test-only); `LaunchPaths` struct (fields: `game_directory`, `assets_root`, `natives_directory`, `legacy_assets_root`, `library_directory`); `offline_uuid()`; `OFFLINE_PLAYER_NAME`; `LaunchIdentity` struct (CP4: `player_name`, `uuid`, `access_token`, `xuid`, `user_type`, `client_id`); `resolve_launch_identity` (CP4: offline flag → offline; no active account → offline; active account → keyring refresh + Xbox chain → online identity; sets `client_id` from `auth::ms_client_id()`); `rewrite_classpath_for_instance` (C2 pure helper: rewrites classpath + natives entries from cache-rooted paths to instance-rooted paths, deduplicates, returns relative paths to materialize)
- `src-tauri/src/core/forge_installer.rs` — headless NeoForge/Forge installer runner: `run_installer_core` (injectable `download` + `spawn` closures for unit tests), `run_installer` (live reqwest + tokio::process), `InstallerLoaderKind` enum, `InstallSink` trait + `CapturingInstallSink` (test-only), `SpawnResult`; pure helpers `installer_url`, `installer_jar_name`, `loader_version_id`; idempotency guard (skips if `versions/<id>/<id>.json` exists); seeds `launcher_profiles.json` before spawn; installer jar cached under `installer-cache/`; `.part` TOCTOU guard on download; concurrent stdout+stderr drain via `tokio::spawn` tasks; 14 unit tests
- `src-tauri/src/lib.rs` — `launch_instance` Tauri command (orchestration: load instance → resolve vanilla → **if fabric/quilt with pinned loader version: `loader_profile::fetch_profile` + `resolver::merge_loader_profile`** → **if forge/neoforge: `ensure_java` + `forge_installer::run_installer` + `loader_profile::load_forge_profile` + `resolver::merge_loader_profile`** (Java installation reused for step 5) → download with outcome inspection → `ensure_java` → **step 6b: `rewrite_classpath_for_instance` + `core::materialize::materialize` (hardlinks libs + version jars into instance tree; assets stay in `cache/assets`)** → `extract_natives` → `resolve_launch_identity` → `build_argv` → spawn); `kill_instance` Tauri command; `TauriLaunchSink` (emits `launch://log` + `launch://exit` events); `TauriInstallSink` (emits `install://log` events, `{ stream, line }` camelCase payload); registry registered via `.manage(Arc<RunningRegistry>)`
- `src-tauri/src/core/instances.rs` — `record_playtime` (ln 206) + `read_manifest_pub` (ln 228) called by the monitor task on child exit

## Docs

- `docs/spec/vanilla-launch.md` — full spec: goal, success criteria, checkpoints (CP1–CP4), decisions, implementation log
- `docs/design/vanilla-launch.md` — design doc: D1/D2 fork resolution, approach rationale, risk table, open questions
- `docs/spec/fabric-quilt-launch.md` — Phase 4 slice A spec; defines the `launch_instance` loader-branch contract (step 3b)
- `docs/design/fabric-quilt-launch.md` — design rationale for loader profile overlay
- `docs/spec/neoforge-forge-launch.md` — Phase 4 slice B spec; defines headless installer contract, forge profile load, merge step (step 3c)
- `docs/design/neoforge-forge-launch.md` — design rationale for headless installer approach; Maven URL patterns, installer argv, idempotency

## Coupling

- **auth domain:** `resolve_launch_identity` in `launch.rs` imports `AccountStore`, `AuthHttpClient`, `refresh_ms_token`, `xbox_chain` from `core::auth`; `launch_instance` in `lib.rs` holds the `SharedAccountStore` lock during identity resolution. Any change to `AccountMeta` or keyring API in auth must be reflected here.
- **resolver domain:** `launch_instance` calls `resolver::assemble` + `resolver::merge_loader_profile`; `LaunchMeta` is defined in `resolver.rs` and consumed directly. Adding fields to `LaunchMeta` requires coordinating both modules.
- **metadata domain:** `launch_instance` calls `loader_profile::fetch_profile` for fabric/quilt instances; `loader_profile::load_forge_profile` for forge/neoforge instances. Changes to `LoaderProfile` field names or parse API propagate here.
- **download domain:** `launch_instance` calls `execute_plan` from `core::download` and inspects `ItemOutcome` results before spawn; a failed download errors before spawning.
- **java domain:** `launch_instance` calls `java::ensure_java` to obtain the `java`/`java.exe` path; for forge/neoforge the Java installation from the installer step is reused (`java_inst_opt`) to avoid a second `ensure_java` call.
- **instances domain:** monitor task calls `instances::record_playtime` and `instances::read_manifest_pub` on exit; `InstanceDetail.tsx` is shared between the instances domain (mods list) and the launch domain (Launch/Stop + log console). `launch_instance` calls `core::materialize::materialize` (defined in instances domain) to hardlink artifacts before spawn.
- **frontend-shell / ipc.ts:** `LaunchLogPayload`, `LaunchExitPayload`, `InstallLogPayload`, `launchInstance`, `killInstance`, `listenInstallLog` are hand-mirrored in `ipc.ts`; no type generation — drift is a manual risk.

## Conventions worth knowing

- Registry keyed by **slug** (not UUID); `launch_instance` and `kill_instance` both take `slug: String`.
- `KillHandle.kill_tx` is `Option<oneshot::Sender<()>>`; `kill_instance` `take()`s the sender, leaving the map entry in place. **Monitor task is the sole owner of registry removal** — both the natural-exit and kill paths deregister only after the child has actually exited (eliminates TOCTOU relaunch window).
- `LaunchSink` trait (`log` + `exited`) makes the CP3 core Tauri-free; `TauriLaunchSink` in `lib.rs` emits events; `CapturingLaunchSink` is test-only. `InstallSink` trait (`log`) is parallel; `TauriInstallSink` emits `install://log`; `CapturingInstallSink` is test-only.
- Natives extracted flat into `<instances>/<slug>/natives/`; `META-INF/` entries and directory entries are skipped; traversal guard uses `canonicalize` + lexical `normalize_path_launch`.
- Legacy manifests (empty `jvm_args`) receive `default_jvm_args` supplying `-Djava.library.path`, `-cp`, classpath. Modern manifests supply their own `jvm_args`.
- `${path}` (log4j config arg) is dropped when `logging_config` is `None`, not passed raw to the JVM.
- Classpath separator: `:` on non-Windows, `;` on Windows (`#[cfg(target_os = "windows")]`).
- Forge installer args: `java -jar <installer_jar> --installClient <data_dir>`; working dir is `data_dir`; `launcher_profiles.json` seeded before spawn.
- NeoForge Maven base: `https://maven.neoforged.net/releases`, coord `net.neoforged:neoforge:<v>`. Forge Maven base: `https://maven.minecraftforge.net`, coord `net.minecraftforge:forge:<mc>-<v>`. Version ID: `neoforge-<v>` / `forge-<mc>-<v>`.
- **Placeholder gotcha:** `build_argv` fails loud (`AssembleError::UnsubstitutedPlaceholders`) on any `${...}` token missing from the substitution table. Modern MS-auth version JSONs emit `--clientId ${clientid}` + `--xuid ${auth_xuid}`; both must be in the table. `${clientid}` = MSA app client id (online) / empty (offline). Regression fixed 2026-06-16: table had `${auth_xuid}` but not `${clientid}` → real-instance launch failed.
- Test count: 36 Rust tests in `launch_tests.rs` (sibling file, wired via `#[path = "launch_tests.rs"] mod tests;` at the end of `launch.rs`) + 14 in `forge_installer_tests.rs` = 50 launch-domain tests. `launch_tests.rs` includes `cp4_clientid_placeholder_is_substituted` (verifies `${clientid}` substitution from `LaunchIdentity.client_id`) alongside the C2 tests: `rewrite_classpath_for_instance` (classpath rewrite, natives rewrite, pass-through, dedup, rel-path correctness) + `build_argv_forge_library_directory_substituted`/`build_argv_assets_dir_stays_in_shared_cache`/`build_argv_forge_classpath_separator_substituted`/`build_argv_forge_version_name_substituted`.
- Open follow-ups: `vanilla-launch-f-1` (legacy asset virtual tree), `vanilla-launch-f-2` (`-Dminecraft.client.jar=` nit), `fabric-quilt-launch-f-1` (loader-library hash verify) — all in `.claude/project/followups/`.
