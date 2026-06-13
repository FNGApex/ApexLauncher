# Project signals

## Framework & runtime

- **Frontend:** React 19, TypeScript ~5.8, Vite 7, Tailwind v4, React Router v7, TanStack Query v5, Zustand v5 (installed, unused)
- **Backend:** Rust 1.96, Tauri 2; reqwest (native-tls, stream feature), serde/serde_json, uuid, chrono, sha1/sha2/hex, futures-util, tokio (sync + rt + process + time + macros), flate2, tar, zip, keyring 3 (apple-native + windows-native + sync-secret-service), async-trait, thiserror, tempfile (dev)
- **Targets:** macOS, Windows, Linux (cross-platform via Tauri)
- **Branding:** product name `ApexLauncher`; bundle identifier `com.apex.apexlauncher`; Cargo crate name remains `modloader` / `modloader_lib` (unchanged)
- **App version:** 0.1.0 (pre-alpha); Phases 0–1 complete, version/loader metadata from Phase 2/4 pulled forward; Phase 2 (download/resolver/java/vanilla launch) + Phase 3 (MS auth, single-account, keyring) + Phase 4 slice A (Fabric/Quilt launch) + Phase 4 slice B (NeoForge/Forge headless installer + launch) + Phase 5 slice A (providers Browse) shipped; storage-auth-reorg branch merged (single-account simplification, ApexLauncher path consolidation, cache layout, materialize.rs stub)

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

**Known test flake:** `cp4_concurrency_bound_not_exceeded` in `download.rs` — timing-sensitive, pre-existing, tracked.

## Language breakdown

| Language | LOC | Files | % |
|----------|-----|-------|---|
| Rust | 14774 | 21 | 62% |
| JSON | 3683 | 17 | 15% |
| Markdown | 2702 | 21 | 11% |
| TypeScript | 2192 | 15 | 9% |
| CSS | 92 | 1 | <1% |
| TOML | 43 | 1 | <1% |
| HTML | 13 | 1 | <1% |

## DevOps & CI

No CI pipeline yet. Cross-platform GitHub Actions builds planned for Phase 7. No signing or auto-update infrastructure.

---

## Domains

| Domain | Repo paths | One-liner | Detail |
|--------|------------|-----------|--------|
| instances | `src-tauri/src/core/instances.rs`, `core/settings.rs`, `core/store.rs`, `core/materialize.rs`, `src/routes/Home.tsx`, `src/routes/InstanceDetail.tsx`, `src/components/NewInstanceModal.tsx` | Instance CRUD, on-disk layout, mods-folder reconciliation, hardlink materializer stub | .claude/project/signals/instances.md |
| metadata | `src-tauri/src/core/versions.rs`, `core/loaders.rs`, `core/meta.rs`, `core/loader_profile.rs`, `src/lib/prefetch.ts` | MC version list + loader builds + Fabric/Quilt loader profile fetch/parse; 6h disk cache; 20 tests | .claude/project/signals/metadata.md |
| download | `src-tauri/src/core/download.rs`, `src/lib/ipc.ts` (download types), `docs/spec/download-engine.md`, `docs/design/vanilla-launch.md` | Concurrent hash-verified download engine; SHA-1/SHA-256/SHA-512 verification; `.part` resume with TOCTOU guard; `download://progress` events; 37 unit tests | .claude/project/signals/download.md |
| resolver | `src-tauri/src/core/resolver.rs`, `src-tauri/src/core/fixtures/`, `src/lib/ipc.ts` (resolver types), `docs/spec/vanilla-resolver.md`, `docs/spec/fabric-quilt-launch.md`, `docs/design/vanilla-launch.md`, `docs/design/fabric-quilt-launch.md` | Vanilla manifest → DownloadPlan + LaunchMeta; loader profile merge (Fabric/Quilt); OS-filtered libs/natives/args; asset index expansion; 43 unit tests + JSON fixtures | .claude/project/signals/resolver.md |
| java | `src-tauri/src/core/java.rs`, `src-tauri/src/core/store.rs` (cache_java_dir), `src/lib/ipc.ts` (JavaInstallation/ensureJava), `docs/spec/java-manager.md`, `docs/design/vanilla-launch.md` | Detect system JRE or provision Temurin from Adoptium; traversal-safe tar.gz/zip extraction; `ensure_java` Tauri command; 39 unit tests + adoptium fixture | .claude/project/signals/java.md |
| launch | `src-tauri/src/core/launch.rs`, `src-tauri/src/lib.rs` (launch_instance/kill_instance commands + TauriLaunchSink), `src/routes/InstanceDetail.tsx` (Launch/Stop UI + log console), `src/lib/ipc.ts` (launch types/wrappers), `docs/spec/vanilla-launch.md`, `docs/spec/fabric-quilt-launch.md`, `docs/design/vanilla-launch.md`, `docs/design/fabric-quilt-launch.md` | Argv assembly, natives extraction, JVM spawn + log streaming; fabric/quilt + forge/neoforge loader profile merge branches; running registry, kill, playtime accounting; 29 Rust tests | .claude/project/signals/launch.md |
| auth | `src-tauri/src/core/auth.rs`, `src-tauri/src/lib.rs` (begin_login/cancel_login/get_account/logout commands), `src-tauri/src/core/store.rs` (account_file), `src/components/Sidebar.tsx` (login/logout control), `src/lib/ipc.ts` (auth types/wrappers), `docs/spec/authentication.md`, `docs/design/authentication.md`, `docs/design/auth-client-id-blocker.md`, `docs/design/storage-auth-reorg.md` | MS OAuth2 device-code flow → Xbox chain → MC identity; single-account store; keyring-backed refresh token; 40 Rust tests (all mock HTTP + fake keyring) | .claude/project/signals/auth.md |
| frontend-shell | `src/main.tsx`, `src/router.tsx`, `src/components/AppShell.tsx`, `src/components/Sidebar.tsx`, `src/routes/Browse.tsx`, `src/routes/Settings.tsx`, `src/lib/ipc.ts`, `src/lib/query.ts`, `src/lib/utils.ts`, `src/styles.css` | App entry, routing, IPC wrapper layer, query client, settings UI, Browse (live), inline sidebar login/logout; no standalone Accounts route | .claude/project/signals/frontend-shell.md |
| providers | `src-tauri/src/core/providers.rs`, `src-tauri/src/core/modrinth.rs`, `src-tauri/src/core/curseforge.rs`, `src-tauri/src/core/fixtures/` (provider fixtures), `src-tauri/src/lib.rs` (search_mods/get_mod_versions commands), `src/lib/ipc.ts` (provider types/wrappers), `src/routes/Browse.tsx`, `docs/spec/providers-browse.md`, `docs/design/providers.md` | `ModProvider` trait + normalized types; Modrinth + CurseForge implementations; injectable HTTP seam; Phase 6 pack-resolver substrate; 62 Rust tests | .claude/project/signals/providers.md |

