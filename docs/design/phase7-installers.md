# Phase 7a — Installers / distributable packaging (design)

Status: proposed (planning only — not implemented)
Scope: produce installable/distributable artifacts for Windows, macOS, Linux.
Sibling spec: `docs/spec/phase7-installers.md`.

## 1. Goal

Turn `tauri build` output into shippable artifacts for all three desktop targets:

- **Windows** — MSI installer (WiX), and decide whether to also ship an NSIS `.exe`.
- **macOS** — a Mac package (`.dmg` vs `.app`/`.pkg`, universal vs per-arch).
- **Linux** — a **generic Linux executable** (explicit user request) plus optionally AppImage;
  decide `.deb`/`.rpm` scope.

This slice configures the *bundler* and the *build entrypoint*. It deliberately does **not**
ship signing, notarization, auto-update, or the GitHub Actions CI matrix — those are separate
Phase 7 slices (see §7 scope boundary).

## 2. Current state (evidence — local inspection)

- `src-tauri/tauri.conf.json`: `productName: "ApexLauncher"`, `version: "0.1.0"`,
  `identifier: "com.apex.apexlauncher"`, `bundle.active: true`, `bundle.targets: "all"`,
  icons reference `icon.icns` + `icon.ico` + PNGs. → verdict: bundler already on; `"all"`
  means every target the *host* can build is attempted.
- `src-tauri/Cargo.toml`: crate `name = "modloader"` (the produced binary stem is `modloader`,
  **not** `ApexLauncher`); `version = "0.1.0"`; `reqwest` uses `native-tls` (no `rustls`).
  → verdict: the raw Linux/portable binary will be named `modloader`; installer *product*
  name is `ApexLauncher` (from `productName`). Worth a naming decision (§5).
- `src-tauri/icons/`: full set present — `32x32.png`, `128x128.png`, `128x128@2x.png`,
  `icon.icns`, `icon.ico`, plus Windows Store `Square*Logo.png`. → verdict: icon assets are
  complete for WiX, NSIS, DMG, and AppImage; no icon work needed for this slice.
- `scripts/build.sh` + `scripts/apex-build.bat`: modes `check|test|build|dev`. `build` already
  runs `npm run tauri build` (→ `cargo tauri build`) and prints
  `src-tauri/target/release/bundle`. On WSL the whole build is mirrored to the native Windows
  FS and run via the `.bat`. → verdict: `build` already produces the Windows bundle on this
  machine; we add a thin `bundle` alias / `--bundles` forwarding rather than a new pipeline.
- `.claude/project/signals.md`: Tauri 2, reqwest native-tls, "No CI yet". WSL host cannot
  build the Linux Tauri target (no GTK/WebKit). → verdict: mac + Linux artifacts cannot be
  produced on this dev machine at all; they are host/CI dependencies, not local steps.

## 3. Cross-compilation reality (evidence — Tauri v2 bundler docs)

Tauri bundles are produced on the native OS of the target. Confirmed per-target:

