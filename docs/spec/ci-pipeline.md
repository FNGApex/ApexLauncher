# CI pipeline — spec (GitHub Actions)

Status: **planning / awaiting approval.** Design: `docs/design/ci-pipeline.md`. Builds on
`docs/spec/phase7-installers.md`. Signing + auto-update deferred (design §9).

Contract: stand up two GitHub Actions workflows under `.github/workflows/` — `test.yml`
(PR/push gate) and `bundle.yml` (per-OS installers as artifacts). CI calls cargo/npm/tauri
**directly** (not `scripts/build.sh` / `apex-build.bat` — see design §2); local builds keep using
`build.sh`. No secrets required this slice. Each checkpoint is independently landable and
verifiable on a **branch/draft PR** (never needs merge to `main` to prove green).

**Files this spec creates:**
- `.github/workflows/test.yml`
- `.github/workflows/bundle.yml`
- (optional) `rust-toolchain.toml` — pin `channel = "stable"` (CP-1, optional)
- doc touch: `CLAUDE.md` "Build & run" note + `docs/spec/phase7-installers.md` change log
  (record the CI gate + rustls-supersedes-OpenSSL reconciliation) — CP-6.

**Verification vocabulary** (how a checkpoint is proven without merging):
- *draft PR* — push the branch, open a **draft PR** against `main`; `test.yml` (PR-triggered)
  runs on it. Green checks = pass. This is the primary method for CP-2.
- *branch dispatch* — `bundle.yml` has `workflow_dispatch`; run it from the **feature branch** via
  the Actions tab (or `gh workflow run bundle.yml --ref <branch>`). Proves a bundle leg without a
  tag and without merging. Primary method for CP-3/4/5.
- *act (local, optional)* — `nektos/act` can dry-run `test.yml` locally for syntax/flow, but it
  cannot reproduce macOS/Windows runners or GUI bundling; use only as a pre-push lint, not as
  checkpoint proof.

---

## Checkpoints

