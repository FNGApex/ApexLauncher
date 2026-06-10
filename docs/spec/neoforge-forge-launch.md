# NeoForge + Forge launch (Phase 4, slice B)

## Goal

Make a NeoForge or Forge instance (no mods) launch to the Minecraft main menu by:

1. Running the official headless installer once per loader version to produce the patched
   client artifacts and loader `version.json`.
2. Parsing that `version.json` into the existing `LoaderProfile` type and merging it via
   the existing `merge_loader_profile` seam.
3. Assembling and spawning the JVM through the unchanged vanilla launch pipeline.

Install is one-time, shared across all instances on the same loader version. Loader versions
already listed by `core/loaders.rs` become launchable.

## Non-goals

- Legacy Forge < 1.13 (pre-processor installer format).
- Server-side installs.
- Mod loading, browsing, or management (Phase 5).
- Reimplementing the installer processor pipeline in Rust.
- Supporting custom Maven mirrors or offline installs.
- SHA verification of empty-URL (processor-produced) loader libraries (no hash available).
- Any change to the `LaunchMeta` / `Loader` serialized shape (no `ipc.ts` change required).

## Success criteria

- [ ] A NeoForge instance (`kind=neoforge`, MC ≥ 1.20.x, a real loader version) launches
      and reaches the Minecraft main menu (manual verification on a machine with a valid
      MSA account or offline mode).
- [ ] A Forge instance (`kind=forge`, MC ≥ 1.20.x) launches and reaches the main menu
      (manual verification on a machine with a valid MSA account or offline mode).
- [ ] The installer runs at most once per loader version; a second launch of the same
      instance skips the install step (idempotency: `versions/<id>/<id>.json` present →
      skip).
- [ ] Install progress (installer stdout/stderr) is forwarded to the `install://log` event
      channel so the user sees output. `install://log` is distinct from
      `download://progress`; installer stdout is log-shaped, not item-progress-shaped.
- [ ] `cargo check` and `cargo test` pass from `src-tauri/` with no live HTTP, no live
      JVM, and no OS keyring in any unit test. The mock spawn in CP1 asserts the exact java
      argv (installer jar path, `--installClient`, target dir) and working dir; tests cover
      both the success path (exit 0) and failure path (non-zero exit → `Err`).
- [ ] Empty-URL libraries (processor-produced, no download URL) are added to the classpath
      only — not as `DownloadItem`s — so the download step does not attempt to fetch them.
- [ ] `${library_directory}`, `${classpath_separator}`, and `${version_name}` are covered
      by the existing `build_argv` substitution table (`launch.rs:219-244`); CP2 adds a
      regression test that exercises those placeholders against a forge-profile argv to
      confirm no regression, not new substitution logic.
- [ ] A vanilla instance and a Fabric/Quilt instance still launch unchanged after this
      slice (regression check, manual verification).

## Approaches

