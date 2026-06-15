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
- Java manager: detect system JREs, download Temurin per required major. ✅ (slice C — see below).
- Download engine: concurrent, hash-verified, content-addressed. ✅ (`core/download.rs` —
  Semaphore-bounded executor over a `DownloadPlan`, sha1/sha256/sha512 verify, content dedupe,
  Range resume, `download://progress` events. See `docs/spec/download-engine.md`.)
- Vanilla resolver (slice B): piston-meta version manifest + asset index → one `DownloadPlan`
  + `LaunchMeta`. ✅ (`core/resolver.rs` — typed parse, OS library-rule eval, classpath/natives
  selection, asset-object mapping, `resolve_vanilla` command. 33 tests. See
  `docs/spec/vanilla-resolver.md`.)
- Java manager (slice C): detect-or-provision a JRE per required major. ✅ (`core/java.rs` —
  release-file detection, Adoptium Temurin download via the engine (sha256), traversal-safe
  in-process tar.gz/zip extraction, `ensure_java` command. 45 tests. See
  `docs/spec/java-manager.md`.)
- Launch (slice D): build classpath + argv, extract natives, spawn the JVM, stream logs, track
  playtime. ✅ verified (`core/launch.rs` — placeholder substitution, offline identity
  (Player + UUIDv3), traversal-safe natives extraction, `tokio::process` spawn with cwd `mc/`,
  `launch://log` streaming, slug-keyed running registry, `launch_instance`/`kill_instance`
  commands, playtime on exit. `InstanceDetail` Launch/Stop + live console + playtime. 5 Rust
  tests. See `docs/spec/vanilla-launch.md`.)
- **Done when:** a vanilla instance launches and reaches the main menu. ✅ verified end-to-end
  (real MC + JRE + display). Pre-1.7 (`assets_legacy`) launch deferred — see follow-ups
  `vanilla-launch-f-1`/`-f-2`.

## Phase 3 — Authentication ✅ DONE
- Microsoft device-code OAuth → MC token; profile fetch. ✔
- Multi-account; tokens in OS keychain; token refresh. ✔
- **Done:** log in with a real Microsoft account; active account flows into launch identity. ✔

## Phase 4 — Mod loaders
- Fabric + Quilt (meta APIs, simplest), then NeoForge + Forge (installers/maven). ✔ (slice A
  Fabric/Quilt + slice B NeoForge/Forge headless-installer launch — see
  `docs/spec/fabric-quilt-launch.md`, `docs/spec/neoforge-forge-launch.md`)
- Launch a modded-loader instance with no mods. ✔ code-complete; NeoForge/Forge manual e2e
  pending (see neoforge-forge-launch spec Implementation log)
- **Done when:** a Fabric/NeoForge instance launches. Fabric ✔; NeoForge pending manual run.

## Phase 5 — Providers: browse & add mods
- ⚠️ **CurseForge API key applied for** (<https://console.curseforge.com>) — approval
  pending (~48-72h). Store it backend-side (env `MODLOADER_CF_API_KEY` or
  `settings.curseforge_api_key`), never in the frontend bundle; keep it out of git. Inject as
  a compile-time/build secret for distributed binaries (the Prism pattern). Modrinth needs no
  key.
- Modrinth + CurseForge clients behind the `ModProvider` trait. ✔ (slice A —
  `docs/spec/providers-browse.md`)
- Unified Browse page (search, provider filter, MC/loader facets, infinite scroll). ✔ code-complete;
  manual UI verify + live CF run (needs the API key) pending
- Add a mod to an instance with dependency resolution; enable/disable/update; surface
  CF "download disabled" → open-in-browser fallback. ✔ (slice B —
  `docs/spec/mod-install.md`). Modrinth path complete (no key); CF rides the same code path,
  gated only by the pending key. Backend commands: `add_mod`, `set_mod_enabled`, `remove_mod`,
  `update_mod`. UI: Browse add-to-instance modal + InstanceDetail per-mod controls.
- **Done when:** you can search, add Sodium (Modrinth) + a CF mod, and launch. Modrinth
  add-and-launch ready for manual verification; CF add pending the API key.

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
Phase 5 is code-complete: slice A (Browse) and slice B (mod install — add/enable/disable/
update + CF download-disabled fallback) both shipped. Modrinth path is fully functional with
no key; manual verification (add Sodium, launch) is the open box for slice B. Three external
gates pending in parallel: CurseForge API key approval (applied, ~48-72h — unblocks the CF
half of Browse + install), Mojang app-review for the MS-auth client id
(`docs/design/auth-client-id-blocker.md`), and NeoForge/Forge manual e2e from Phase 4. Next
build phase is **Phase 6 — Modpack import** (the headline feature), which reuses the
`ModProvider` trait + normalized types as its pack-resolver substrate.
