# ApexLauncher

A lightweight, cross-platform Minecraft mod launcher with a modern UI. Browse and import
modpacks from **CurseForge**, **Modrinth**, **FTB**, and **ATLauncher**, manage instances,
install mods, and launch the game.

> Status: pre-alpha (0.1.0). Active development, no released builds yet. See
> [`docs/ROADMAP.md`](docs/ROADMAP.md) for the plan.

## Features

- **Four pack sources.** Browse modpacks from CurseForge, Modrinth, FTB, and ATLauncher, and
  install any of them in one click.
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

## Download & install

There is no published release yet. Every installer format below is built by the
[`Bundle`](.github/workflows/bundle.yml) GitHub Actions workflow, which runs on a `v*` tag push
or a manual dispatch, and uploads the results as workflow artifacts (retained 14 days). Until the
first tagged release lands, grab a build from the **Actions → Bundle** run you want and unzip the
artifact for your platform.

| Platform | Artifact | Built on | Install |
|----------|----------|----------|---------|
| Windows | `ApexLauncher_0.1.0_x64_en-US.msi` | `windows-latest` | Double-click; installs per-machine. Requires the VBSCRIPT optional Windows feature. |
| Windows | `ApexLauncher_0.1.0_x64-setup.exe` (NSIS) | `windows-latest` | Double-click; installs per-user, no admin prompt. Preferred if the MSI refuses to run. |
| macOS | `ApexLauncher_0.1.0_aarch64.dmg` | `macos-latest` (Apple Silicon) | Mount, drag to Applications. |
| macOS | `ApexLauncher_0.1.0_x64.dmg` | `macos-latest` (cross-compiled `x86_64-apple-darwin`) | Mount, drag to Applications. For Intel Macs. |
| Linux | `ApexLauncher_0.1.0_amd64.AppImage` | `ubuntu-22.04` | `chmod +x` then run. Self-contained. |
| Linux | `ApexLauncher_0.1.0_amd64.tar.gz` | `ubuntu-22.04` | Extract; contains the `ApexLauncher` binary, a `.desktop` entry, and this README. |

Each host builds only the formats that its own toolchain supports: MSI and NSIS are
Windows-only, DMG is macOS-only, and AppImage and the raw binary are Linux-only. There is no
cross-building between them, which is why the workflow fans out across three runner operating
systems. The Linux artifacts are built on Ubuntu 22.04 (WebKitGTK 4.1) to set the glibc floor;
they should run on Ubuntu 22.04 / Debian 12 and newer, but older distributions will fail with a
`GLIBC_2.xx not found` error.

The version number in every filename comes from a single source: the `version` field in
`src-tauri/tauri.conf.json`, kept in lockstep with `version` in `src-tauri/Cargo.toml`. Bump both
together before tagging a release.

### These builds are unsigned

Every artifact listed above ships without a code signature or notarization. This is deliberate
for the pre-alpha — code signing is a separate, still-unstarted slice of the roadmap — but it has
real consequences you should understand before installing.

On Windows, SmartScreen will show a blue "Windows protected your PC" warning, and you must click
"More info" and then "Run anyway" to proceed. On macOS, Gatekeeper will refuse to open the app
outright; you would have to right-click the app and choose "Open", or clear the quarantine
attribute manually, to launch it. Because the artifacts are unsigned, your operating system
cannot verify who produced them or that they have not been modified since they were built. Only
install a build you obtained yourself from a workflow run in this repository, and treat any
ApexLauncher installer that reaches you from anywhere else as untrusted.

Signing, notarization, and auto-update are tracked in [`docs/ROADMAP.md`](docs/ROADMAP.md); the
packaging details behind the table above are recorded in
[`docs/spec/phase7-installers.md`](docs/spec/phase7-installers.md) and
[`docs/spec/ci-pipeline.md`](docs/spec/ci-pipeline.md).

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