| Target | Bundler | Host required | Cross-compile? | Source |
|---|---|---|---|---|
| Windows MSI | WiX Toolset v3 | **Windows only** | No ("can only be created on Windows") | distribute/windows-installer |
| Windows NSIS `.exe` | NSIS | any (Win/mac/Linux) | Yes (cross-compilable) | distribute/windows-installer |
| macOS `.dmg` / `.app` | tauri dmg/macos | **macOS only** ("run on a Mac computer") | No | distribute/dmg |
| Linux AppImage | linuxdeploy | **Linux only** | No (linuxdeploy can't cross-compile) | distribute/appimage |
| Linux `.deb`/`.rpm` | tauri linux bundlers | **Linux only** | No | distribute/ |
| Linux raw binary | cargo/rustc | **Linux only** (GTK/WebKit link) | (toolchain cross-compile out of scope) | — |

Consequences for THIS repo:

1. The documented WSL→Windows split already gives us a **Windows builder host** for free
   (`scripts/build.sh build`). MSI + NSIS both land there.
2. There is **no macOS host and no Linux host** in the current dev setup. macOS `.dmg` and the
   Linux generic binary / AppImage **cannot be built locally** — they require either a Mac and a
   Linux box, or the GitHub Actions matrix (a later Phase 7 slice). The config for them lands
   now; the *artifact* is gated on a host/CI that produces it.
3. AppImage glibc baseline: build on the **oldest** base providing WebKitGTK 4.1
   (Ubuntu 22.04 / Debian 12). Building on a newer distro raises the minimum glibc and breaks
   on older targets (`GLIBC_2.xx not found`). This is a CI-image decision recorded here so the
   later CI slice picks `ubuntu-22.04`, not `ubuntu-latest`.

## 4. Decisions & rationale

### 4.1 Windows — MSI (WiX) primary; NSIS also (recommended)

- **MSI (WiX)** is the baseline enterprise-friendly installer; required by the task. Windows-only,
  which we already have. Note: building MSI needs the **VBSCRIPT optional Windows feature** enabled
  on the builder (pre-req to record).
- **NSIS `.exe`** recommended *as well*: smaller, friendlier per-user install UX, current-user
  install without elevation, and cross-compilable (useful once CI exists). Low marginal cost —
  `bundle.windows` already produces both under `"targets": "all"`.
- **Chosen:** ship both MSI + NSIS. (Confirmed §8.)

### 4.2 macOS — `.dmg` (drag-to-Applications), unsigned this slice (universal vs per-arch → CI slice)

- **`.dmg` over `.pkg`/bare `.app`:** DMG is the conventional direct-download format for a
  non-App-Store launcher (Prism/MultiMC ship DMGs). `.pkg` is an installer-receipt flow aimed at
  managed deploys / App Store; overkill here. A bare `.app` isn't a distributable container.
- **Per-arch vs universal:** universal (`--target universal-apple-darwin`) needs both
  `aarch64` and `x86_64` Rust std + lipo; simplest under CI. For *this slice* we only commit the
  `dmg` target choice and config; whether the first real build is per-arch or universal is a
  CI-slice detail (recorded as open question §8). Default recommendation: **universal** for a
  single user-facing download.
- **Chosen:** `dmg` bundle target. Build host = macOS / CI macOS runner. Not buildable locally.

### 4.3 Linux — generic executable (primary, user-requested) + AppImage; no deb/rpm this slice

- The user explicitly asked for a **generic Linux executable**. Two interpretations, both
  delivered:
  - **Raw release binary** `target/release/modloader` — the literal portable executable. Plus a
    **`.tar.gz`** wrapping the binary + a `README`/`.desktop` for distribution (a binary alone has
    no icon/desktop integration). This is the "works on any glibc-compatible distro" artifact.
  - **AppImage** — the idiomatic Tauri "single self-contained Linux executable" (`.AppImage` is a
    runnable file). This is the closest match to "generic Linux executable" with bundled WebKit
    deps and desktop integration, and is what most launchers ship.
- **deb/rpm:** out of scope for this slice — they target specific package managers, not "generic",
  and add per-distro QA. Recommend deferring (open question §8 if the human wants them).
- **Chosen:** AppImage + raw-binary tarball. Build host = Linux (Ubuntu 22.04 base for glibc
  floor) / CI Linux runner. **Not buildable on this WSL machine** (no GTK/WebKit) — host/CI dep.

### 4.4 Versioning & artifact naming

- Single source of truth: `tauri.conf.json` `version` (`0.1.0`). Tauri stamps installers from it;
  `Cargo.toml` version is kept in lockstep (currently also `0.1.0`). Do **not** introduce a second
  version source.
- Output naming: Tauri derives installer names from `productName` (`ApexLauncher`) + version +
  arch (e.g. `ApexLauncher_0.1.0_x64_en-US.msi`, `ApexLauncher_0.1.0_x64-setup.exe`,
  `ApexLauncher_0.1.0_aarch64.dmg`, `ApexLauncher_0.1.0_amd64.AppImage`). The **raw Linux binary
  keeps the crate stem `modloader`** — the tarball step renames/wraps it to `ApexLauncher` for a
  consistent download name (decision: wrap, don't rename the crate).

### 4.5 Build entrypoint

- Extend `scripts/build.sh` + `scripts/apex-build.bat` with a **`bundle`** mode that forwards
  `--bundles <list>` to `tauri build`, so a builder can target a specific format
  (`scripts/build.sh bundle msi`, `... bundle nsis`). Existing `build` mode (no filter, honors
  `bundle.targets`) stays as the "everything the host can make" path. On WSL, `bundle` flows
  through the same C: mirror → `.bat` as `build`.

## 5. Approaches considered / rejected

- **Set `bundle.targets` to an explicit per-platform array vs keep `"all"`.** Chosen: keep
  `"all"` as the default (each host builds what it can) **and** add per-platform `bundle.windows`/
  `bundle.macOS`/`bundle.linux` config blocks for format-specific options. Rejected pinning a
  global explicit list because `"all"` already self-restricts to host-supported targets and avoids
  a target that the host can't build erroring the run.
- **MSI-only (drop NSIS).** Rejected: NSIS is near-free under `"all"` and gives a better
  non-admin install UX + future cross-compile. (Re-openable per §8.)
- **`.pkg` for macOS.** Rejected: receipt/managed-deploy flow, not a direct-download launcher norm.
- **deb/rpm now.** Rejected: per-distro QA cost, not "generic"; defer to a packaging follow-up.
- **Build mac/Linux locally via cross toolchains.** Rejected as infeasible here: DMG and AppImage
  tooling are host-locked per Tauri docs (§3); WSL lacks GTK/WebKit. These are CI/host deps.
- **`native-tls` → `rustls-tls` switch in this slice.** Rejected as belonging to the CI slice
  (ROADMAP lists it under CI to drop the OpenSSL build dep). Flagged as a Linux-build dependency:
  the Linux CI image must provide OpenSSL dev headers until that switch lands.

## 6. Risks

- **MSI build pre-req:** VBSCRIPT optional feature must be enabled on the Windows builder, else
  MSI bundling fails. Record in spec "Done when".
- **Binary stem ≠ product name** (`modloader` vs `ApexLauncher`): only affects the raw Linux
  tarball naming; handled by the wrap step (§4.4).
- **DMG icon positions ignored on CI** (documented Tauri limitation) — cosmetic; accept.
- **glibc floor:** wrong CI base image silently breaks old distros; pinned to Ubuntu 22.04 (§3).

## 7. Scope boundary (vs other Phase 7 slices)

IN this slice: `bundle.targets`/per-platform bundle config; `bundle` build mode; artifact
naming; version single-sourcing; documenting which artifacts build on which host; local MSI/NSIS
smoke test on the existing WSL→Windows setup.

OUT (separate Phase 7 slices):
- **CI matrix** (GitHub Actions win/mac/linux runners producing mac + Linux artifacts).
- **Signing/notarization** — Windows Authenticode, macOS Developer ID + notarization. This slice
  ships **unsigned** artifacts (acceptable for pre-alpha 0.1.0); the spec records the follow-up.
- **Auto-update** (updater plugin, signed update manifests).
- **`native-tls` → `rustls-tls`** switch (CI slice; only a *dependency note* here).

## 8. Resolved decisions (human, 2026-06-18)

1. **NSIS as well as MSI?** → **Yes — ship both** MSI + NSIS.
2. **macOS Apple Developer ID?** → **Not available — DMG ships unsigned** this slice (Gatekeeper
   warning on first open; signing deferred to the CI/signing slice). Universal-vs-per-arch left
   to the build/CI slice (default universal).
3. **Linux `.deb`/`.rpm`?** → **Deferred** — AppImage + raw-binary `.tar.gz` only.
4. **Linux glibc baseline** → **Ubuntu 22.04 / Debian 12** (WebKitGTK 4.1) — oldest base, widest floor.
5. **macOS / Linux host availability?** → **None locally now.** This slice lands **config for all
   targets**; mac/Linux *artifact production + smoke-test* is deferred to a later **Linux VM + Mac**
   (follow-ups `ip-f-mac`, `ip-f-linux`). Only Windows (MSI/NSIS) is locally verifiable on the
   current WSL→Windows host.