## Cross-cutting

- **IPC type drift risk:** `src/lib/ipc.ts` hand-mirrors Rust structs (camelCase via `serde rename_all`). No generated types yet — specta/ts-rs planned in roadmap. Any Rust struct change requires a manual `ipc.ts` update.
- **Auth commands changed:** `list_accounts`/`get_active_account_id`/`remove_account`/`set_active_account` removed; `get_account`/`logout` added. `accounts.json` → `account.json`. `Accounts.tsx` route removed; auth UI is now inline in `Sidebar.tsx`.
- **App data dir:** `<OS-appdata-base>/ApexLauncher/` resolved via `app.path().data_dir()` + join `"ApexLauncher"` in `store.rs`. macOS: `~/Library/Application Support/ApexLauncher/`. Windows: `%APPDATA%\ApexLauncher\`. Linux: `~/.local/share/ApexLauncher/`. Path is independent of bundle identifier. Cache subtree: `cache/{assets,libraries,versions,java,meta,installers}`.
- **Test layout:** ~307 Rust tests total — 37 in `download.rs` (hand-rolled `TcpListener` mock, 1 known timing flake) + 43 in `resolver.rs` (fixture-based) + 39 in `java.rs` (fixture-based, injected provision closure — no live HTTP) + 29 in `launch.rs` (argv assembly, identity routing, async spawn/kill) + 40 in `auth.rs` (mock HTTP via `MockAuthClient` VecDeque; keyring via `FakeKeyring`/`FailingKeyring` — no real TCP, no OS keyring) + 20 in `loader_profile.rs` + 13 in `store.rs` + 6 in `materialize.rs`. No frontend tests. Component tests + Playwright planned Phase 7.
- **New module:** `src-tauri/src/core/materialize.rs` — hardlink+copy-fallback helper (Slice C1); not yet wired into launch (Slice C2 deferred, see `.claude/project/followups/storage-auth-reorg-c2.md`).
- **Conventions pointer:** folder/domain conventions live in `.claude/project/signals/*.md`; regenerate with `/refresh-signals`.
- **Deterministic substrate:** `.claude/project/deterministic-signals.md`
- **Domain partitioning basis:** partitioned by functional workflow — instance lifecycle, metadata fetching, download execution, vanilla resolution, Java management, launch orchestration, Microsoft auth, and UI shell are orthogonal concerns that change independently. `src-tauri/src/lib.rs` is the Tauri command dispatch layer shared across all domains.
- **Stubs:** `Browse.tsx` (Phase 5) is live (Phase 5 slice A complete). No remaining UI stubs — `Accounts.tsx` removed; Browse is implemented.
