# Stretch goals (parked)

Work that is real and planned but **deliberately deferred** so it doesn't distract from the
current focus (Windows functionality + UI). Pick these up only when explicitly resumed.

---

## SG-1 · macOS + Linux installer artifact builds (Phase 7a tail)

**Status:** config landed, artifact builds parked. **Blocker:** no local Mac/Linux host.

The Windows installer leg (IP-1→IP-4: MSI + NSIS) is **done and verified**. The per-platform
bundle config for every target already lives in `src-tauri/tauri.conf.json` (`bundle.macOS.dmg`,
`bundle.linux.appimage`). What remains is host-gated and parked:

| CP | What | Why parked |
|----|------|------------|
| IP-5 (build) | macOS `.dmg` artifact build + smoke test | needs a Mac host (`ip-f-mac`) |
| IP-6 | Linux generic-executable **tarball/wrap step** (`scripts/`) + linux block; then build | wrap step is locally writable but unverifiable; build needs a Linux host (`ip-f-linux`) |
| IP-7 (build) | Linux AppImage artifact build + smoke test | needs a Linux host (`ip-f-linux`) |
| IP-8 | README "Download / Install" section + per-host build notes | do alongside the mac/Linux legs so the doc lists real artifacts |

Authoritative spec: `docs/spec/phase7-installers.md`. Design: `docs/design/phase7-installers.md`.
Windows handoff (complete): `docs/handoff/phase7-installers-windows.md`.

**To resume:** stand up a Linux VM (Ubuntu 22.04 glibc floor) and/or a Mac, then run
`/ax-implement` scoped to IP-5/IP-6/IP-7/IP-8. Note the `native-tls`→`rustls-tls` swap is wanted
before Linux CI to avoid the OpenSSL build dep.

---

## SG-2 · Other deferred Phase 7 slices

Tracked in `docs/spec/phase7-installers.md` "Out of scope" — kept here as a reminder:

- Code-signing / notarization (Windows Authenticode, Apple Developer ID).
- GitHub Actions cross-platform CI build matrix.
- `native-tls` → `rustls-tls` migration (drops the OpenSSL dep on Linux CI).
- Auto-update infrastructure.