| CP | Deliverable | Files | Done when (verification) |
|----|-------------|-------|--------------------------|
| **CP-1** | **`test.yml` skeleton + `rust-test` job (ubuntu) + toolchain pin.** Triggers `pull_request` + `push` to `main`; `concurrency` cancel-in-progress. Single `ubuntu-22.04` leg: `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2` (`workspaces: "src-tauri -> target"`, `prefix-key: test`), apt deps (§Linux deps — test variant, NO `libfuse2`/`file`), `taiki-e/install-action@nextest`, `cargo nextest run --manifest-path src-tauri/Cargo.toml --retries 2`. **Add `rust-toolchain.toml` (`channel = "stable"`) — required.** | `.github/workflows/test.yml`, `rust-toolchain.toml` | Draft PR shows `rust-test (ubuntu-22.04)` **green**; the full lib suite runs (~757 tests) and passes; `cp4_concurrency_bound_not_exceeded` survives via retry. |
| **CP-2** | **`test.yml` complete: event-conditional matrix + `frontend-and-bindings` job.** `rust-test` matrix is **ubuntu-only on `pull_request`, full `[ubuntu-22.04, windows-latest, macos-latest]` on `push` to main** (`fail-fast: false`). Mechanism: a small `setup` job that emits the matrix JSON via `github.event_name` (or guard the windows/macos legs with `if: github.event_name != 'pull_request'`) — builder picks the cleaner of the two. Windows leg adds `ilammy/msvc-dev-cmd@v1`. Add `frontend-and-bindings` job (ubuntu, runs on both PR + push): `actions/setup-node@v4` (node 20, `cache: npm`), `npm ci`, `npx tsc --noEmit`, `npm run build` (vite), then bindings-drift gate: `cargo test --manifest-path src-tauri/Cargo.toml export_bindings` + `git diff --exit-code src/lib/bindings.ts`. | `.github/workflows/test.yml` | Draft PR: `rust-test (ubuntu-22.04)` + `frontend-and-bindings` **green** (windows/macos legs ABSENT on the PR — confirm they do NOT run). A push to a temporary main-like branch (or merge) shows the 3-OS matrix. Sanity: a throwaway commit editing a Rust DTO without regenerating `bindings.ts` makes `frontend-and-bindings` **red** on the bindings step (revert after proving). |
| **CP-3** | **`bundle.yml` Linux leg.** New workflow, triggers tags `v*` + `workflow_dispatch`, `fail-fast: false`. `ubuntu-22.04` leg: checkout, node 20 + `npm ci`, rust stable, `rust-cache` (`prefix-key: bundle`), apt deps **+ `libfuse2 file`**, `npm run tauri build -- --bundles appimage`, then tarball wrap (raw `target/release/modloader` + `.desktop` + README → `ApexLauncher_0.1.0_amd64.tar.gz`; call `scripts/` wrap step if IP-6 added one, else inline `tar`), `actions/upload-artifact@v4`. | `.github/workflows/bundle.yml` | Branch dispatch (`--ref <branch>`) of `bundle.yml` produces a downloadable `*.AppImage` **and** `*_amd64.tar.gz` artifact. **First-ever Linux build** — record any config fix needed (icons, linuxdeploy) in change log. |
| **CP-4** | **`bundle.yml` macOS leg (dual-arch).** Two `macos-latest` matrix entries with `args`/`--target` `aarch64-apple-darwin` and `x86_64-apple-darwin`; `rustup target add <target>`; `npm run tauri build -- --target <t> --bundles dmg`; upload each DMG. | `.github/workflows/bundle.yml` | Branch dispatch produces `ApexLauncher_0.1.0_aarch64.dmg` **and** `..._x64.dmg` artifacts. **First-ever macOS build** — note breakage/fixes. (Unsigned → would warn on a real Mac; CI only proves it builds + bundles.) |
| **CP-5** | **`bundle.yml` Windows leg + finalize.** `windows-latest` leg: `ilammy/msvc-dev-cmd@v1`, `npm run tauri build -- --bundles "msi nsis"`, upload `*_x64_en-US.msi` + `*_x64-setup.exe`. Confirm trigger set (tags `v*` + dispatch) and that every leg uploads named artifacts; add `retention-days`. | `.github/workflows/bundle.yml` | Branch dispatch: all 4 legs (linux, mac×2, windows) report independently; Windows artifacts (MSI + NSIS) download and match phase7 IP-3/IP-4 names. Full matrix green = mac/linux first-build risk retired. |
| **CP-6** | **Docs reconcile.** CLAUDE.md "Build & run": add one line "CI is the single sanctioned exception to the build-via-build.sh rule (`.github/workflows/`, calls cargo/npm/tauri directly)". Update `docs/spec/phase7-installers.md` change log: CI now satisfies the IP-5/6/7 "or CI" gate; mark §native-tls note superseded by rustls. Mark this spec implemented. | `CLAUDE.md`, `docs/spec/phase7-installers.md`, this spec's change log | Docs merged; a fresh reader learns CI exists, why it bypasses build.sh, and that phase7's OpenSSL note is dead. |

---

## Linux deps

