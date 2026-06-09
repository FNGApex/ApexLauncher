# Fabric + Quilt launch (Phase 4, slice A)

## Goal

Make a created Fabric or Quilt instance launch to the Minecraft main menu by overlaying the
loader's profile (mainClass, libraries, extra arguments) onto the resolved vanilla manifest.
Vanilla launch behavior is unchanged.

## Non-goals

- NeoForge + Forge launch (installer/maven runner — separate slice).
- Mod installation, browsing, or management (Phase 5).
- SHA verification of loader libraries (profile ships no hash; deferred, follow-up filed).
- Pre-1.7 legacy-assets launch (already deferred).
- Any frontend / `ipc.ts` change — create + launch UI already exist and are loader-agnostic;
  the serialized shape of `LaunchMeta` / `Loader` does not change.
- De-duplicating loader libraries against vanilla libraries.

## Success criteria

- [ ] A Fabric instance (created via the modal: `kind=fabric`, a real loader build, a MC ≥1.14)
      launches and reaches the main menu, end to end on the developer's machine.
- [ ] A Quilt instance (`kind=quilt`, MC ≥1.14.4) launches and reaches the main menu.
- [ ] A vanilla instance still launches unchanged (regression check — same argv/classpath as before).
- [ ] The loader profile JSON is fetched from the correct per-loader endpoint and disk-cached via
      `meta::cached_text`.
- [ ] Loader libraries are added to the `DownloadPlan` with URL + dest derived from the Maven
      coordinate and the library's `url` base; they download successfully (no `expected_hash`).
- [ ] The merged `LaunchMeta` has `main_class` = the loader's `mainClass`, the loader libraries on
      the classpath, the vanilla **client jar still last** on the classpath, and the loader's
      `arguments.jvm` / `arguments.game` appended after the vanilla args.
- [ ] Maven coordinate → path conversion is unit-tested for the 3-segment case and the optional
      4-segment (classifier) case.
- [ ] `cargo check` and `cargo test` pass from `src-tauri/`.

## Approaches

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | **Inherit + merge** | Resolve vanilla; fetch loader profile; override mainClass; append libs to plan+classpath; append args | med | loader libs unverified (no sha1); coord→path correctness |
| B | Run loader installer jar | Download + run fabric/quilt installer before launch | high | needs JVM pre-launch; same machinery as Forge — wrong sequencing |
| C | Hardcode loader libraries | Embed known lib set per loader version | low | brittle, breaks every loader release |

## Recommendation

**Approach A.** `resolver::assemble` (`resolver.rs:571`) already emits `(DownloadPlan, LaunchMeta)`
with `main_class`, classpath (client jar last), and `jvm_args`/`game_args` as plain appendable
`Vec<String>`. `DownloadItem` already allows `expected_hash: None` / `size: None`, so hashless
loader libs are representable. `meta::cached_text` (`meta.rs`) is the existing fetch+cache primitive
(already used by `loaders.rs`). Appending to existing `Vec` fields and overriding `main_class`
keeps the serialized shape identical, so no `ipc.ts` change. B is the deferred Forge path; C is
rejected.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | **Fetch + parse loader profile.** New module `core/loader_profile.rs`: typed `LoaderProfile` (`main_class`, `libraries: Vec<{name, url}>`, optional `arguments`); `fetch_profile(app, kind, mc, loader_version)` selecting the Fabric vs Quilt endpoint and caching via `meta::cached_text`; a `maven_coord_to_path(coord) -> String` helper. Register the module in `core/mod.rs`. Add a fabric profile fixture under `core/fixtures/`. | `src-tauri/src/core/loader_profile.rs`, `core/mod.rs`, `core/fixtures/fabric_profile.json` | atomic-builder | ~3 | Unit tests: fixture parses (mainClass, libs, args read); endpoint selection per kind; coord→path for 3-seg and 4-seg (classifier) cases |
| 2 | **Merge profile into resolve result.** In `resolver.rs`: pure `merge_loader_profile(plan: &mut DownloadPlan, launch: &mut LaunchMeta, profile: &LoaderProfile, data_dir)` — for each loader lib, derive maven path, push a `DownloadItem` (dest under `libraries/`, `expected_hash: None`) and a classpath entry inserted **before** the vanilla client jar; override `launch.main_class`; append the profile's `arguments.jvm`/`.game` (reuse the existing arg-flattening + OS-rule logic). | `src-tauri/src/core/resolver.rs` | atomic-builder | 1 | Unit tests: after merge, `main_class` == loader's; loader libs present on classpath with client jar still last; plan contains loader items with `expected_hash` None; jvm/game args appended after vanilla |
| 3 | **Wire into launch path.** In `lib.rs` `launch_instance`: after `assemble`, branch on `inst.loader.kind` — for `"fabric"`/`"quilt"` with a `version`, fetch the profile and call `merge_loader_profile` before the download step. Vanilla/unknown kinds fall through unchanged. (Apply the same branch in `resolve_vanilla` only if trivial; otherwise leave it vanilla-only and note in the change log.) | `src-tauri/src/lib.rs` | atomic-surgeon | 1 | `cargo check` passes; manual e2e: a Fabric instance and a Quilt instance each reach the main menu; a vanilla instance still launches |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Loader libs download unverified (no sha1 in profile) | high (by design) | Accept for this slice; HTTPS-only integrity; file a follow-up to fetch `.sha1` siblings later |
| Maven coord → path wrong for classifier/edge coords | med | Unit-test 3-seg + 4-seg explicitly; fixture from a real Fabric profile |
| Classpath ordering breaks loader (client jar position) | low | Contract pins client jar last, loader libs before it; verified in CP2 test + manual launch |
| Quilt profile shape diverges from Fabric | low | Endpoints share the Mojang launcher-profile format; CP1 fixture is Fabric, but parser is shape-driven; manual Quilt launch in CP3 is the backstop |
| CP3 not unit-testable (lib.rs needs AppHandle) | high | Accepted — CP3's contract is the manual e2e launch, matching the roadmap done-criterion; merge logic is fully unit-tested in CP2 |

## Change log

### 2026-06-09 — CP3 wired into launch_instance

**What changed:** `launch_instance` in `src-tauri/src/lib.rs` now fetches and merges the
loader profile before the download step for Fabric and Quilt instances that have a pinned
`loader.version`. Vanilla instances, unknown loader kinds, and loader instances with
`version == None` fall through the branch unchanged and launch exactly as before.
The `loader_profile` module import was added to `lib.rs` following the existing `use core::`
import style.

**Why:** completes the slice — loader instances now launch with the loader's `mainClass`,
loader libraries on the classpath (ahead of the vanilla client jar), and the loader's
extra JVM/game arguments, all wired from the fully unit-tested merge logic in CP2.

**Note:** `resolve_vanilla` (the standalone Tauri preview command) is intentionally left
vanilla-only — it exists for the frontend resolver preview flow and does not take a loader
version parameter. This is an explicit scope boundary, not an oversight.
