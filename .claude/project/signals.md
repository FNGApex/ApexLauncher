# Project signals

## Framework & runtime

- **Frontend:** React 19, TypeScript ~5.8, Vite 7, Tailwind v4, React Router v7, TanStack Query v5, Zustand v5 (installed, unused)
- **Backend:** Rust 1.96, Tauri 2; reqwest (native-tls, stream feature), serde/serde_json, uuid, chrono, sha1/sha2/hex, futures-util, tokio (sync + rt + process + time + macros), flate2, tar, zip, keyring 3 (apple-native + windows-native + sync-secret-service), async-trait, thiserror, tempfile (dev)
- **Targets:** macOS, Windows, Linux (cross-platform via Tauri)
- **App version:** 0.1.0 (pre-alpha); Phases 0–1 complete, version/loader metadata from Phase 2/4 pulled forward; Phase 2 slices A (download engine) + B (vanilla resolver) + C (Java manager) + D (vanilla launch) shipped; Phase 3 (MS auth, multi-account, keyring) shipped; Phase 4 slice A (Fabric/Quilt launch) shipped; Phase 4 slice B (NeoForge/Forge headless installer + launch) shipped; Phase 5 slice A (providers Browse — search, facets, infinite scroll) shipped
- **New deps (Phase 3):** `keyring` crate (OS keyring backend), `async-trait` (injectable HTTP + keyring seams)

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

## Language breakdown

| Language | LOC | Files | % |
|----------|-----|-------|---|
| Rust | 11976 | 17 | 56% |
| JSON | 5256 | 18 | 24% |
| Markdown | 2137 | 17 | 10% |
| TypeScript | 1796 | 16 | 8% |
| CSS | 92 | 1 | <1% |
| TOML | 43 | 1 | <1% |
| HTML | 13 | 1 | <1% |

## DevOps & CI

No CI pipeline yet. Cross-platform GitHub Actions builds planned for Phase 7. No signing or auto-update infrastructure.

---

## Domains

| Domain | Repo paths | One-liner | Detail |
|--------|------------|-----------|--------|
| instances | `src-tauri/src/core/instances.rs`, `core/settings.rs`, `core/store.rs`, `src/routes/Home.tsx`, `src/routes/InstanceDetail.tsx`, `src/components/NewInstanceModal.tsx` | Instance CRUD, on-disk layout, mods-folder reconciliation | .claude/project/signals/instances.md |
| metadata | `src-tauri/src/core/versions.rs`, `core/loaders.rs`, `core/meta.rs`, `core/loader_profile.rs`, `src/lib/prefetch.ts` | MC version list + loader builds + Fabric/Quilt loader profile fetch/parse + Forge/NeoForge disk profile load; 6h disk cache; 20 tests | .claude/project/signals/metadata.md |
| download | `src-tauri/src/core/download.rs`, `src/lib/ipc.ts` (download types), `docs/spec/download-engine.md`, `docs/design/vanilla-launch.md` | Concurrent hash-verified download engine; SHA-1/SHA-256/SHA-512 verification; `.part` resume with TOCTOU guard; `download://progress` events; 37 unit tests | .claude/project/signals/download.md |
| resolver | `src-tauri/src/core/resolver.rs`, `src-tauri/src/core/fixtures/`, `src/lib/ipc.ts` (resolver types), `docs/spec/vanilla-resolver.md`, `docs/spec/fabric-quilt-launch.md`, `docs/spec/neoforge-forge-launch.md`, `docs/design/vanilla-launch.md`, `docs/design/fabric-quilt-launch.md`, `docs/design/neoforge-forge-launch.md` | Vanilla manifest → DownloadPlan + LaunchMeta; loader profile merge (Fabric/Quilt/Forge/NeoForge); OS-filtered libs/natives/args; asset index expansion; 43 unit tests + 5 JSON fixtures | .claude/project/signals/resolver.md |
| java | `src-tauri/src/core/java.rs`, `src-tauri/src/core/store.rs` (java_dir), `src/lib/ipc.ts` (JavaInstallation/ensureJava), `docs/spec/java-manager.md`, `docs/design/vanilla-launch.md` | Detect system JRE or provision Temurin from Adoptium; traversal-safe tar.gz/zip extraction; `ensure_java` Tauri command; 39 unit tests + adoptium fixture | .claude/project/signals/java.md |
| launch | `src-tauri/src/core/launch.rs`, `src-tauri/src/core/forge_installer.rs`, `src-tauri/src/lib.rs` (launch_instance/kill_instance commands + TauriLaunchSink + TauriInstallSink), `src/routes/InstanceDetail.tsx` (Launch/Stop UI + log console + install log), `src/lib/ipc.ts` (launch + install types/wrappers), `docs/spec/vanilla-launch.md`, `docs/spec/fabric-quilt-launch.md`, `docs/spec/neoforge-forge-launch.md`, `docs/design/vanilla-launch.md`, `docs/design/fabric-quilt-launch.md`, `docs/design/neoforge-forge-launch.md` | Argv assembly, natives extraction, JVM spawn + log streaming; fabric/quilt loader profile merge branch; forge/neoforge headless installer branch (idempotent, `install://log` events); running registry, kill, playtime accounting; 29 + 14 Rust tests | .claude/project/signals/launch.md |
| auth | `src-tauri/src/core/auth.rs`, `src-tauri/src/lib.rs` (begin_login/cancel_login/list_accounts/remove_account/set_active_account commands), `src-tauri/src/core/store.rs` (accounts_file), `src/routes/Accounts.tsx`, `src/lib/ipc.ts` (auth types/wrappers), `docs/spec/authentication.md`, `docs/design/authentication.md`, `docs/design/auth-client-id-blocker.md` | MS OAuth2 device-code flow → Xbox chain → Minecraft identity; multi-account store; keyring-backed refresh tokens; env-overridable client ID; 44 Rust tests (all mock HTTP + fake keyring) | .claude/project/signals/auth.md |
| frontend-shell | `src/main.tsx`, `src/router.tsx`, `src/components/AppShell.tsx`, `src/components/Sidebar.tsx`, `src/routes/Browse.tsx`, `src/routes/Accounts.tsx`, `src/routes/Settings.tsx`, `src/lib/ipc.ts`, `src/lib/query.ts`, `src/lib/utils.ts`, `src/styles.css` | App entry, routing, IPC wrapper layer, query client, settings UI, live accounts UI, live Browse page (Phase 5) | .claude/project/signals/frontend-shell.md |
| providers | `src-tauri/src/core/providers.rs`, `src-tauri/src/core/modrinth.rs`, `src-tauri/src/core/curseforge.rs`, `src-tauri/src/core/fixtures/` (provider fixtures), `src-tauri/src/lib.rs` (search_mods/get_mod_versions commands), `src/lib/ipc.ts` (provider types/wrappers), `src/routes/Browse.tsx`, `docs/spec/providers-browse.md`, `docs/design/providers.md` | `ModProvider` trait + normalized types; Modrinth + CurseForge implementations; injectable HTTP seam; Phase 6 pack-resolver substrate; 62 Rust tests | .claude/project/signals/providers.md |

