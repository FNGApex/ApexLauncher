# Roadmap

Phased so each phase ends with something runnable and demoable. Earlier phases de-risk the
hard parts (launch + auth) before polishing UI.

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
- **Still ⬜ for Phase 2:** libraries, asset index, natives, Java manager, and the actual
  launch. (Download engine ✅ — see below.)

## Phase 2 — Minecraft install + launch (vanilla)
- Mojang piston-meta client: versions ✅ (release list done), libraries/asset index/natives ✅
  (slice B resolver — see below).
- Java manager: detect system JREs, download Temurin per required major.
- Download engine: concurrent, hash-verified, content-addressed. ✅ (`core/download.rs` —
  Semaphore-bounded executor over a `DownloadPlan`, sha1/sha512 verify, content dedupe,
  Range resume, `download://progress` events. See `docs/spec/download-engine.md`.)
- Vanilla resolver (slice B): piston-meta version manifest + asset index → one `DownloadPlan`
  + `LaunchMeta`. ✅ (`core/resolver.rs` — typed parse, OS library-rule eval, classpath/natives
  selection, asset-object mapping, `resolve_vanilla` command. 33 tests. See
  `docs/spec/vanilla-resolver.md`.)
- Launch a **vanilla** instance; live log console; playtime tracking.
- **Done when:** a vanilla instance launches and reaches the main menu.

## Phase 3 — Authentication
- Microsoft device-code OAuth → MC token; profile fetch.
- Multi-account; tokens in OS keychain; token refresh.
- **Done when:** you log in with a real Microsoft account and launch online.

## Phase 4 — Mod loaders
- Fabric + Quilt (meta APIs, simplest), then NeoForge + Forge (installers/maven).
- Launch a modded-loader instance with no mods.
- **Done when:** a Fabric/NeoForge instance launches.

## Phase 5 — Providers: browse & add mods
- ⚠️ **Apply for the free CurseForge API key now** (<https://console.curseforge.com>) — it's
  the first phase that calls the CF API. Store it backend-side (env/Tauri secret), never in
  the frontend bundle; keep it out of git. Modrinth needs no key.
- Modrinth + CurseForge clients behind the `ModProvider` trait.
- Unified Browse page (search, provider filter, MC/loader facets, infinite scroll).
- Add a mod to an instance with dependency resolution; enable/disable/update; surface
  CF "download disabled" → open-in-browser fallback.
- **Done when:** you can search, add Sodium (Modrinth) + a CF mod, and launch.

## Phase 6 — Modpack import (the headline feature)
- `.mrpack` import (Modrinth) — direct downloads + overrides.
- CF zip import — per-file URL resolution + manual-download surfacing.
- Browse & one-click install modpacks from both providers.
- Pack update / re-resolve.
- **Done when:** you install a real CF and a real Modrinth modpack end to end.

## Phase 7 — Polish & ship
- Instance icons, themes (dark/light), skin/cape preview, import from other launchers.
- Cross-platform CI builds (GitHub Actions: win/mac/linux), signing, auto-update.
- Error reporting, crash log parsing/help.

## Cross-cutting (ongoing)
- Generated TS types from Rust (specta/ts-rs) so IPC stays in sync.
- Test fixtures for resolvers and pack import.
- Bounded concurrency, cancellation, and resumable downloads throughout.

---

### Suggested next action
Phases 0–1 run; version/loader metadata is wired into create-instance. Tackle the rest of
**Phase 2** next — download engine + Java manager + the vanilla launch path. It's the
riskiest piece and everything else (auth, loaders, packs) hangs off a real launch.
