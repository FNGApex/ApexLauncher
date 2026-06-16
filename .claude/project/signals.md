# Project signals

## Framework & runtime

- **Frontend:** React 19, TypeScript ~5.8, Vite 7, Tailwind v4, React Router v7, TanStack Query v5, Zustand v5 (installed, unused)
- **Backend:** Rust 1.96, Tauri 2; reqwest (native-tls, stream feature), serde/serde_json, uuid, chrono, sha1/sha2/hex, futures-util, tokio (sync + rt + process + time + macros), flate2, tar, zip, keyring 3 (apple-native + windows-native + sync-secret-service), async-trait, thiserror, tempfile (dev)
- **Targets:** macOS, Windows, Linux (cross-platform via Tauri)
- **Branding:** product name `ApexLauncher`; bundle identifier `com.apex.apexlauncher`; Cargo crate name remains `modloader` / `modloader_lib` (unchanged)
- **App version:** 0.1.0 (pre-alpha); Phases 0–1 complete, version/loader metadata from Phase 2/4 pulled forward; Phase 2 (download/resolver/java/vanilla launch) + Phase 3 (MS auth, single-account, keyring) + Phase 4 slice A (Fabric/Quilt launch) + Phase 4 slice B (NeoForge/Forge headless installer + launch) + Phase 5 slice A (providers Browse) + Phase 5 slice B (mod install/enable/disable/update/remove) + Phase 6 slice A (Modrinth `.mrpack` import) + Phase 6 slice B (CurseForge `.zip` import) shipped; storage-auth-reorg fully shipped (A+B+C1+C2): single-account simplification, ApexLauncher path consolidation, cache layout, materialize hardlink helper, per-instance lib/jar materialization wired into launch path

## Build / test / lint

| Purpose | Command | Source |
|---------|---------|--------|
| Frontend build (tsc + vite) | `npm run build` | package.json |
| Dev window (HMR) | `. "$HOME/.cargo/env" && npm run tauri dev` | CLAUDE.md |
| Rust typecheck | `. "$HOME/.cargo/env" && cargo check` (from `src-tauri/`) | CLAUDE.md |
| Rust tests | `. "$HOME/.cargo/env" && cargo test` (from `src-tauri/`) | Cargo.toml |
| Frontend dev only | `npm run dev` | package.json |

Rust is installed via rustup — not on default PATH. Source `$HOME/.cargo/env` before any cargo/tauri command in a fresh shell.

No CI configuration exists yet (planned Phase 7).

**Known test flake:** `cp4_concurrency_bound_not_exceeded` in `download_tests.rs` — timing-sensitive, pre-existing, tracked.

## Language breakdown

| Language | LOC | Files | % |
|----------|-----|-------|---|
| Rust | 20195 | 29 | 65% |
| JSON | 3898 | 25 | 12% |
| Markdown | 3416 | 25 | 11% |
| TypeScript | 2968 | 15 | 9% |
| CSS | 92 | 1 | <1% |
| TOML | 44 | 1 | <1% |
| HTML | 13 | 1 | <1% |

## DevOps & CI

No CI pipeline yet. Cross-platform GitHub Actions builds planned for Phase 7. No signing or auto-update infrastructure.

---

## Domains

