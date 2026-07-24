# Roadmap

Phased so each phase ends with something runnable and demoable. Earlier phases de-risked the
hard parts (launch + auth) before polishing UI.

**Status legend:** `✅ Done` · `🚧 Active` · `⬜ Planned` — per item, `- [x]` shipped / `- [ ]` outstanding.

**Overall:** Phases 0–6 ✅, download-runner-rework ✅, generated TS types ✅. Active work is
**Phase 7 — Polish & ship** — icons/themes, launcher import, installers, and CI shipped;
crash-log help, skin/cape preview, signing/auto-update remain.

## Phase 0 — Scaffold & shell — ✅ Done
- [x] Tauri 2 + React 19 + TS + Vite 7 + Tailwind v4 project (shadcn primitives deferred).
- [x] App window opens, nav shell (Instances / Browse / Accounts / Settings), routing.
- [x] Typed IPC layer; `app_info` round-trip working end to end (Home "Connected" pill).
- **Demo:** `npm run tauri dev` opens the app with working navigation.

## Phase 1 — Instances & local management — ✅ Done
- [x] `instance.json` model + on-disk layout, content-addressed cache dirs.
- [x] Create/list/get/delete instances from the UI; instance detail page (read-only mods list
  reconciled from the folder).
- [x] Settings store (memory, java args, dirs) + read-only data/instances paths on Settings.
- **Demo:** create an instance from the New Instance modal and see it on Home.

### Pulled forward (from Phase 2/4) — version & loader metadata — ✅ Done
Done early so the create-instance flow picks real versions/builds:
- [x] `core/versions.rs` — Mojang piston-meta release list.
- [x] `core/loaders.rs` — per-MC loader builds (Fabric/Quilt/Forge/NeoForge), filtered to what
  each MC version supports.
- [x] `core/meta.rs` — TTL'd (6h) disk-cached HTTP helper backing both.
- [x] Frontend: `lib/query.ts` (shared query client, 6h meta stale-time), `lib/prefetch.ts`
  (warm cache on startup), `components/NewInstanceModal.tsx` (live dropdowns).

## Phase 2 — Minecraft install + launch (vanilla) — ✅ Done
- [x] Mojang piston-meta client: versions, libraries/asset index/natives (slice B resolver).
- [x] Download engine: concurrent, hash-verified, content-addressed. (`core/download.rs` —
  Semaphore-bounded executor over a `DownloadPlan`, sha1/sha256/sha512 verify, content dedupe,
  Range resume, `download://progress` events. See `docs/spec/download-engine.md`.)
- [x] Vanilla resolver (slice B): piston-meta version manifest + asset index → one `DownloadPlan`
  + `LaunchMeta`. (`core/resolver.rs` — typed parse, OS library-rule eval, classpath/natives
  selection, asset-object mapping, `resolve_vanilla` command. See `docs/spec/vanilla-resolver.md`.)
- [x] Java manager (slice C): detect-or-provision a JRE per required major. (`core/java.rs` —
  release-file detection, Adoptium Temurin download via the engine (sha256), traversal-safe
  in-process tar.gz/zip extraction, `ensure_java` command. See `docs/spec/java-manager.md`.)
- [x] Launch (slice D): build classpath + argv, extract natives, spawn the JVM, stream logs, track
  playtime. (`core/launch.rs` — placeholder substitution, offline identity (Player + UUIDv3),
  traversal-safe natives extraction, `tokio::process` spawn with cwd `mc/`, `launch://log`
  streaming, slug-keyed running registry, `launch_instance`/`kill_instance` commands, playtime
  on exit. `InstanceDetail` Launch/Stop + live console + playtime. See `docs/spec/vanilla-launch.md`.)
- **Demo:** a vanilla instance launches and reaches the main menu — verified end-to-end (real
  MC + JRE + display). Pre-1.7 (`assets_legacy`) launch deferred — see follow-ups
  `vanilla-launch-f-1`/`-f-2`.

## Phase 3 — Authentication — ✅ Done
- [x] Microsoft device-code OAuth → MC token; profile fetch.
- [x] Single persistent account; refresh token in OS keychain; token refresh.
- [x] Azure client id registered and approved; online launch identity flows through to launch.
- **Demo:** log in with a real Microsoft account; account flows into launch identity.

## Phase 4 — Mod loaders — ✅ Done
- [x] Fabric + Quilt (meta APIs), then NeoForge + Forge (headless installer / maven). (slice A
  Fabric/Quilt + slice B NeoForge/Forge headless-installer launch — see
  `docs/spec/fabric-quilt-launch.md`, `docs/spec/neoforge-forge-launch.md`)
- **Demo:** a Fabric/Quilt/NeoForge/Forge instance launches with the loader merged into the
  classpath + argv.

## Phase 5 — Providers: browse & add mods — ✅ Done
- [x] Modrinth + CurseForge clients behind the `ModProvider` trait. (slice A —
  `docs/spec/providers-browse.md`). CurseForge `x-api-key` resolves env →
  `settings.curseforge_api_key` → baked tier (`build.rs`), kept out of the frontend bundle and
  out of git.
- [x] Browse page — **reworked** (`docs/spec/ui-modpack-rework.md`): a single ordered
  **modpack** discovery feed across both providers (platform-badged, click opens the provider
  page), plus one-click in-app install (Phase 6 slice C). Mod search/add moved per-instance.
