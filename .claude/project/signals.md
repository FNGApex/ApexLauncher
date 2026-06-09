# Project signals

## Framework & runtime

- **Frontend:** React 19, TypeScript ~5.8, Vite 7, Tailwind v4, React Router v7, TanStack Query v5, Zustand v5 (installed, unused)
- **Backend:** Rust 1.96, Tauri 2; reqwest (native-tls, stream feature), serde/serde_json, uuid, chrono, sha1/sha2/hex, futures-util, tokio (sync + rt), flate2, tar, zip, tempfile (dev)
- **Targets:** macOS, Windows, Linux (cross-platform via Tauri)
- **App version:** 0.1.0 (pre-alpha); Phases 0–1 complete, version/loader metadata from Phase 2/4 pulled forward; Phase 2 slices A (download engine) + B (vanilla resolver) + C (Java manager) + D (vanilla launch) shipped; Phase 3 (MS auth, multi-account, keyring) shipped
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
| Rust | 9817 | 15 | 59% |
| JSON | 3316 | 11 | 20% |
| TypeScript | 1735 | 16 | 10% |
| Markdown | 1558 | 12 | 9% |
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
| metadata | `src-tauri/src/core/versions.rs`, `core/loaders.rs`, `core/meta.rs`, `src/lib/prefetch.ts` | MC version list + loader builds from Mojang/Forge/Fabric/Quilt/NeoForge, 6h disk cache | .claude/project/signals/metadata.md |
| download | `src-tauri/src/core/download.rs`, `src/lib/ipc.ts` (download types), `docs/spec/download-engine.md`, `docs/design/vanilla-launch.md` | Concurrent hash-verified download engine; SHA-1/SHA-256/SHA-512 verification; `.part` resume with TOCTOU guard; `download://progress` events; 37 unit tests | .claude/project/signals/download.md |
| resolver | `src-tauri/src/core/resolver.rs`, `src-tauri/src/core/fixtures/`, `src/lib/ipc.ts` (resolver types), `docs/spec/vanilla-resolver.md`, `docs/design/vanilla-launch.md` | Vanilla manifest → DownloadPlan + LaunchMeta; OS-filtered libs/natives/args; asset index expansion; 33 unit tests + 3 JSON fixtures | .claude/project/signals/resolver.md |
| java | `src-tauri/src/core/java.rs`, `src-tauri/src/core/store.rs` (java_dir), `src/lib/ipc.ts` (JavaInstallation/ensureJava), `docs/spec/java-manager.md`, `docs/design/vanilla-launch.md` | Detect system JRE or provision Temurin from Adoptium; traversal-safe tar.gz/zip extraction; `ensure_java` Tauri command; 45 unit tests + adoptium fixture | .claude/project/signals/java.md |
| launch | `src-tauri/src/core/launch.rs`, `src-tauri/src/lib.rs` (launch_instance/kill_instance commands + TauriLaunchSink), `src/routes/InstanceDetail.tsx` (Launch/Stop UI + log console), `src/lib/ipc.ts` (launch types/wrappers), `docs/spec/vanilla-launch.md`, `docs/design/vanilla-launch.md` | Argv assembly, natives extraction, JVM spawn + log streaming, running registry, kill, playtime accounting; 22 Rust tests (20 unit + 2 async) | .claude/project/signals/launch.md |
| auth | `src-tauri/src/core/auth.rs`, `src-tauri/src/lib.rs` (begin_login/cancel_login/list_accounts/remove_account/set_active_account commands), `src-tauri/src/core/store.rs` (accounts_file), `src/routes/Accounts.tsx`, `src/lib/ipc.ts` (auth types/wrappers), `docs/spec/authentication.md`, `docs/design/authentication.md` | MS OAuth2 device-code flow → Xbox chain → Minecraft identity; multi-account store; keyring-backed refresh tokens; 57 Rust unit tests (all mock HTTP + fake keyring) | .claude/project/signals/auth.md |
| frontend-shell | `src/main.tsx`, `src/router.tsx`, `src/components/AppShell.tsx`, `src/components/Sidebar.tsx`, `src/routes/Browse.tsx`, `src/routes/Accounts.tsx`, `src/routes/Settings.tsx`, `src/lib/ipc.ts`, `src/lib/query.ts`, `src/lib/utils.ts`, `src/styles.css` | App entry, routing, IPC wrapper layer, query client, settings UI; Browse stub | .claude/project/signals/frontend-shell.md |

## Cross-cutting

- **IPC type drift risk:** `src/lib/ipc.ts` hand-mirrors Rust structs (camelCase via `serde rename_all`). No generated types yet — specta/ts-rs planned in roadmap cross-cutting. Any Rust struct change requires a manual `ipc.ts` update.
- **Test layout:** ~186 Rust tests total — 31 in `download.rs` (hand-rolled `TcpListener` mock) + 33 in `resolver.rs` (fixture-based, 3 JSON fixtures under `src-tauri/src/core/fixtures/`) + 45 in `java.rs` (fixture-based, `adoptium_latest.json`, `tempfile` TempDirs, injected provision closure — no live HTTP) + 20 unit + 2 async in `launch.rs` (argv assembly, playtime, kill, spawn smoke — no JVM) + 57 in `auth.rs` (all mock HTTP via `MockAuthClient` VecDeque; keyring via `FakeKeyring`/`FailingKeyring`/`StoreOkDeleteFailKeyring` — no real TCP, no OS keyring). No frontend tests. Component tests + Playwright planned Phase 7.
- **Conventions pointer:** folder/domain conventions live in these signals files (`.claude/project/signals/*.md`); regenerate with `/refresh-signals`. The legacy per-folder `SKILLS.md` + `HANDOFF.md` system has been removed.
- **Deterministic substrate:** `.claude/project/deterministic-signals.md`
- **Domain partitioning basis:** partitioned by functional workflow — instance lifecycle, metadata fetching, download execution, vanilla resolution, Java management, launch orchestration, authentication, and UI shell are orthogonal concerns that change independently. `src-tauri/src/lib.rs` is the Tauri command dispatch layer shared across all domains; changes there are driven by whichever domain adds a new command.
- **Stubs:** `Browse.tsx` (Phase 5) is the only remaining UI placeholder.
- **App data dir:** macOS `~/Library/Application Support/modloader/`, Windows `%APPDATA%\modloader\`, Linux `~/.local/share/modloader/`
