# Architecture

## 1. High-level shape

```
┌──────────────────────────────────────────────────────────────┐
│                        Frontend (React)                       │
│  Routes: Home/Instances · Browse · Instance detail · Settings │
│          Accounts                                             │
│  State:  Zustand (UI)  +  TanStack Query (provider/API cache) │
└───────────────▲───────────────────────────┬──────────────────┘
                │ Tauri events (progress,    │ Tauri commands
                │ logs, status)              │ (invoke)
┌───────────────┴───────────────────────────▼──────────────────┐
│                        Backend (Rust)                         │
│                                                               │
│  commands/      thin IPC layer → calls into core              │
│  core/                                                        │
│    instances    create/read/update, instance.json model      │
│    providers    Modrinth + CurseForge clients (trait-based)   │
│    packs        .mrpack / CF-zip import + resolution          │
│    minecraft    Mojang piston-meta, assets, libraries         │
│    loaders      Fabric · Quilt · Forge · NeoForge meta        │
│    java         JRE detection + Adoptium download per MC ver  │
│    download     concurrent, hash-verified, resumable, cached  │
│    launch       classpath + JVM/game args, spawn, log capture │
│    auth         Microsoft device-code OAuth → MC token        │
│  store/         on-disk layout, settings, content cache       │
└───────────────────────────────────────────────────────────────┘
```

The frontend never talks to the network for *game* data (downloads, auth, launch) — all
of that lives in Rust where we have real concurrency, filesystem, and process control.
Provider **search/browse** calls can go either way; we'll route them through Rust too so
the CF API key never touches the webview and we get one caching layer.

## 2. On-disk layout

Cross-platform app data dir (via Tauri path API):

- macOS: `~/Library/Application Support/modloader/`
- Windows: `%APPDATA%\modloader\`
- Linux: `~/.local/share/modloader/`

```
modloader/
  settings.json                 # global settings
  accounts.json                 # account list (tokens in OS keychain, not here)
  java/                         # downloaded JREs, keyed by major version
    17/ 21/ 8/ ...
  meta-cache/                   # cached Mojang/loader manifests (TTL'd)
  assets/                       # shared Minecraft asset objects (content-addressed)
  libraries/                    # shared Maven libraries (content-addressed)
  instances/
    <slug>/
      instance.json             # our metadata (see §3)
      mc/                       # the actual .minecraft game dir
        mods/ config/ saves/ resourcepacks/ ...
      .cache/                   # per-instance scratch
```

Assets and libraries are **shared and content-addressed** so 10 instances on the same MC
version don't redownload gigabytes. The per-instance `mc/` dir holds only what must be
unique (mods, configs, worlds).

## 3. Instance data model (`instance.json`)

```jsonc
{
  "schema": 1,
  "id": "uuid",
  "name": "All the Mods 9",
  "slug": "all-the-mods-9",
  "icon": "atm9.png",
  "minecraft": "1.20.1",
  "loader": { "kind": "neoforge", "version": "47.1.106" },
  "java":   { "major": 17, "argsOverride": null, "memoryMb": 4096 },
  "source": {                       // where this pack came from (nullable)
    "provider": "curseforge",       // or "modrinth" | "manual"
    "projectId": "715572",
    "fileId":    "5169956",
    "packVersion": "0.5.7"
  },
  "mods": [                         // tracked managed content
    {
      "provider": "modrinth",
      "projectId": "AANobbMI",      // Sodium
      "versionId": "xyz",
      "fileName": "sodium-fabric-0.5.3.jar",
      "hashes": { "sha512": "..." },
      "enabled": true,
      "side": "client"
    }
  ],
  "created": "2026-06-06T00:00:00Z",
  "lastPlayed": null,
  "totalPlaytimeSec": 0
}
```

Manually dropped-in jars aren't in `mods[]`; we reconcile the folder on open so the UI
shows both managed and unmanaged mods.

## 4. Provider abstraction

A `ModProvider` trait both clients implement, so the UI is provider-agnostic:

```rust
trait ModProvider {
    async fn search(&self, q: SearchQuery) -> Result<Page<ProjectSummary>>;
    async fn project(&self, id: &ProjectId) -> Result<Project>;
    async fn versions(&self, id: &ProjectId, filter: VersionFilter) -> Result<Vec<Version>>;
    async fn resolve_pack(&self, file: PackRef) -> Result<ResolvedPack>;
}
```

Normalized domain types (`ProjectSummary`, `Version`, `ResolvedPack`) decouple the two
APIs' wildly different JSON shapes. See [`PROVIDERS.md`](PROVIDERS.md) for per-API detail
and gotchas (CF API key, CF "download disabled" mods, hash algorithms, modpack formats).

## 5. Download engine

- Single shared `reqwest` client, bounded concurrency (semaphore, ~8–16 in flight).
- Every file has an expected hash (sha1 for Mojang/Maven, sha512 for Modrinth, sha1/fingerprint for CF) → verify on completion, dedupe via content-addressed cache.
- Resumable via HTTP range where the server supports it.
- Progress + per-file status streamed to the UI via Tauri events (`download://progress`).
- One `DownloadPlan` (list of `(url, dest, hash, size)`) is produced by the resolver, then executed — separating "what to fetch" from "fetching" keeps it testable.

## 6. Launch sequence

1. Resolve MC version manifest (piston-meta) → libraries, asset index, main class, natives.
2. Merge loader manifest (Fabric/Forge/etc.) → extra libraries, patched main class, extra args.
3. Ensure the right **Java major** is present (download Temurin if missing).
4. Build classpath, extract natives, substitute arg placeholders (`${auth_player_name}`, `${classpath}`, etc.).
5. Inject auth (access token, uuid, xuid) from the active account.
6. Spawn the JVM with `mc/` as cwd; stream stdout/stderr to an in-app log console; track PID; record playtime on exit.

## 7. Authentication

Microsoft **OAuth 2.0 device-code** flow (no embedded browser needed):
`device code → poll for MS token → Xbox Live (XBL) → XSTS → Minecraft services token → profile`.
Refresh tokens stored in the OS keychain via the `keyring` crate (never on disk in plaintext).
Supports multiple accounts; offline/demo mode gated behind a setting.

## 8. Frontend structure

```
src/
  main.tsx, App.tsx, router.tsx
  lib/ipc.ts            # typed wrappers over Tauri invoke + event subscriptions
  lib/providers.ts      # TanStack Query hooks for search/browse
  stores/               # Zustand stores (instances, downloads, accounts)
  routes/
    Home.tsx            # instance grid, play buttons
    Browse.tsx          # unified CF+Modrinth search w/ provider filter
    Instance.tsx        # detail: mods, versions, settings, logs
    Settings.tsx
    Accounts.tsx
  components/ui/        # shadcn/ui primitives
```

Types shared across the IPC boundary are generated from Rust (via `ts-rs` or `specta`)
so the command signatures can't silently drift.

## 9. Testing strategy

- Rust: unit tests on resolvers/arg-building with recorded API fixtures; the download
  engine tested against a local mock server.
- Pack import: golden `.mrpack` and CF-zip fixtures → assert the produced `DownloadPlan`.
- Frontend: component tests for the browse/instance flows; Playwright smoke test on the
  built app later.