- [x] Add a mod to an instance with dependency resolution; enable/disable/update; CF
  "download disabled" → open-in-browser fallback. (slice B — `docs/spec/mod-install.md`).
  Backend commands: `add_mod`, `set_mod_enabled`, `remove_mod`, `update_mod`. UI: per-instance
  **Manage installs** slide-over on `InstanceDetail`.
- **Demo:** search, add a Modrinth + a CurseForge mod, and launch.

## Phase 6 — Modpack import (the headline feature) — ✅ Done
- [x] `.mrpack` import (Modrinth) — direct downloads + overrides. (slice A — `505670b`).
- [x] CF zip import — per-file URL resolution + manual-download surfacing. (slice B —
  `cfbabf2`..`f6a6556`).
- [x] Browse modpacks from both providers — unified discovery feed (opens provider page) **and**
  one-click in-app install from a Browse card. (slice C — `resolve_pack_file` →
  `install_modpack`, `ModpackInstallResult` tagged enum; `docs/spec/modpack-import.md`).
- [x] Pack update / re-resolve — version picker + Pack Lock. (slice D — `update_modpack` /
  `set_pack_lock`; `docs/spec/modpack-import.md`).
- **Demo:** install and update a CF and a Modrinth modpack end to end.

### Download-runner-rework — ✅ Done (post-Phase-6 hardening)
A serial-queue rework of all download-bearing operations, merged on `main`
(`docs/design/download-runner-rework.md`, `docs/spec/download-runner-rework/`):
- [x] Serial FIFO `TaskManager` (single worker + `Arc<RwLock<snapshot>>`); pack/mod ops enqueue
  `TaskJob`s and return a task id immediately; terminal result rides a `task://update` event.
- [x] `CancelToken` seam in the download engine; stage-and-promote (temp staging dir + atomic
  rename) for all instance-bound writes; cheap FS ops (enable/disable/remove) stay on an
  instant fast-path off the queue.
- [x] `RunStatus`/`RunState` + per-run log ring; `PrepSemaphore` serializes prep; N-concurrent
  runner. Frontend: Zustand store, `DownloadManager` panel, `Toasts`, `RunningIndicator`.

### Generated TS types (tauri-specta) — ✅ Done (cross-cutting foundation)
Retires the standing `ipc.ts`/`store.ts` hand-mirror drift risk (CP-1→6, merged on `main`;
`docs/spec/ts-type-generation.md`):
- [x] `src/lib/bindings.ts` — committed, generated by tauri-specta via `make_builder()`; the
  single source of truth for all 35 commands, 8 event channels, and DTOs. Regenerated on
  Windows via `scripts/build.sh dev` (`#[cfg(debug_assertions)]` startup export) + a Linux-only
  export test for future CI.
- [x] `src/lib/store.ts` imports its task/run types from `bindings.ts` (no hand-mirrored decls).
- [x] `src/lib/ipc.ts` is a thin adapter over generated `commands.*` / `events.*` (an `unwrap()`
  helper restores reject-on-error); zero hand-declared IPC types remain. Residual specta
  artifacts absorbed by the adapter (`LoaderKind`→`string`, `Task` S/D split,
  `AccountMeta`→`AccountMeta_Serialize`).

## Phase 7 — Polish & ship — 🚧 Active
- [x] Instance icons + themes (dark/light) — shipped (session 2026-06-26 batch).
- [x] Import from other launchers — Prism/MultiMC/PolyMC instance import
  (`core/launcher_import.rs`, "From launcher" modal tab; merged `ddf3fa4`). CP-8
  (ATLauncher-launcher import) deferred.
- [x] **Installers (Phase 7a).** Windows MSI + NSIS verified (IP-1→IP-4, unsigned) —
  `docs/spec/phase7-installers.md`. macOS DMG ×2 + Linux AppImage/tarball build in CI.
- [x] Cross-platform CI (GitHub Actions): `test.yml` (PR gate + 3-OS push-to-main) +
  `bundle.yml` (installers on `v*` tags / dispatch) — `docs/spec/ci-pipeline.md`. reqwest
  switched `native-tls` → `rustls-tls`.
- [ ] Skin/cape preview.
- [ ] Error reporting, crash log parsing/help.
- [ ] Code signing + auto-update.
- [ ] IP-8 README installer docs.

## Cross-cutting — ✅ Done (maintenance ongoing)
- [x] Generated TS types from Rust (tauri-specta) so IPC stays in sync — see the dedicated
  section above. Maintenance: regenerate `bindings.ts` whenever a Rust DTO/command/event changes.
- [x] Test fixtures for resolvers and pack import (resolver JSON fixtures, provider/adoptium
  fixtures, modpack import tests).
- [x] Bounded concurrency, cancellation, and resumable downloads throughout (Semaphore-bounded
  engine, `CancelToken` seam, `.part` Range resume).

---

### Suggested next action
Phase 7 is mostly shipped: icons/themes, launcher import, installers, and cross-platform CI
are all on `main`. Remaining slices, roughly in value order: **crash-log parsing/help** (last
substantial user-facing feature), skin/cape preview (small), then release infrastructure
(code signing, auto-update, IP-8 README installer docs) when a public release is near.
