# Phase 7a — Installers / distributable packaging (spec)

Status: **Windows leg (IP-1→IP-4) implemented + verified on Windows** (commits cb44c9a,
c659739). macOS/Linux bundle config blocks landed (IP-5/IP-7) but artifacts build later on
their own hosts (`ip-f-mac` / `ip-f-linux`); IP-6 tarball-wrap step and IP-8 docs not started.
Design: `docs/design/phase7-installers.md`.

Contract: configure the Tauri v2 bundler + build entrypoint to emit shippable artifacts per
target. Each checkpoint is independently shippable. Gates respect host reality: **Windows MSI/NSIS
build on the existing WSL→Windows setup; macOS DMG and Linux AppImage/binary require a macOS host
and a Linux host respectively and cannot be built on this dev machine** — their "Done when" is
"config lands + a Linux/macOS host or CI produces the artifact", not a local build.

Single version source: `tauri.conf.json` `version` (`0.1.0`), kept in lockstep with `Cargo.toml`.
Artifacts ship **unsigned** in this slice (signing = separate Phase 7 slice).

**Resolved scope (human, 2026-06-18):** Windows = MSI **+** NSIS; macOS = `.dmg` unsigned (no Apple
Developer ID yet); Linux = AppImage **+** raw-binary `.tar.gz`, deb/rpm deferred, glibc floor
Ubuntu 22.04 / Debian 12. **No local mac/Linux host** — this slice lands config for every target and
is verified by `check`; mac/Linux artifact production + smoke-test are deferred to a later Linux VM
+ Mac (`ip-f-mac`, `ip-f-linux`). See design §8.

## Checkpoints

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **IP-1** | Per-platform `bundle` config blocks in `tauri.conf.json`: keep `bundle.targets: "all"`; add `bundle.windows` (wix + nsis present), `bundle.macOS` (dmg), `bundle.linux` (appimage). No format options beyond defaults yet. | `src-tauri/tauri.conf.json` | `scripts/build.sh check` passes (config parses); `bundle.windows`, `bundle.macOS`, `bundle.linux` keys present and schema-valid. |
| **IP-2** | `bundle` build mode forwarding `--bundles <list>` to `tauri build`. `scripts/build.sh bundle <fmt...>` → on WSL routes through the C: mirror + `.bat`; native passthrough on mac/Linux. `build` mode unchanged. | `scripts/build.sh`, `scripts/apex-build.bat` | `scripts/build.sh bundle --help`-equivalent path resolves; `scripts/build.sh bundle msi` reaches `tauri build --bundles msi` (dry-trace ok). |
| **IP-3** | **Windows MSI** builds + smoke-tested locally. Pre-req: VBSCRIPT optional feature enabled on the Windows builder. | (no source change; consumes IP-1/IP-2) | `scripts/build.sh bundle msi` on this machine produces `src-tauri/target/release/bundle/msi/ApexLauncher_0.1.0_x64_en-US.msi`; installs + launches on Windows. |
| **IP-4** | **Windows NSIS `.exe`** builds + smoke-tested locally. | (consumes IP-1/IP-2) | `scripts/build.sh bundle nsis` produces `.../bundle/nsis/ApexLauncher_0.1.0_x64-setup.exe`; per-user install + launch verified on Windows. |
| **IP-5** | **macOS `.dmg`** config block validated (universal vs per-arch left to the build/CI slice; default universal). | `src-tauri/tauri.conf.json` (macOS dmg block if options needed) | **This-slice gate:** config lands + `scripts/build.sh check` passes (schema-valid). **Deferred (`ip-f-mac`):** a Mac runs `tauri build --bundles dmg` → `ApexLauncher_0.1.0_<arch>.dmg` mounts → drag-to-Applications → launches. No local mac host. |
| **IP-6** | **Linux generic executable**: raw `target/release/modloader` + `.tar.gz` wrapping binary + `.desktop` + README under the `ApexLauncher` name. | `scripts/` (tarball/wrap step), `src-tauri/tauri.conf.json` (linux block) | **This-slice gate:** config + wrap step land; `scripts/build.sh check` passes. **Deferred (`ip-f-linux`):** a Linux VM (Ubuntu 22.04 base) builds the release binary + wrap → `ApexLauncher_0.1.0_amd64.tar.gz`; binary runs on a clean Ubuntu 22.04. No local Linux host. |
| **IP-7** | **Linux AppImage** config block validated (Ubuntu 22.04 base for glibc floor). | `src-tauri/tauri.conf.json` (linux appimage block) | **This-slice gate:** config lands + `scripts/build.sh check` passes. **Deferred (`ip-f-linux`):** a Linux VM runs `tauri build --bundles appimage` → `ApexLauncher_0.1.0_amd64.AppImage`; `chmod +x` + run launches on a clean Ubuntu 22.04. No local Linux host. |
| **IP-8** | Docs: `README` "Download / Install" + `docs/` note recording build hosts per target, version single-sourcing, unsigned-artifact caveat, and the deferred signing/CI/auto-update boundary. | `README.md`, this spec's change log | README lists the four artifact types + which host builds each; signing caveat + follow-up linked. |

