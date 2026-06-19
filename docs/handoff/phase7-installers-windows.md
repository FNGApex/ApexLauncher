# Handoff — Phase 7a Installers · Windows leg (MSI + NSIS)

> For a fresh Claude Code session running **natively on Windows**. Windows is the only locally
> buildable target this slice; macOS and Linux get their own handoffs (`ip-f-mac`, `ip-f-linux`).

## Where things stand (as of 2026-06-18)

- Branch `main`, pushed to `origin`. Complete and merged: ts-type-generation CP-1→6 (tauri-specta,
  `src/lib/bindings.ts` is the generated source of truth), roadmap restyle, signals refresh.
- **Phase 7a installer plan is approved.** Authoritative docs:
  - Design: `docs/design/phase7-installers.md`
  - Spec (checkpoint table): `docs/spec/phase7-installers.md`
- No installer code written yet — this leg starts at IP-1.

## Decisions locked (do NOT relitigate)

- **Windows = MSI + NSIS** (both). Unsigned this slice — code-signing is a later Phase 7 slice.
- Version single-sourced from `src-tauri/tauri.conf.json` `version` = `0.1.0`; product name
  `ApexLauncher`; bundle id `com.apex.apexlauncher`. (Crate/binary stem is `modloader` — only
  affects the raw Linux binary name, not Windows artifacts.)
- macOS DMG (unsigned) and Linux AppImage + raw-binary `.tar.gz` are config-only this slice and
  built later on a Mac / Linux VM. deb/rpm deferred. glibc floor Ubuntu 22.04 / Debian 12.

## ⚠️ Required pre-req before IP-3 (MSI)

Tauri's WiX MSI bundler needs the **VBSCRIPT optional Windows feature** enabled on this builder
(surfaced from the Tauri v2 windows-installer docs). Enable it first:
- Settings → System → Optional features → Add an optional feature → search **VBSCRIPT** → install.
- (Or via elevated PowerShell DISM; reboot if prompted.)
NSIS (IP-4) does not need this.

## Environment notes (native Windows, not WSL)

- Prior sessions built from WSL by mirroring the tree to `C:\Users\drgor\Documents\GitHub\ApexLauncher`
  and running `scripts/apex-build.bat` there. **On native Windows you build in place** — no mirror.
- Toolchain (per `CLAUDE.md`): cargo at `C:\Users\drgor\.cargo\bin\cargo.exe`, Node at
  `C:\Program Files\nodejs`. `scripts/apex-build.bat` puts both on PATH; modes: `check|test|build|dev`.
- `scripts/build.sh` is bash — run it from Git Bash, or call `apex-build.bat` directly from cmd/PowerShell.
- First `npm install` / dep build is slow; incremental rebuilds are fast.

## Checkpoints for this leg (from `docs/spec/phase7-installers.md`)

| CP | Do | Gate |
|----|----|------|
| **IP-1** | Add per-platform `bundle` config to `src-tauri/tauri.conf.json` (keep `bundle.targets: "all"`; add `bundle.windows` with wix + nsis, plus the `bundle.macOS` dmg + `bundle.linux` appimage blocks so all targets are configured now). | `check` passes; keys schema-valid. |
| **IP-2** | Add a `bundle` mode to `scripts/build.sh` + `scripts/apex-build.bat` forwarding `--bundles <list>` to `tauri build`. Leave `build` mode unchanged. | `scripts/build.sh bundle msi` reaches `tauri build --bundles msi`. |
| **IP-3** | Build the **MSI** (needs VBSCRIPT pre-req above). | `…/target/release/bundle/msi/ApexLauncher_0.1.0_x64_en-US.msi` exists; installs + launches to the app window. |
| **IP-4** | Build the **NSIS** `.exe`. | `…/target/release/bundle/nsis/ApexLauncher_0.1.0_x64-setup.exe` exists; per-user install + launch verified. |

## Commands

```
# typecheck both sides (cargo check + tsc)
scripts/build.sh check          # or: scripts\apex-build.bat check

# after IP-2 lands the bundle mode:
scripts/build.sh bundle msi nsis

# or invoke the bundler directly (works before IP-2):
npm run tauri build -- --bundles msi nsis
```

Expected output dir: `src-tauri/target/release/bundle/{msi,nsis}/`.

## How to resume

1. `git pull` on `main` (this handoff + the plan docs are committed there).
2. Read `CLAUDE.md`, this file, then `docs/spec/phase7-installers.md`.
3. Enable the VBSCRIPT feature (pre-req above).
4. Run `/ax-implement` against the spec, scoped to **IP-1 → IP-4** (the Windows leg). IP-1/IP-2 are
   pure config/script and gate on `check`; IP-3/IP-4 produce + smoke-test the real installers.
5. Commit per green checkpoint; push so the Mac/Linux legs inherit the bundle config.

## Out of scope this leg

macOS DMG build, Linux AppImage/tarball build, code-signing/notarization, GitHub Actions CI matrix,
`native-tls`→`rustls-tls`, auto-update. Tracked as separate Phase 7 slices / follow-ups.
