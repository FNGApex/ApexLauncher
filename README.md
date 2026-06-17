# Modloader

A lightweight, multiplatform Minecraft mod launcher with a modern UI — pull modpacks
from both **CurseForge** and **Modrinth**, manage instances, and launch the game.

> Status: 🏗️ early scaffolding. See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the plan.

## Stack

| Layer       | Choice                                                        |
|-------------|---------------------------------------------------------------|
| Shell       | [Tauri 2](https://tauri.app) (native webview, ~8MB bundles)   |
| Frontend    | React + TypeScript + Vite + Tailwind + shadcn/ui              |
| State/data  | Zustand (UI state) + TanStack Query (server cache)            |
| Backend     | Rust (downloads, instance mgmt, Java mgmt, launch, auth)      |
| Targets     | Windows, macOS, Linux                                         |

## Why Tauri

The brief is "lightweight + modern UI." Tauri uses the OS webview instead of bundling
Chromium, so we get a React/Tailwind UI (modern, fast to build) with a Rust backend and
single-digit-MB installers — far lighter than Electron, while keeping a real systems
language for the heavy lifting (concurrent hash-verified downloads, process management,
Microsoft auth).

## Prerequisites (dev)

- **Node ≥ 20** (you have v26 ✓)
- **Rust** (stable) via [rustup](https://rustup.rs) — *not yet installed*
- Platform deps: macOS needs Xcode CLT ✓; Linux needs `webkit2gtk` + `libsoup`;
  Windows needs WebView2 + MSVC build tools.
- A **CurseForge API key** (free, from the [CF console](https://console.curseforge.com))
  — CurseForge browse and pack import need one; Modrinth needs no key. The build
  bakes a key from a gitignored `src-tauri/.env` (`MODLOADER_CF_API_KEY=...`) at
  compile time, so distributed builds work out of the box. For a clean source
  build, add your own key to `src-tauri/.env`, or enter it at runtime under
  Settings → Advanced → API Keys.

## Quick start (once Rust is installed)

```bash
npm install
npm run tauri dev
```

## Docs

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — system design, subsystems, data model
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — phased build plan
- [`docs/PROVIDERS.md`](docs/PROVIDERS.md) — CurseForge & Modrinth API notes and gotchas
# ApexLauncher