## Notes / constraints (carried from design)

- **Host matrix** (Tauri v2 docs): MSI = Windows-only; NSIS = cross-compilable but built on the
  Windows host here; DMG = macOS-only; AppImage/deb/rpm/raw-binary = Linux-only. WSL cannot build
  any Linux/mac target (no GTK/WebKit) → IP-5/6/7 are host/CI-gated, not locally buildable.
- **Locally smoke-testable on this machine:** IP-1, IP-2, IP-3 (MSI), IP-4 (NSIS) — via the
  documented `scripts/build.sh` WSL→Windows path. IP-5/6/7 are not.
- **glibc floor:** Linux artifacts built on Ubuntu 22.04 / Debian 12 (WebKitGTK 4.1) so older
  distros don't hit `GLIBC_2.xx not found`.
- **`native-tls` dependency:** Linux build host needs OpenSSL dev headers until the separate
  `native-tls`→`rustls-tls` CI slice lands. Not changed here.
- **Out of scope:** signing/notarization, auto-update, the GitHub Actions CI matrix,
  `native-tls`→`rustls-tls`. Artifacts ship unsigned.

## Change log

- 2026-06-19 — **Windows leg shipped + verified.** IP-1 (per-platform bundle config), IP-2
  (`bundle <fmt...>` build mode in `build.sh` + `apex-build.bat` forwarding `--bundles`), IP-3
  (MSI), IP-4 (NSIS) all done. Both installers build, install, launch (app window confirmed),
  and uninstall cleanly on Windows. `apex-build.bat` now self-sources the MSVC dev env
  (`vcvarsall.bat`, direct-probed) so `rc.exe` (needed by `tauri-winres`) is on PATH — the
  native build no longer requires a Developer shell. Empirically confirmed space-separated
  `--bundles msi nsis` works with the installed tauri-cli. **Startup-panic bug uncovered by the
  IP-4 smoke test and fixed separately** (commit cb44c9a): `TaskManager::new` called
  `tokio::spawn` from Tauri's synchronous `.setup()` hook (no runtime entered) → release app
  exited 101 on launch; switched to `tauri::async_runtime::spawn` + added a plain-`#[test]`
  regression. Remaining: IP-5/IP-6/IP-7 mac/Linux artifact builds (config present), IP-8 docs.
- 2026-06-18 — Open questions resolved (human): both MSI + NSIS; macOS DMG unsigned (no Apple
  Developer ID); Linux AppImage + tarball, deb/rpm deferred; glibc floor Ubuntu 22.04 / Debian 12;
  no local mac/Linux host → IP-5/6/7 gate on config + `check` this slice, artifact build/test
  deferred to `ip-f-mac` / `ip-f-linux`. Spec ready for approval.
- 2026-06-18 — Initial spec drafted (planning only).
