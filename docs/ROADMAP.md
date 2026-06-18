# Roadmap

Phased so each phase ends with something runnable and demoable. Earlier phases de-risked the
hard parts (launch + auth) before polishing UI.

**Status: Phases 0–6 complete.** Active work is **Phase 7 — Polish & ship** plus the
cross-cutting items below.

## Phase 0 — Scaffold & shell ✅ DONE
- Tauri 2 + React 19 + TS + Vite 7 + Tailwind v4 project (shadcn primitives deferred).
- App window opens, nav shell (Instances / Browse / Accounts / Settings), routing.
- Typed IPC layer; `app_info` round-trip working end to end (Home "Connected" pill).
- **Done:** `npm run tauri dev` opens the app with working navigation. ✔

## Phase 1 — Instances & local management ✅ DONE
- `instance.json` model + on-disk layout, content-addressed cache dirs. ✔
- Create/list/get/delete instances from the UI; instance detail page (read-only mods list
  reconciled from the folder). ✔
- Settings store (memory, java args, dirs) + read-only data/instances paths on Settings. ✔
- **Done:** create an instance from the New Instance modal and see it on Home. ✔

### Pulled forward (from Phase 2/4) — version & loader metadata ✅
Done early so the create-instance flow picks real versions/builds:
- `core/versions.rs` — Mojang piston-meta release list.
- `core/loaders.rs` — per-MC loader builds (Fabric/Quilt/Forge/NeoForge), filtered to what
  each MC version supports.
- `core/meta.rs` — TTL'd (6h) disk-cached HTTP helper backing both.
- Frontend: `lib/query.ts` (shared query client, 6h meta stale-time), `lib/prefetch.ts`
  (warm cache on startup), `components/NewInstanceModal.tsx` (live dropdowns).

## Phase 2 — Minecraft install + launch (vanilla) ✅ DONE
- Mojang piston-meta client: versions ✅, libraries/asset index/natives ✅ (slice B resolver).
- Download engine: concurrent, hash-verified, content-addressed. ✅ (`core/download.rs` —
  Semaphore-bounded executor over a `DownloadPlan`, sha1/sha256/sha512 verify, content dedupe,
  Range resume, `download://progress` events. See `docs/spec/download-engine.md`.)
- Vanilla resolver (slice B): piston-meta version manifest + asset index → one `DownloadPlan`
  + `LaunchMeta`. ✅ (`core/resolver.rs` — typed parse, OS library-rule eval, classpath/natives
  selection, asset-object mapping, `resolve_vanilla` command. See `docs/spec/vanilla-resolver.md`.)
- Java manager (slice C): detect-or-provision a JRE per required major. ✅ (`core/java.rs` —
  release-file detection, Adoptium Temurin download via the engine (sha256), traversal-safe
  in-process tar.gz/zip extraction, `ensure_java` command. See `docs/spec/java-manager.md`.)
- Launch (slice D): build classpath + argv, extract natives, spawn the JVM, stream logs, track
  playtime. ✅ (`core/launch.rs` — placeholder substitution, offline identity (Player + UUIDv3),
  traversal-safe natives extraction, `tokio::process` spawn with cwd `mc/`, `launch://log`
  streaming, slug-keyed running registry, `launch_instance`/`kill_instance` commands, playtime
  on exit. `InstanceDetail` Launch/Stop + live console + playtime. See `docs/spec/vanilla-launch.md`.)
- **Done:** a vanilla instance launches and reaches the main menu — verified end-to-end (real
  MC + JRE + display). Pre-1.7 (`assets_legacy`) launch deferred — see follow-ups
  `vanilla-launch-f-1`/`-f-2`. ✔

## Phase 3 — Authentication ✅ DONE
- Microsoft device-code OAuth → MC token; profile fetch. ✔
- Single persistent account; refresh token in OS keychain; token refresh. ✔
- Azure client id registered and approved; online launch identity flows through to launch. ✔
- **Done:** log in with a real Microsoft account; account flows into launch identity. ✔

## Phase 4 — Mod loaders ✅ DONE
- Fabric + Quilt (meta APIs), then NeoForge + Forge (headless installer / maven). ✔ (slice A
  Fabric/Quilt + slice B NeoForge/Forge headless-installer launch — see
  `docs/spec/fabric-quilt-launch.md`, `docs/spec/neoforge-forge-launch.md`)
