# CI pipeline — design (GitHub Actions, cross-platform)

Status: **planning** (no `.github/` exists yet). Implements the Phase 7 roadmap line
"Cross-platform CI builds (GitHub Actions: win/mac/linux)". Spec: `docs/spec/ci-pipeline.md`.
Builds on `docs/spec/phase7-installers.md` (the bundle config already landed). Signing +
auto-update are the **next** layer, explicitly out of scope here (see §9).

---

## 1. Goal & success criteria

Two GitHub Actions workflows:

1. **`test.yml`** — fast PR/push feedback: full Rust lib test suite (`cargo test`) +
   `tsc --noEmit` + frontend production build (`vite build`) + a "bindings.ts is not stale"
   guard. Runs on PRs and pushes to `main`.
2. **`bundle.yml`** — per-OS `tauri build` producing installers (Windows MSI + NSIS, macOS DMG,
   Linux AppImage + tarball), uploaded as workflow artifacts. Runs on **tags `v*`** and
   **manual `workflow_dispatch`** — *not* every push.

Success = a contributor opening a PR gets a green/red signal from `test.yml` in minutes, and a
maintainer can cut a tag (or click "Run workflow") to discover whether the **never-before-built
macOS DMG and Linux AppImage/tarball** actually build, and download the artifacts.

The headline risk this CI exists to retire: **the macOS and Linux bundle configs
(`docs/spec/phase7-installers.md` IP-5/6/7) have NEVER been built on those OSes.** CI is the
first real macOS/Linux build. `bundle.yml` is deliberately `fail-fast: false` so each OS leg
reports independently — a broken Linux AppImage must not hide a working macOS DMG.

---

## 2. The central decision: `scripts/build.sh` / `apex-build.bat` in CI? → **No. CI calls cargo/npm/tauri directly.**

The repo rule is "always build/test through `scripts/build.sh`". That rule earns its keep on the
**dev machine** (a WSL2 host). It does **not** transfer to CI, and forcing it would be wrong.
Tracing what the scripts actually do on each OS:

### What `scripts/build.sh` does
- `is_wsl()` greps `/proc/version` for "microsoft". On a GitHub runner this is **false** on all
  three OSes → it would take the `run_native()` branch.
- `run_native()` is just: `cargo check`/`cargo test`/`npm run tauri build` with `ensure_node`
  (`npm install` if `node_modules` missing) and a `. $HOME/.cargo/env` fallback. **On macOS and
  Linux runners this is exactly the right set of commands** — there is nothing WSL-specific in it.
- The entire `run_wsl()` mirror-to-`C:\Users\drgor\...`-and-call-`.bat` machinery is **dead code
  on CI** — it only fires under WSL, which never happens on a hosted runner.

### Why `apex-build.bat` is NOT CI-portable
`apex-build.bat` hardcodes the **dev machine's** absolute paths:
`C:\Users\drgor\.cargo\bin`, `C:\Program Files\nodejs`, and `cd /d "%~dp0.."` assuming the
mirror lives at `C:\Users\drgor\Documents\GitHub\ApexLauncher`. On a `windows-latest` runner the
user is `runneradmin` and the checkout is under `D:\a\...`. The `.bat` also self-sources
`vcvarsall.bat` by probing `C:\Program Files (x86)\Microsoft Visual Studio\2022\...` — which
happens to exist on the runner, but the rest of the script is machine-specific. Using it on CI
would require rewriting it; at that point it is not "the same entrypoint" anymore.

### Decision
**CI invokes the underlying tools directly** (`npm ci`, `cargo test`, `tsc`, `vite build`,
`tauri build`) on all three OSes. Rationale:

1. **build.sh's real value (the WSL→Windows mirror) is irrelevant on native runners.** What's
   left (`run_native`) is a thin wrapper over the exact commands CI would call anyway.
2. **apex-build.bat hardcodes dev-machine paths** and cannot run unmodified on a runner.
3. **CI needs per-step control** the scripts don't expose: matrix args (`--target` for macOS
   dual-arch), `--bundles` lists per OS, granular caching, nextest retries, the bindings-drift
   `git diff` gate, and apt dep installation. Threading all that through build.sh modes would
   bloat the scripts for one consumer.