| # | Approach | Pros | Cons |
|---|----------|------|------|
| A | **Headless official installer run** | Supported path; format-churn-proof (installer matches its own profile); reuses `ensure_java` + existing merge seam | Installer owns its downloads (coarse progress); needs dummy `launcher_profiles.json`; ~1 min one-time step |
| B | Reimplement processor pipeline in Rust | Full control, fine-grained progress | High cost; breaks on format churn (Prism's ForgeWrapper history proves it); large test surface |
| C | ForgeWrapper at launch time | Battle-tested in Prism | Third-party jar dep; opaque failures; requires ongoing wrapper updates |
| D | Prism meta server profiles | Pre-digested JSON | External service dep; patched client still requires processor run |

## Recommendation

**Approach A — headless installer.** The installer jar is the only artifact guaranteed to
match its own profile format across the version range users will pick. Evidence:

- `neoforged/legacyinstaller` `ClientInstall.java`: `--installClient <dir>` downloads the
  vanilla jar, writes `versions/<id>/<id>.json` + `libraries/`, runs all processors.
- `ClientInstall.java` guards on `launcher_profiles.json` — seed `{"profiles":{}}` before
  running; use the app-data dir so `libraries/` is shared with vanilla launches.
- The produced `version.json` (`inheritsFrom` + `mainClass` + extra libs + extra args) is
  structurally a superset of the Fabric/Quilt profile JSON — parse it into the existing
  `LoaderProfile` type (`core/loader_profile.rs`) and the rest of the pipeline is shared.
- `core/java.rs ensure_java` and `core/resolver.rs merge_loader_profile` are already in
  place; no new abstractions are needed.
- Forge/NeoForge `version.json` JVM args use three placeholders the vanilla set lacks:
  `${library_directory}`, `${classpath_separator}`, `${version_name}`. These are already
  present in the `build_argv` substitution table (`launch.rs:219-244`); CP2 adds a
  regression test to confirm they work against a forge-profile argv.
- Some profile libraries carry no download URL (produced locally by processors). `resolver`
  must skip `DownloadItem` generation for these, adding only a classpath entry.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | **Headless installer runner module.** New `core/forge_installer.rs`: `run_installer(app, loader_kind, loader_version, mc_version, java_bin, data_dir) -> Result<PathBuf>` — downloads installer jar from the correct Maven repo (NeoForge: `maven.neoforged.net/releases/...`; Forge: `maven.minecraftforge.net/...`), seeds `launcher_profiles.json`, spawns JVM with `--installClient <data_dir>`, streams stdout/stderr to the `install://log` event channel, returns path to the produced `versions/<id>/<id>.json`. Idempotency guard: return early if `version.json` already exists. Spawn is injectable for unit-testability. Register in `core/mod.rs`. | `src-tauri/src/core/forge_installer.rs`, `core/mod.rs` | atomic-builder | ~2 | Unit tests: idempotency guard short-circuits on existing file; Maven URL construction for NeoForge and Forge (both repo patterns); mock spawn asserts exact java argv (installer jar path, `--installClient`, target dir) and working dir, and exercises both exit-0 (success) and non-zero-exit (error) paths; event emission verified via mock sink. No live JVM. |
| 2 | **`version.json` parse → `LoaderProfile` extension + resolver merge extensions.** Extend `loader_profile.rs` `LoaderProfile` to accept the forge `version.json` shape: change `LoaderLibrary.url` from `String` to `Option<String>` (`loader_profile.rs:30`) — Forge `version.json` library entries can omit `url` entirely; the current `String` field fails deserialization on absence (the existing empty-string guard at `resolver.rs:748` does not cover absence). Add `inherits_from: Option<String>` field — load-only; it identifies the vanilla base version already resolved separately; no validation logic in this slice. Add `load_forge_profile(path: &Path) -> Result<LoaderProfile>` reading from disk rather than HTTP. Extend `resolver.rs merge_loader_profile`: skip `DownloadItem` for libraries where `url` is `None` or empty (classpath-only). The three placeholders `${library_directory}`, `${classpath_separator}`, `${version_name}` are already in the `build_argv` substitution table (`launch.rs:219-244`); add a regression test against a forge-profile argv to confirm they substitute correctly. | `src-tauri/src/core/loader_profile.rs`, `src-tauri/src/core/resolver.rs`, `src-tauri/src/core/fixtures/` | atomic-builder | ~3 | Unit tests: fixture `neoforge_profile.json` (real extracted `version.json`) parses into `LoaderProfile` including libraries with absent `url`; `merge_loader_profile` with `url=None` lib → no `DownloadItem`, classpath entry present; regression test asserts `${library_directory}`, `${classpath_separator}`, `${version_name}` substitution against a forge-profile argv; existing Fabric/Quilt and vanilla tests unchanged. |
| 3 | **`lib.rs` wiring: forge/neoforge launch path + install-progress events.** In `launch_instance`: branch on `inst.loader.kind` — for `"forge"` / `"neoforge"` with a `version`, call `ensure_java`, then `run_installer` (idempotent), then `load_forge_profile`, then `merge_loader_profile`, then proceed with the download + spawn path unchanged. Vanilla and Fabric/Quilt branches unchanged. Wire install-progress stdout/stderr to the `install://log` event (distinct from `download://progress`; installer stdout is log-shaped, not item-progress-shaped). | `src-tauri/src/lib.rs` | atomic-surgeon | 1 | `cargo check` passes; manual e2e: NeoForge instance reaches main menu; Forge instance reaches main menu; vanilla and Fabric/Quilt instances still launch (regression). |
| 4 | **Frontend `ipc.ts` + UI surfacing.** Expose the `install://log` event type in `src/lib/ipc.ts`. Surface install progress in `InstanceDetail.tsx` — show the log console during the install phase, distinguishing install-phase output from game-phase output if the existing log component permits it without modification. If the existing log console is already adequate (installer stdout appears there), this checkpoint may be a no-op beyond the `ipc.ts` type addition. | `src/lib/ipc.ts`, `src/routes/InstanceDetail.tsx` | atomic-builder | ~2 | Frontend build (`npm run build`) passes; install-phase log lines visible in the UI during a live NeoForge install (manual verification on a machine with a valid MSA account or offline mode). |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Installer format churn (processor count/shape changes across NeoForge/Forge versions) | med | Approach A is inherently format-proof — installer handles its own format; no Rust code models processors |
| Installer runtime failure (network flake, Java missing, disk full) | med | `run_installer` propagates exit code as `Err`; `ensure_java` is called first; user sees stdout via event channel; idempotency guard allows retry |
| `launcher_profiles.json` guard removed in a future installer release | low | Seeded unconditionally; a future removal makes it a harmless extra file |
| Disk layout collision: installer writes `versions/<id>/` in app-data root alongside launcher-managed layout | low | Accepted: `.minecraft`-style layout in app-data root is what the installer supports; `versions/<id>/` namespace collision with vanilla is impossible (loader id is distinct from vanilla id) |
| `${library_directory}` / `${classpath_separator}` / `${version_name}` placeholder gaps — forge profile uses additional undiscovered placeholders | med | Unit-test all three known ones; log unknown placeholders as warnings at runtime (pass through unreplaced) so they surface immediately on manual launch |
| Empty-URL library classpath path resolution fails (no Maven coord prefix) | low | Lib `name` field is always a Maven coordinate; `maven_coord_to_path` already handles this; covered by CP2 unit test |

## Open questions

Unresolved items deferred per the design doc:

- **Re-run policy:** if a subsequent launch finds `version.json` present but a library file
  missing, re-run installer vs fail loud? Leaning: re-run once, then fail loud. Decision
  deferred; see `docs/design/neoforge-forge-launch.md` Open questions.

## Change log

<!-- new entries go here, newest first -->

## Implementation log

### shipped (manual e2e pending) — 2026-06-10

Built across 7 iterations of /subagent-implementation. Commits (chronological):

- `005ed3f` — CP-1 headless installer runner module (forge_installer.rs, 11 tests)
- `c984f88` — CP-2 forge version.json → LoaderProfile + resolver merge extensions
- `596e0af` — CP-1 io hardening (concurrent stream drain, .part guard, chunked download) + CP-2 argv regression tests (missed staging in c984f88)
- `ca3afc3` — CP-3 lib.rs forge/neoforge launch wiring + install://log sink
- `c772d91` — CP-4 frontend install://log surfacing in InstanceDetail
- `c09cb4e` — polish: JoinError propagation, single ensure_java, cfg(test) gate

**Out-of-scope work performed during this build:**
- none

**Unforeseens — surprises that emerged during implementation:**
- Forge `version.json` library URLs are FULL artifact URLs, while fabric/quilt profiles carry base repo URLs; the shared `LoaderLibrary.url` field would have produced double-path 404s. Resolved with a `.jar`-suffix routing contract in `merge_loader_profile` (caught by iter-2 reviewer before any live download ran).
- CP1's select!-based stream drain was a real deadlock risk for noisy installers; replaced with spawned reader tasks (iter 4).

**Deferred items still open:**
- `neoforge-forge-launch-f-5` — type-safe artifact-vs-base URL contract (.claude/project/followups/)
- `neoforge-forge-launch-f-11` — install://log per-instance filtering (.claude/project/followups/)
- Manual e2e (success criteria 1, 2, 8): NeoForge launch, Forge launch, vanilla/fabric regression — pending user verification; MS-auth online launch additionally blocked on Mojang app-review approval (see docs/design/auth-client-id-blocker.md), offline mode unaffected.
- Dropped at triage: F-4 (deliberate maven_coord_to_path reuse), F-6 (placeholders covered by synthetic launch.rs tests), F-12 (wrapper kept for ipc.ts pattern parity).