- **Done:** a Fabric/Quilt/NeoForge/Forge instance launches with the loader merged into the
  classpath + argv. ✔

## Phase 5 — Providers: browse & add mods ✅ DONE
- Modrinth + CurseForge clients behind the `ModProvider` trait. ✔ (slice A —
  `docs/spec/providers-browse.md`). CurseForge `x-api-key` resolves env →
  `settings.curseforge_api_key` → baked tier (`build.rs`), kept out of the frontend bundle and
  out of git.
- Browse page. ✔ **Reworked** (`docs/spec/ui-modpack-rework.md`): Browse is a single ordered
  **modpack** discovery feed across both providers (platform-badged, click opens the provider
  page), plus one-click in-app install (Phase 6 slice C). Mod search/add moved per-instance.
- Add a mod to an instance with dependency resolution; enable/disable/update; CF
  "download disabled" → open-in-browser fallback. ✔ (slice B — `docs/spec/mod-install.md`).
  Backend commands: `add_mod`, `set_mod_enabled`, `remove_mod`, `update_mod`. UI: per-instance
  **Manage installs** slide-over on `InstanceDetail`.
- **Done:** search, add a Modrinth + a CurseForge mod, and launch. ✔

## Phase 6 — Modpack import (the headline feature) ✅ DONE
- `.mrpack` import (Modrinth) — direct downloads + overrides. ✔ (slice A — `505670b`).
- CF zip import — per-file URL resolution + manual-download surfacing. ✔ (slice B —
  `cfbabf2`..`f6a6556`).
- Browse modpacks from both providers — unified discovery feed (opens provider page) **and**
  one-click in-app install from a Browse card. ✔ (slice C — `resolve_pack_file` →
  `install_modpack`, `ModpackInstallResult` tagged enum; `docs/spec/modpack-import.md`).
- Pack update / re-resolve — version picker + Pack Lock. ✔ (slice D — `update_modpack` /
  `set_pack_lock`; `docs/spec/modpack-import.md`).
- **Done:** install and update a CF and a Modrinth modpack end to end. ✔

### Download-runner-rework ✅ SHIPPED (post-Phase-6 hardening)
A serial-queue rework of all download-bearing operations, merged on `main`
(`docs/design/download-runner-rework.md`, `docs/spec/download-runner-rework/`):
- Serial FIFO `TaskManager` (single worker + `Arc<RwLock<snapshot>>`); pack/mod ops enqueue
  `TaskJob`s and return a task id immediately; terminal result rides a `task://update` event.
- `CancelToken` seam in the download engine; stage-and-promote (temp staging dir + atomic
  rename) for all instance-bound writes; cheap FS ops (enable/disable/remove) stay on an
  instant fast-path off the queue.
- `RunStatus`/`RunState` + per-run log ring; `PrepSemaphore` serializes prep; N-concurrent
  runner. Frontend: Zustand store, `DownloadManager` panel, `Toasts`, `RunningIndicator`.

## Phase 7 — Polish & ship (active)
- Instance icons, themes (dark/light), skin/cape preview, import from other launchers.
- Cross-platform CI builds (GitHub Actions: win/mac/linux), signing, auto-update. Switch
  reqwest `native-tls` → `rustls-tls` before CI to drop the OpenSSL build dependency.
- Error reporting, crash log parsing/help.

## Cross-cutting (ongoing)
- Generated TS types from Rust (specta/ts-rs) so IPC stays in sync — retires the standing
  `ipc.ts`/`store.ts` hand-mirror drift risk.
- Test fixtures for resolvers and pack import.
- Bounded concurrency, cancellation, and resumable downloads throughout.

---

### Suggested next action
Phases 0–6 are complete; the download-runner-rework hardening is merged. The natural next
target is **generated TS types (specta/ts-rs)** — a cross-cutting foundation that retires the
IPC hand-mirror drift risk every future feature would otherwise inherit — followed by the
**Phase 7 — Polish & ship** slices (icons, themes, crash-log parsing, then cross-platform CI,
signing, auto-update).