4. **Honor the *spirit* of the rule:** CI runs commands that are *equivalent* to what build.sh
   runs locally (`cargo test` = `build.sh test`; `tauri build` = `build.sh build`). The local
   rule stands unchanged; **CI is the single sanctioned exception**, documented in CLAUDE.md.

The Windows `rc.exe` requirement (tauri-winres needs the Windows SDK Resource Compiler — see
phase7-installers.md) is handled on CI by `ilammy/msvc-dev-cmd@v1`, which sources the VS2022
developer environment into the job (the CI analogue of what `apex-build.bat` does locally). The
`windows-latest` runner ships VS2022 + Windows SDK preinstalled, so no install step is needed —
only environment activation. (MSI's VBSCRIPT feature is also present on the runner.)

> Trade-off acknowledged: this means two build paths exist (build.sh locally, raw tooling in CI).
> They are kept equivalent by construction and both documented. The alternative — making build.sh
> CI-aware — couples the dev-machine script to GitHub runner internals, which ages worse.

---

## 3. Workflows & jobs

### 3.1 `test.yml` — PR / push feedback
Triggers: `pull_request` (any branch) + `push` to `main`. `concurrency` group keyed on ref to
cancel superseded runs.

Two jobs:

- **`rust-test`** — matrix over `[ubuntu-22.04, windows-latest, macos-latest]`,
  `fail-fast: false`. Installs Rust stable + `Swatinem/rust-cache`, installs the Linux WebKit
  deps on the ubuntu leg, activates MSVC on the windows leg, runs the **full lib suite**. Uses
  `cargo-nextest` with `--retries 2` to absorb the one known timing flake
  (`cp4_concurrency_bound_not_exceeded`) without masking real failures (see §6).
- **`frontend-and-bindings`** — ubuntu-only (platform-independent). `npm ci`, `tsc --noEmit`,
  `vite build`, then the **bindings-drift gate**: run the Linux-only export test and fail if it
  rewrites the committed file:
  ```bash
  cargo test --manifest-path src-tauri/Cargo.toml export_bindings
  git diff --exit-code src/lib/bindings.ts
  ```
  This is where `bindings_export_tests.rs` (gated `not(target_os = "windows")`) finally runs in
  anger — it regenerates `src/lib/bindings.ts` through the *same* `make_builder()` the app uses
  and the `git diff --exit-code` fails the job if a Rust DTO/command/event changed without a
  `bindings.ts` regen. (The export test also runs as part of `rust-test`'s ubuntu leg; the
  dedicated `git diff` step is what turns "it exported" into "it didn't change".)

> Why all 3 OSes for `rust-test` but PR feedback "fast"? The suite is GUI-free and quick; the
> three legs run in parallel, so wall-clock ≈ one leg + matrix overhead. Catching a
> Windows/macOS-only test regression on the PR is worth it. If this proves slow, the fallback is
> ubuntu-only on PR and 3-OS on push-to-main (noted as a tuning lever, not the initial design).

### 3.2 `bundle.yml` — installers
Triggers: `push` tags `v*` **and** `workflow_dispatch` (manual, with an optional input). Matrix,
`fail-fast: false`:

| OS leg | `--bundles` | Artifact(s) | Notes |
|--------|-------------|-------------|-------|
| `ubuntu-22.04` | `appimage` + tarball wrap | `*.AppImage`, `ApexLauncher_*_amd64.tar.gz` | **22.04 for the glibc floor** (phase7 §glibc) so older distros don't hit `GLIBC_2.xx`. AppImage needs `libfuse2` at build time. |
| `macos-latest` (`--target aarch64-apple-darwin`) | `dmg` | `ApexLauncher_*_aarch64.dmg` | Apple-silicon DMG. |
| `macos-latest` (`--target x86_64-apple-darwin`) | `dmg` | `ApexLauncher_*_x64.dmg` | Intel DMG. Per-arch (not universal) so a broken arch fails in isolation. |
| `windows-latest` | `msi nsis` | `*_x64_en-US.msi`, `*_x64-setup.exe` | MSVC + SDK preinstalled; activate via `msvc-dev-cmd`. |

Each leg: checkout → setup-node (npm cache) → `npm ci` → Rust stable + the macOS extra target
(`rustup target add`) → `Swatinem/rust-cache` (distinct `prefix-key` so it doesn't collide with
`test.yml`'s debug cache) → OS deps → `npm run tauri build -- --bundles <list>` → `glob`-collect
the bundle dir → `actions/upload-artifact@v4`.

The **Linux tarball** (phase7 IP-6: raw `target/release/modloader` + `.desktop` + README wrapped
as `.tar.gz`) is not a `tauri build` bundle target; it's a post-build shell step that tars the
release binary. The spec carries this as part of the Linux checkpoint. (If a `scripts/` wrap step
is added per IP-6, CI calls it; otherwise CI inlines the `tar` invocation.)

> Artifacts only — **no GitHub Release is created** in this slice. Release attachment +
> checksums land with the signing phase (a signed release is the meaningful one). Artifacts give
> us downloadable installers to smoke-test immediately.

---

## 4. Matrix, toolchain, caching

- **Rust toolchain:** `dtolnay/rust-toolchain@stable` (the maintained replacement for the
  **deprecated `actions-rs/*`** actions — `actions-rs` is unmaintained and throws Node-version
  deprecation warnings; do not use it). Channel `stable`, edition 2021 (repo has no
  `rust-toolchain.toml` and no MSRV pin). Optional hardening: add a `rust-toolchain.toml` pinning
  `channel = "stable"` for reproducibility — flagged as an open question, not required for v1.
- **Node:** `actions/setup-node@v4`, `node-version: 20` (LTS; repo declares no `engines`),
  `cache: npm`. Install with **`npm ci`** (lockfile present and committed → reproducible).
- **Rust cache:** `Swatinem/rust-cache@v2` with `workspaces: "src-tauri -> target"`. Keys on
  `Cargo.lock` + rustc version automatically. Give `test.yml` and `bundle.yml` **distinct
  `prefix-key`s** (e.g. `test` vs `bundle`) because debug and release artifacts must not share a
  cache slot. Per-OS keys are automatic (rust-cache includes the runner OS).
- **fail-fast:** `false` on both matrices — we want every OS leg's result, especially the
  first-ever mac/linux bundle builds.

---

## 5. Linux runner dependencies (the footgun, cited)

Tauri 2 on Ubuntu needs **WebKit2GTK 4.1** (Tauri v1 used 4.0; v2 moved to 4.1). The official
Tauri "GitHub" distribute guide installs, on `ubuntu-22.04`:

```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

Plus, for **AppImage** bundling, `libfuse2` (the AppImage runtime is a FUSE mount) and `file`.

Version footguns to get right:
- **`ubuntu-22.04` ships `libwebkit2gtk-4.1-dev`** (good — and it's our glibc-floor target from
  phase7). `ubuntu-20.04` only had 4.0 and is EOL on GitHub runners → do not use it.
- On **`ubuntu-24.04`** the appindicator package is renamed to
  `libayatana-appindicator3-dev` (the `libappindicator3-dev` transitional package was dropped).
  We pin **`ubuntu-22.04`** for the glibc floor, so we use `libappindicator3-dev`. If a future
  leg moves to 24.04, swap to `libayatana-appindicator3-dev`. This rename is the classic
  "works on my 22.04, breaks on 24.04" CI failure.
- **No OpenSSL/`libssl-dev` needed.** `feat/rustls-tls-switch` is merged — reqwest uses
  rustls (`ring` + `webpki-roots`), no native-tls. Confirmed: no `openssl`/`native-tls`/`libssl`
  references remain in `src-tauri/Cargo.toml`. (phase7-installers.md §native-tls is now stale —
  reconciled in §8.) Omitting `libssl-dev` is deliberate, not an oversight.

Sources: Tauri v2 "Distribute → GitHub" pipeline guide; Tauri Linux prerequisites; Tauri
discussion #10026 (AppImage glibc floor → build on oldest target = Ubuntu 22.04 for v2).

---

## 6. Flaky test handling

`cp4_concurrency_bound_not_exceeded` in `download_tests.rs` is timing-sensitive and pre-existing
(documented in signals as a known flake). Options considered:

| Option | Effect | Verdict |
|--------|--------|---------|
| Leave as-is, plain `cargo test` | Occasional red PR on a non-bug | Bad for a gating check |
| `#[ignore]` the test | Stops flaking but **loses the concurrency-bound coverage** | Last resort |
| `cargo-nextest --retries 2` | Re-runs only failed tests; a real failure still fails (won't pass 3×), a flake passes on retry; keeps coverage | **Chosen** |
| `continue-on-error` on the step | Hides *all* failures, not just this one | Unacceptable |

**Chosen: `cargo-nextest` with `--retries 2`** for the `rust-test` job (install via
`taiki-e/install-action@nextest`). Nextest also parallelizes better and gives nicer CI output.
Trade-off for the human: this adds a CI-only dev tool (nextest) and a small risk that retries
mask a *newly* flaky test. If you'd rather not adopt nextest, the fallback is plain `cargo test`
+ `#[ignore]` on that one test with a tracking comment — accepting the lost coverage. **Open
question for approval** (§ spec). Either way, document the decision next to the test.

---

## 7. Secrets

**None required for this slice.** The CF API key resolves env → settings → baked tier; the only
tests that need it (`curseforge_live.rs`, `tls_live.rs`) are `#[ignore]`d and never run under a
plain `cargo test`/nextest run. `build.rs` bakes nothing when `src-tauri/.env` is absent (it is —
gitignored), and the build succeeds with an empty baked tier. Bundling needs no secret either
(the key is a runtime concern). When signing lands, *that* phase introduces secrets
(Authenticode cert, Apple cert + notarization creds).

---

## 8. Reconciliation with `docs/spec/phase7-installers.md`

- phase7 IP-5/6/7 deliberately gated mac/Linux artifacts on "a Linux/macOS host **or CI**".
  **This CI is that gate** — `bundle.yml` is the first execution of those configs. Expect to
  discover real breakage (icon formats, AppImage linuxdeploy quirks, DMG layout). That discovery
  is the point.
- phase7 lists glibc floor = Ubuntu 22.04 / Debian 12 → `bundle.yml` Linux leg pins
  `ubuntu-22.04`. Consistent.
- phase7 §"native-tls dependency" (needs OpenSSL dev headers) is **now stale** — superseded by
  the merged rustls switch. CI does **not** install `libssl-dev`. (Note added to phase7 change
  log when this lands, or left as historical — flagged in spec.)
- phase7 IP-1 set `bundle.targets: "all"`; CI overrides per-leg with `--bundles <list>` so each
  OS produces only its formats (avoids a leg trying to build a foreign target).

---

## 9. Deferred — the next layer (do not build here)

- **Code signing.** Windows Authenticode (cert in a secret, `signtool`/tauri signing config);
  macOS codesign + notarization (Apple Developer ID cert, `xcrun notarytool`, hardened runtime).
  Needs: paid Apple Developer account + a Windows cert; both injected as Actions secrets. Unsigned
  DMG/MSI will warn on launch until then.
- **Auto-update.** Tauri updater plugin + a signed `latest.json` release feed + an update signing
  keypair (`TAURI_SIGNING_PRIVATE_KEY`). Requires real GitHub Releases (this slice only uploads
  artifacts), so it layers on top of a release-creating `bundle.yml` (swap explicit steps for
  `tauri-apps/tauri-action@v0`, which creates the release + uploads + signs in one).

`tauri-action@v0` is the natural upgrade path for `bundle.yml` once we want releases + update
signing; for the artifact-only slice, explicit steps are clearer and mirror `build.sh` 1:1.

---

## 10. Rejected approaches

- **Call `scripts/build.sh` in CI** — rejected: its WSL-mirror core is dead on runners and the
  Windows path routes through the dev-machine-pathed `apex-build.bat` (§2).
- **Use `tauri-apps/tauri-action@v0` for `bundle.yml` now** — rejected for v1: its headline value
  is release creation + update signing, neither of which exists yet; explicit steps give finer
  control over dual-arch macOS, the Linux tarball wrap, and the apt deps, and map 1:1 onto
  `build.sh build`. Adopt it when signing/releases land.
- **`actions-rs/toolchain`** — rejected: deprecated/unmaintained. Use `dtolnay/rust-toolchain`.
- **Bundle on every push** — rejected: slow/expensive and would red-flag `main` on every push
  while the mac/linux first-builds are being stabilized. Tags + dispatch instead.
- **`continue-on-error` for the flake** — rejected: masks all failures (§6).
</content>
</invoke>