| Domain | Repo paths | One-liner | Detail |
|--------|------------|-----------|--------|
| instances | `src-tauri/src/core/instances.rs`, `core/settings.rs`, `core/store.rs`, `core/materialize.rs`, `src/routes/Home.tsx`, `src/routes/InstanceDetail.tsx`, `src/components/NewInstanceModal.tsx` | Instance CRUD, on-disk layout, mods-folder reconciliation, hardlink materializer (wired into launch); mod-state ops (enable/disable/remove) via `instances.rs`; 14 tests | .claude/project/signals/instances.md |
| metadata | `src-tauri/src/core/versions.rs`, `core/loaders.rs`, `core/meta.rs`, `core/loader_profile.rs`, `src/lib/prefetch.ts` | MC version list + loader builds + Fabric/Quilt loader profile fetch/parse; 6h disk cache; 20 tests | .claude/project/signals/metadata.md |
| download | `src-tauri/src/core/download.rs`, `src/lib/ipc.ts` (download types), `docs/spec/download-engine.md`, `docs/design/vanilla-launch.md` | Concurrent hash-verified download engine; SHA-1/SHA-256/SHA-512 verification; `.part` resume with TOCTOU guard; `download://progress` events; 37 unit tests | .claude/project/signals/download.md |
| resolver | `src-tauri/src/core/resolver.rs`, `src-tauri/src/core/fixtures/`, `src/lib/ipc.ts` (resolver types), `docs/spec/vanilla-resolver.md`, `docs/spec/fabric-quilt-launch.md`, `docs/design/vanilla-launch.md`, `docs/design/fabric-quilt-launch.md` | Vanilla manifest → DownloadPlan + LaunchMeta; loader profile merge (Fabric/Quilt); OS-filtered libs/natives/args; asset index expansion; 43 unit tests + JSON fixtures | .claude/project/signals/resolver.md |
| java | `src-tauri/src/core/java.rs`, `src-tauri/src/core/store.rs` (cache_java_dir), `src/lib/ipc.ts` (JavaInstallation/ensureJava), `docs/spec/java-manager.md`, `docs/design/vanilla-launch.md` | Detect system JRE or provision Temurin from Adoptium; traversal-safe tar.gz/zip extraction; `ensure_java` Tauri command; 39 unit tests + adoptium fixture | .claude/project/signals/java.md |
| launch | `src-tauri/src/core/launch.rs`, `src-tauri/src/lib.rs` (launch_instance/kill_instance commands + TauriLaunchSink), `src/routes/InstanceDetail.tsx` (Launch/Stop UI + log console), `src/lib/ipc.ts` (launch types/wrappers), `docs/spec/vanilla-launch.md`, `docs/spec/fabric-quilt-launch.md`, `docs/design/vanilla-launch.md`, `docs/design/fabric-quilt-launch.md` | Argv assembly, natives extraction, per-instance lib/jar materialization (C2 wired), JVM spawn + log streaming; fabric/quilt + forge/neoforge loader profile merge branches; running registry, kill, playtime accounting; 35 Rust tests | .claude/project/signals/launch.md |
| auth | `src-tauri/src/core/auth.rs`, `src-tauri/src/lib.rs` (begin_login/cancel_login/get_account/logout commands), `src-tauri/src/core/store.rs` (account_file), `src/components/Sidebar.tsx` (login/logout control), `src/lib/ipc.ts` (auth types/wrappers), `docs/spec/authentication.md`, `docs/design/authentication.md`, `docs/design/auth-client-id-blocker.md`, `docs/design/storage-auth-reorg.md` | MS OAuth2 device-code flow → Xbox chain → MC identity; single-account store; keyring-backed refresh token; 40 Rust tests (all mock HTTP + fake keyring) | .claude/project/signals/auth.md |
| frontend-shell | `src/main.tsx`, `src/router.tsx`, `src/components/AppShell.tsx`, `src/components/Sidebar.tsx`, `src/routes/Browse.tsx`, `src/routes/Settings.tsx`, `src/lib/ipc.ts`, `src/lib/query.ts`, `src/lib/utils.ts`, `src/styles.css` | App entry, routing, IPC wrapper layer, query client, settings UI, Browse (live), inline sidebar login/logout; no standalone Accounts route | .claude/project/signals/frontend-shell.md |
| providers | `src-tauri/src/core/providers.rs`, `src-tauri/src/core/modrinth.rs`, `src-tauri/src/core/curseforge.rs`, `src-tauri/src/core/fixtures/` (provider fixtures), `src-tauri/src/lib.rs` (search_mods/get_mod_versions/add_mod/set_mod_enabled/remove_mod/update_mod commands), `src/lib/ipc.ts` (provider + mod-install types/wrappers), `src/routes/Browse.tsx`, `docs/spec/providers-browse.md`, `docs/design/providers.md` | `ModProvider` trait + normalized types; Modrinth + CurseForge implementations; injectable HTTP seam; CF `get_file` single-file resolver (modpack-import substrate); 70 Rust tests | .claude/project/signals/providers.md |
| mod-install | `src-tauri/src/core/mod_install.rs`, `src-tauri/src/lib.rs` (add_mod/set_mod_enabled/remove_mod/update_mod commands), `src/lib/ipc.ts` (mod-install types/wrappers), `src/routes/Browse.tsx` (AddToInstanceModal), `src/routes/InstanceDetail.tsx` (per-mod controls), `docs/spec/mod-install.md`, `docs/design/mod-install.md` | BFS dep resolver → InstallPlan (downloads/manual/unresolved/suggestions/warnings); add/enable/disable/update/remove mods; traversal-safe file validation; 40 Rust tests | .claude/project/signals/mod-install.md |
| modpack | `src-tauri/src/core/modpack.rs`, `src-tauri/src/lib.rs` (import_mrpack/import_curseforge_zip commands), `src-tauri/src/core/curseforge.rs` (get_file), `src/lib/ipc.ts` (modpack import types/wrappers), `src/routes/Home.tsx` (import buttons), `src-tauri/tests/curseforge_live.rs`, `docs/spec/modpack-import.md`, `docs/design/modpack-import.md` | Pure parse/plan/extract for `.mrpack` (slice A) and CF `.zip` (slice B) pack import; thin Tauri-command executors create the instance, resolve/download files, apply overrides; 58 Rust tests | .claude/project/signals/modpack.md |

