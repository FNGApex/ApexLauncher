# Roadmap

Phased so each phase ends with something runnable and demoable. Earlier phases de-risk the
hard parts (launch + auth) before polishing UI.

## Phase 0 — Scaffold & shell ✅ DONE
- Tauri 2 + React 19 + TS + Vite 7 + Tailwind v4 project (shadcn primitives deferred).
- App window opens, nav shell (Instances / Browse / Accounts / Settings), routing.
- Typed IPC layer; `app_info` round-trip working end to end (Home "Connected" pill).
- **Done:** `npm run tauri dev` opens the app with working navigation. ✔

## Phase 1 — Instances & local management
- `instance.json` model + on-disk layout, content-addressed cache dirs.
- Create/list/delete instances from the UI; instance detail page (read-only mods list
  reconciled from the folder).
- Settings store (memory, java args, dirs).
- **Done when:** you can create an empty instance and see it on Home.

## Phase 2 — Minecraft install + launch (vanilla)
- Mojang piston-meta client: versions, libraries, asset index, natives.
- Java manager: detect system JREs, download Temurin per required major.
- Download engine: concurrent, hash-verified, content-addressed.
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
Install Rust (`rustup`) and finish Phase 0 to a running app, then tackle Phase 2's
launch path early — it's the riskiest piece and everything else hangs off a real launch.
