# Project signals

## Framework & runtime

- **Frontend:** React 19, TypeScript ~5.8, Vite 7, Tailwind v4, React Router v7, TanStack Query v5, Zustand v5 (installed, unused)
- **Backend:** Rust 1.96, Tauri 2; reqwest (native-tls, stream feature), serde/serde_json, uuid, chrono, sha1/sha2/hex, futures-util, tokio (sync + rt)
- **Targets:** macOS, Windows, Linux (cross-platform via Tauri)
- **App version:** 0.1.0 (pre-alpha); Phases 0–1 complete, version/loader metadata from Phase 2/4 pulled forward; Phase 2 slices A (download engine) + B (vanilla resolver) shipped

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
| JSON | 3280 | 10 | 34% |
| Rust | 3952 | 12 | 41% |
| TypeScript | 1128 | 16 | 11% |
| Markdown | 911 | 8 | 9% |
| CSS | 92 | 1 | 1% |
| TOML | 36 | 1 | <1% |
| HTML | 13 | 1 | <1% |

## DevOps & CI

No CI pipeline yet. Cross-platform GitHub Actions builds planned for Phase 7. No signing or auto-update infrastructure.

---

## Domains

| Domain | Repo paths | One-liner | Detail |
|--------|------------|-----------|--------|
| instances | `src-tauri/src/core/instances.rs`, `core/settings.rs`, `core/store.rs`, `src/routes/Home.tsx`, `src/routes/InstanceDetail.tsx`, `src/components/NewInstanceModal.tsx` | Instance CRUD, on-disk layout, mods-folder reconciliation | .claude/project/signals/instances.md |
| metadata | `src-tauri/src/core/versions.rs`, `core/loaders.rs`, `core/meta.rs`, `src/lib/prefetch.ts` | MC version list + loader builds from Mojang/Forge/Fabric/Quilt/NeoForge, 6h disk cache | .claude/project/signals/metadata.md |
| download | `src-tauri/src/core/download.rs`, `src/lib/ipc.ts` (download types), `docs/spec/download-engine.md`, `docs/design/vanilla-launch.md` | Concurrent hash-verified download engine; SHA-1/SHA-512 verification; `.part` resume; `download://progress` events; 31 unit tests | .claude/project/signals/download.md |
| resolver | `src-tauri/src/core/resolver.rs`, `src-tauri/src/core/fixtures/`, `src/lib/ipc.ts` (resolver types), `docs/spec/vanilla-resolver.md`, `docs/design/vanilla-launch.md` | Vanilla manifest → DownloadPlan + LaunchMeta; OS-filtered libs/natives/args; asset index expansion; 33 unit tests + 3 JSON fixtures | .claude/project/signals/resolver.md |
| frontend-shell | `src/main.tsx`, `src/router.tsx`, `src/components/AppShell.tsx`, `src/components/Sidebar.tsx`, `src/routes/Browse.tsx`, `src/routes/Accounts.tsx`, `src/routes/Settings.tsx`, `src/lib/ipc.ts`, `src/lib/query.ts`, `src/lib/utils.ts`, `src/styles.css` | App entry, routing, IPC wrapper layer, query client, settings UI, stub routes | .claude/project/signals/frontend-shell.md |

## Cross-cutting

- **IPC type drift risk:** `src/lib/ipc.ts` hand-mirrors Rust structs (camelCase via `serde rename_all`). No generated types yet — specta/ts-rs planned in roadmap cross-cutting. Any Rust struct change requires a manual `ipc.ts` update.
- **Test layout:** 64 Rust unit tests total — 31 in `src-tauri/src/core/download.rs` (`#[cfg(test)]` module; hand-rolled `tokio::net::TcpListener` mock, no external HTTP-mock dep) + 33 in `src-tauri/src/core/resolver.rs` (fixture-based: 3 JSON files under `src-tauri/src/core/fixtures/`, no live HTTP). No frontend tests. Component tests + Playwright planned Phase 7.
- **Conventions pointer:** folder/domain conventions live in these signals files (`.claude/project/signals/*.md`); regenerate with `/refresh-signals`. The legacy per-folder `SKILLS.md` + `HANDOFF.md` system has been removed.
- **Deterministic substrate:** `.claude/project/deterministic-signals.md`
- **Domain partitioning basis:** partitioned by functional workflow — instance lifecycle, metadata fetching, download execution, vanilla resolution, and UI shell are orthogonal concerns that change independently. `src-tauri/src/lib.rs` is the Tauri command dispatch layer shared across all domains; changes there are driven by whichever domain adds a new command.
- **Stubs:** `Browse.tsx` (Phase 5) and `Accounts.tsx` (Phase 3) are UI placeholders with no backend wiring.
- **App data dir:** macOS `~/Library/Application Support/modloader/`, Windows `%APPDATA%\modloader\`, Linux `~/.local/share/modloader/`