## Cross-cutting

- **IPC type drift risk:** `src/lib/ipc.ts` hand-mirrors Rust structs (camelCase via `serde rename_all`). No generated types yet — specta/ts-rs planned in roadmap. Any Rust struct change requires a manual `ipc.ts` update.
- **Auth commands changed:** `list_accounts`/`get_active_account_id`/`remove_account`/`set_active_account` removed; `get_account`/`logout` added. `accounts.json` → `account.json`. `Accounts.tsx` route removed; auth UI is now inline in `Sidebar.tsx`.
- **App data dir:** `<OS-appdata-base>/ApexLauncher/` resolved via `app.path().data_dir()` + join `"ApexLauncher"` in `store.rs`. macOS: `~/Library/Application Support/ApexLauncher/`. Windows: `%APPDATA%\ApexLauncher\`. Linux: `~/.local/share/ApexLauncher/`. Path is independent of bundle identifier. Cache subtree: `cache/{assets,libraries,versions,java,meta,installers}`.
- **Test layout:** Unit tests live in **sibling `<stem>_tests.rs` files**, not inside the source files. Each source module ends with `#[cfg(test)] #[path = "<stem>_tests.rs"] mod tests;`; the sibling is a child module (`use super::*;`, full private access, no `pub`-leak). Module-scope `#[cfg(test)]` scaffolding (mock sinks `CapturingSink`/`CapturingLaunchSink`/`CapturingInstallSink`, helpers `read_manifest_pub`/`percent_decode`) stays in the source file because it is `pub` + cross-referenced. See CLAUDE.md → "Rust test layout (convention)". 436 Rust lib tests — 37 in `download_tests.rs` (hand-rolled `TcpListener` mock, 1 known timing flake) + 43 in `resolver_tests.rs` (fixture-based) + 39 in `java_tests.rs` (fixture-based, injected provision closure — no live HTTP) + 35 in `launch_tests.rs` (argv assembly, C2 classpath rewrite, identity routing, async spawn/kill) + 40 in `auth_tests.rs` (mock HTTP via `MockAuthClient` VecDeque; keyring via `FakeKeyring`/`FailingKeyring` — no real TCP, no OS keyring) + 20 in `loader_profile_tests.rs` + 14 in `instances_tests.rs` + 13 in `store_tests.rs` + 8 in `materialize_tests.rs` + 14 in `forge_installer_tests.rs` + 40 in `mod_install_tests.rs` (mock `ProviderHttpClient` via `MockProvider` VecDeque — no live HTTP) + 70 in providers (`providers_tests.rs` 21, `modrinth_tests.rs` 15, `curseforge_tests.rs` 34 — 26 pre-existing + 8 `get_file_*` for modpack-import slice B) + 58 in `modpack_tests.rs` (fixture JSON + fixture zips; `resolve_and_build_cf_plan` async tests via mock `ProviderHttpClient` — no live HTTP) + 10 integration tests in `src-tauri/tests/` (`platform_common.rs`: 2 cross-platform path-shape tests; `platform_{linux,macos,unix,windows}.rs`: OS-gated smoke and separator tests; `curseforge_live.rs`: 2 tests, both `#[ignore]`d — hit the real CurseForge API, key read from a gitignored `.env`, run explicitly via `cargo test --test curseforge_live -- --ignored`). No frontend tests. Component tests + Playwright planned Phase 7.
- **Materialize (Slice C complete):** `src-tauri/src/core/materialize.rs` — hardlink+copy-fallback helper (C1 implementation, C2 wiring); `launch_instance` in `lib.rs` calls `rewrite_classpath_for_instance` + `materialize` at step 6b before natives extraction. Assets stay shared in `cache/assets`; libs + version jars are hardlinked (copy fallback on EXDEV / `CrossesDevices` only) into `<instances>/<slug>/`. Follow-up `storage-auth-reorg-c2.md` is closed and deleted.
- **Conventions pointer:** folder/domain conventions live in `.claude/project/signals/*.md`; regenerate with `/refresh-signals`.
- **Deterministic substrate:** `.claude/project/deterministic-signals.md`
- **Domain partitioning basis:** partitioned by functional workflow — instance lifecycle, metadata fetching, download execution, vanilla resolution, Java management, launch orchestration, Microsoft auth, and UI shell are orthogonal concerns that change independently. `src-tauri/src/lib.rs` is the Tauri command dispatch layer shared across all domains. `pub mod core` in `lib.rs` (widened from private) allows `src-tauri/tests/` integration tests to reach `core::store` pure helpers without an `AppHandle`.
- **Stubs:** `Browse.tsx` (Phase 5 slice A) and mod-install flow (slice B) are both live. No remaining UI stubs — `Accounts.tsx` removed; Browse and mod management are implemented. Modrinth path fully functional; CurseForge gated on pending API key.
- **New domain:** `mod-install` — `src-tauri/src/core/mod_install.rs` is a pure BFS planner (no FS I/O, no Tauri commands); executor lives in `lib.rs` `add_mod` / `update_mod`; mod-state ops (`set_mod_enabled`, `remove_mod`) delegate to `instances.rs`.
- **New domain:** `modpack` — `src-tauri/src/core/modpack.rs` is a pure parse/plan/extract module covering both Phase 6 slices (Modrinth `.mrpack` slice A shipped, CurseForge `.zip` slice B shipped); executors are `import_mrpack` / `import_curseforge_zip` in `lib.rs`; reuses `instances::create`/`load_manifest`/`save_manifest`, `download::execute_plan`, and (slice B) `CurseForgeProvider::get_file` + `ProviderHttpClient` from the providers domain.
- **New dependency:** `tauri-plugin-dialog` (`Cargo.toml` + `package.json` `@tauri-apps/plugin-dialog` + `capabilities/default.json` `"dialog:default"`) — added for the modpack-import file pickers in `Home.tsx`.
