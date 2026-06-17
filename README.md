# ApexLauncher

A lightweight, cross-platform Minecraft mod launcher with a modern UI. Browse and import
modpacks from both **CurseForge** and **Modrinth**, manage instances, install mods, and
launch the game.

> Status: pre-alpha (0.1.0). Active development, no released builds yet. See
> [`docs/ROADMAP.md`](docs/ROADMAP.md) for the plan.

## Features

- **One feed for both providers.** Browse modpacks from CurseForge and Modrinth in a single
  unified list, sorted by popularity.
- **Import any pack.** Drop in a `.mrpack` (Modrinth) or `.zip` (CurseForge) modpack, or
  install straight from the browse feed in one click.
- **Every loader.** Vanilla, Fabric, Quilt, Forge, and NeoForge instances.
- **Mod management.** Add, enable, disable, update, and remove mods per instance, with
  dependency resolution.
- **Hands-off Java.** Detects a system JRE or fetches the right Temurin build automatically.
- **Fast, safe downloads.** Concurrent, hash-verified downloads with resume support and a
  shared cache that deduplicates files across instances.
- **Sign in once.** Microsoft account login (device-code OAuth), stored securely in your
  OS keyring.
- **Play.** Launch with live log output, stop, and playtime tracking.

## Why Tauri

The goal is a lightweight launcher with a modern UI. ApexLauncher is built on
[Tauri 2](https://tauri.app), which uses the operating system's native webview instead of
bundling Chromium. That keeps installers in the single-digit-MB range, far smaller than an
Electron app, while a Rust backend handles the heavy lifting: concurrent downloads, process
management, and authentication.

| Layer    | Choice                                                       |
|----------|--------------------------------------------------------------|
| Shell    | Tauri 2 (native webview, small bundles)                      |
| Frontend | React 19 + TypeScript + Vite + Tailwind                      |
| Backend  | Rust                                                         |
| Targets  | Windows, macOS, Linux                                        |

## Where your data lives

Instances and caches live under a single folder so nothing is scattered across your system:

- macOS: `~/Library/Application Support/ApexLauncher/`
- Windows: `%APPDATA%\ApexLauncher\`
- Linux: `~/.local/share/ApexLauncher/`

Downloaded assets, libraries, and Java runtimes are shared across instances to save disk
space.

## Building from source

You will need [Node.js](https://nodejs.org) (≥ 20) and a stable
[Rust](https://rustup.rs) toolchain, plus the platform webview dependencies: Xcode Command
Line Tools on macOS, `webkit2gtk` and `libsoup` on Linux, and WebView2 with the MSVC build
tools on Windows.

```bash
scripts/build.sh dev      # run a dev window with hot reload
scripts/build.sh build    # produce an installable bundle
scripts/build.sh test     # run the test suite
```

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — system design, subsystems, data model
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — phased build plan
- [`docs/PROVIDERS.md`](docs/PROVIDERS.md) — CurseForge and Modrinth API notes