Two variants. **`test.yml` (CP-1/2)** — no AppImage, so drop `libfuse2`/`file`:
```yaml
- name: Install Linux WebKit/GTK deps
  if: startsWith(matrix.os, 'ubuntu')
  run: |
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```
**`bundle.yml` Linux leg (CP-3)** — adds the AppImage tools:
```yaml
    sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libfuse2 file
```
- `libwebkit2gtk-4.1-dev` (Tauri v2 needs 4.1, not 4.0) — available on `ubuntu-22.04`.
- `libfuse2` + `file` are for **AppImage** bundling — bundle leg only (decision #6).
- On `ubuntu-24.04` (NOT used here) swap `libappindicator3-dev` → `libayatana-appindicator3-dev`.
- **No `libssl-dev`** — rustls is merged; OpenSSL is gone (design §5).

Source: Tauri v2 *Distribute → GitHub* pipeline guide; Tauri Linux prerequisites; tauri-apps
discussion #10026 (AppImage glibc floor → build on oldest target, Ubuntu 22.04 for v2).

## Action versions (pin majors)

| Action | Version | Note |
|--------|---------|------|
| `actions/checkout` | `@v4` | |
| `actions/setup-node` | `@v4` | `node-version: 20`, `cache: npm` |
| `dtolnay/rust-toolchain` | `@stable` | replaces deprecated `actions-rs/*` |
| `Swatinem/rust-cache` | `@v2` | `workspaces: "src-tauri -> target"`; distinct `prefix-key` per workflow |
| `taiki-e/install-action` | `@v2` (`tool: nextest`) | installs `cargo-nextest` |
| `ilammy/msvc-dev-cmd` | `@v1` | Windows: sources VS2022 dev env for `rc.exe` (tauri-winres) |
| `actions/upload-artifact` | `@v4` | bundle artifacts |
| `tauri-apps/tauri-action` | `@v0` | **deferred** — adopt when releases/signing land |

## Build commands per leg (equivalent to `build.sh` modes)

| Leg | Command | `build.sh` analogue |
|-----|---------|---------------------|
| rust-test | `cargo nextest run --manifest-path src-tauri/Cargo.toml --retries 2` | `build.sh test` |
| frontend | `npx tsc --noEmit` + `npm run build` | `build.sh check` (cargo-check side covered by rust-test) |
| bundle linux | `npm run tauri build -- --bundles appimage` + tar wrap | `build.sh bundle appimage` |
| bundle macOS | `npm run tauri build -- --target <arch> --bundles dmg` | `build.sh bundle dmg` |
| bundle windows | `npm run tauri build -- --bundles "msi nsis"` | `build.sh bundle msi nsis` |

---

## Resolved decisions (human, 2026-06-27 — locked before execution)

1. **Flaky test** ✅ `cargo-nextest --retries 2` (keeps concurrency-bound coverage; real
   failures still fail). CP-1.
2. **`rust-test` matrix breadth** ✅ **ubuntu-only on `pull_request`; full 3-OS on push-to-main.**
   (Diverges from the original "3-OS always" assumption — cheaper PR feedback; OS-specific
   breakage caught at merge to main.) CP-1/CP-2 adjusted accordingly.
3. **`rust-toolchain.toml` pin** ✅ ADD it (`channel = "stable"`) — reproducible CI. Now a
   required deliverable in CP-1 (not optional).
4. **macOS arch** ✅ per-arch DMGs (x86_64 + aarch64) — isolates failures, smaller downloads. CP-4.
5. **Bundle trigger** ✅ tags `v*` + `workflow_dispatch` only — NO every-push bundling. CP-3/5.
6. **`libfuse2`/`file` in `test.yml`** ✅ DROP them — `test.yml` does no AppImage bundling
   (PR is ubuntu test-only; bundling lives in `bundle.yml`). Keep them in `bundle.yml`'s Linux
   leg only.

## Change log

- 2026-06-27 — Initial spec drafted (planning only). Two workflows, CP-1→CP-6, branch/draft-PR
  verification. CI calls tooling directly (not build.sh); nextest+retries for the flake; tags +
  dispatch for bundling; no secrets; rustls supersedes phase7's OpenSSL note.
- 2026-06-27 — Decisions locked (human): #1 nextest+retries; #2 **ubuntu-only PR + 3-OS on
  push-to-main** (was "3-OS always" — CP-1/CP-2 reworked to event-conditional matrix); #3
  `rust-toolchain.toml` now required (CP-1); #4 per-arch DMGs; #5 tags+dispatch only; #6 drop
  `libfuse2`/`file` from `test.yml`. Cleared for implementation via `/ax-implement`.
- 2026-06-27 — **ALL CHECKPOINTS IMPLEMENTED + VERIFIED GREEN** on branch `feat/ci-pipeline`.
  CP-1+2 `test.yml` (PR #2 run 28283770823: ubuntu rust-test + frontend/bindings-drift green).
  CP-3 Linux bundle (first-ever) green. CP-4 macOS dual-arch DMG (first-ever, incl. x86_64
  cross-compile) green. CP-5 Windows MSI+NSIS green after fixing a `--bundles "msi nsis"`
  quoting bug (clap variadic needs two tokens; commit `acc6bf7`). Final full bundle matrix
  (run 28307297196) all four legs green. Verification used throwaway `v*` tags (workflow_dispatch
  404s off the default branch) — all deleted. CP-6 docs reconciled (CLAUDE.md CI-exception note;
  phase7-installers `native-tls`→rustls + IP-5/6/7-satisfied-by-CI). Known non-blocker: Node20
  action deprecation warnings (followup 001 — bump @v4→@v5). Signing + auto-update still deferred.
</content>