## Cross-cutting

- **IPC type drift risk:** `src/lib/ipc.ts` hand-mirrors Rust structs (camelCase via `serde rename_all`). No generated types yet — specta/ts-rs planned in roadmap cross-cutting. Any Rust struct change requires a manual `ipc.ts` update.
- **Test layout:** 293 Rust tests total — 37 in `download.rs` (hand-rolled `TcpListener` mock) + 43 in `resolver.rs` (fixture-based, 5 JSON files under `src-tauri/src/core/fixtures/`) + 39 in `java.rs` (fixture-based: `src-tauri/src/core/fixtures/adoptium_latest.json`; `ensure_java_core` with injected provision closure) + 29 in `launch.rs` (argv assembly + identity routing + async spawn/kill; mock `AuthHttpClient` + `FakeKeyring` for CP4 tests) + 44 in `auth.rs` (mock HTTP client via `VecDeque<MockResp>`, injected `FakeKeyring`/`FailingKeyring`) + 20 in `loader_profile.rs` (fabric fixture parse + NeoForge fixture via `load_forge_profile`) + 14 in `forge_installer.rs` (injectable download+spawn closures; idempotency, argv, .part guard, launcher_profiles seeding, sink delivery) + 21 in `providers.rs` (CF key resolution, serde camelCase round-trips, mock seam, object-safety assertion) + 15 in `modrinth.rs` (URL construction, fixture mapping, filter logic, UA header, `CapturingMockClient`) + 26 in `curseforge.rs` (`split_game_versions` heuristic, key-absent guard, header assertion, fixture mapping, filter logic) + 5 in `lib.rs`. No live HTTP or real keyring in any test. No frontend tests. Component tests + Playwright planned Phase 7.
- **Conventions pointer:** folder/domain conventions live in these signals files (`.claude/project/signals/*.md`); regenerate with `/refresh-signals`. The legacy per-folder `SKILLS.md` + `HANDOFF.md` system has been removed.
- **Deterministic substrate:** `.claude/project/deterministic-signals.md`
- **Domain partitioning basis:** partitioned by functional workflow — instance lifecycle, metadata fetching, download execution, vanilla resolution, Java management, launch orchestration, Microsoft auth, UI shell, and provider search are orthogonal concerns that change independently. `src-tauri/src/lib.rs` is the Tauri command dispatch layer shared across all domains; changes there are driven by whichever domain adds a new command.
- **Stubs:** no remaining UI stubs. `Browse.tsx` is live (Phase 5 slice A). `Accounts.tsx` is live (Phase 3).
- **App data dir:** macOS `~/Library/Application Support/modloader/`, Windows `%APPDATA%\modloader\`, Linux `~/.local/share/modloader/`
