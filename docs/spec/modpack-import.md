# Modpack import (Phase 6 — Modrinth `.mrpack` slice A · CurseForge `.zip` slice B · Browse install slice C)

> Slices A (`.mrpack`) and B (CurseForge `.zip`) are **shipped** (`505670b`..`241294f`); their
> sections below are the shipped contract. **Slice C (Browse → one-click install) is the active
> work** — see `## Slice C` near the end.


## Goal

Import a local Modrinth `.mrpack` file into a new instance: parse `modrinth.index.json`,
create the instance with the pack's MC version + loader, download every client-supported
file (hash-verified, host-allowlisted) to its declared path, apply `overrides/` +
`client-overrides/` verbatim, and record mod files as `ModEntry`s. No API key (mrpack carries
direct URLs).

## Non-goals

- Pack update / re-resolve against a newer index — slice D.
- Provider-id-based update of mrpack-imported mods (mrpack carries no project/version ids).
- `server-overrides/` (we are a client launcher).

## Success criteria

- [ ] `parse_modrinth_index` parses a valid `modrinth.index.json` into `MrpackManifest`
      (name, version id, mc version, loader, files with path/hashes/env/downloads/size).
- [ ] Loader keys map correctly: `fabric-loader`→`fabric`, `quilt-loader`→`quilt`,
      `forge`→`forge`, `neoforge`→`neoforge`, none→`vanilla`; loader version = dep value.
- [ ] A file with `env.client == "unsupported"` is skipped; absent `env` ⇒ installed.
- [ ] Hash pick prefers `sha512` then `sha1` → `ExpectedHash`; a file with neither hash
      rejects the pack (malformed) rather than downloading unverified.
- [ ] A `downloads` URL whose host is not on the allowlist (`cdn.modrinth.com`, `github.com`,
      `raw.githubusercontent.com`, `gitlab.com`) aborts the import with a clear error.
- [ ] Any `files[].path` or override entry containing `..`, an absolute path, or a
      drive-letter prefix is rejected (zip-slip / path-escape guard) before any write.
- [ ] `build_pack_plan` produces `DownloadItem`s with `dest` = `<instance>/mc/<path>`, correct
      `expected_hash` + `size`, and `ModEntry`s for files under `mods/` only (provider
      `"modrinth"`, `side` from env, empty project/version ids).
- [ ] `extract_overrides` copies `overrides/` then `client-overrides/` into `mc/` zip-slip-safe
      (override wins on collision with a downloaded path; server-overrides ignored).
- [ ] `import_mrpack` Tauri command wires it end to end: open zip → parse → create instance →
      `execute_plan` → extract overrides → write `ModEntry`s → return `MrpackImportResult`.
- [ ] All parse/plan/extract logic is unit-tested against fixture JSON + fixture `.mrpack`
      zips — no live network in tests.
- [ ] `cargo test` green (via the Windows toolchain — see build note); `npm run build` green.

## Build & test note (read before running anything)

This crate builds on the **Windows** cargo toolchain over the WSL UNC path, NOT WSL-native
(WSL lacks the GTK/WebKit libs Tauri's Linux target needs). Do not run `cargo` in WSL — it
fails on `webkit2gtk-sys`. Run tests with:

```bash
cd /mnt/c && cmd.exe /c "C:\Users\drgor\apex-build.bat" <cargo-test-args>
# e.g.  ... apex-build.bat --lib modpack -- --nocapture
```

`apex-build.bat` sets `CARGO_INCREMENTAL=0` and forwards `%*` to
`cargo test --manifest-path \\wsl.localhost\…\src-tauri\Cargo.toml`.

## Approaches

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | Pure planner (`core/modpack.rs`) + thin `import_mrpack` executor | parse+plan are I/O-free, fixture-tested; command does zip/create/download/extract | med | low |
| B | Monolithic `import_mrpack` doing everything inline | one command opens zip, parses, downloads | low | high — untestable without live HTTP + real fs |
| C | Parse zip in frontend, hand URLs to backend | TS parses index, backend fetches list | med | high — host allowlist lost, logic duplicated |

## Recommendation

**Approach A** — mirrors the proven `mod_install.rs` pure-planner/thin-executor split, so the
security-critical logic (host allowlist, path safety, env filter, hash pick) is unit-tested
against fixtures with no network. The executor reuses `instances::create` and
`download::execute_plan` unchanged. Full rationale + Mermaid flow in
`docs/design/modpack-import.md`.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Manifest model + parser: `MrpackManifest`, `MrpackFile`, `PackLoader` types; `parse_modrinth_index(&str) -> Result<MrpackManifest, _>`; loader+mc mapping; env normalization | `src-tauri/src/core/modpack.rs` (new), `core/mod.rs` | atomic-builder | ~2 | unit tests w/ JSON fixtures: full parse; each loader key→kind; missing loader→vanilla; env unsupported flagged; malformed/missing index errors |
| 2 | Plan builder (pure): `build_pack_plan(&MrpackManifest, instance_mc_dir) -> Result<PackPlan,_>` where `PackPlan { items: Vec<DownloadItem>, mods: Vec<ModEntry>, skipped: Vec<String> }`; host allowlist; path-safety guard; hash pick; env filter; `mods/`→`ModEntry` | `src-tauri/src/core/modpack.rs` | atomic-builder | ~1 | unit tests: dest path under mc dir; sha512>sha1 pick; no-hash→err; disallowed host→err; `..`/absolute path→err; env-unsupported→skipped; only `mods/` files get `ModEntry` |
| 3 | Overrides extraction (zip-slip safe): `extract_overrides(&mut ZipArchive, mc_dir) -> Result<u32,_>` applying `overrides/` then `client-overrides/`, ignoring `server-overrides/` | `src-tauri/src/core/modpack.rs` | atomic-builder | ~1 | unit tests w/ fixture zip: files land under mc dir; `..` entry rejected; client-overrides applied; server-overrides ignored; collision → override wins |
| 4 | `import_mrpack` command + result type: open zip → read index → parse → `instances::create` → `build_pack_plan` → `execute_plan` → `extract_overrides` → write `ModEntry`s; `MrpackImportResult { slug, name, installed, failed, skipped }` | `src-tauri/src/lib.rs`, `src-tauri/src/core/modpack.rs` | atomic-builder | ~2 | test (no live net): malformed zip → error; result counts; happy path wires create+plan (download mocked/asserted by plan shape) |
| 5 | Frontend: `importMrpack` ipc wrapper + import entry point (file picker via Tauri dialog plugin, calls command, shows result, navigates to new instance) | `src/lib/ipc.ts`, `src/components/NewInstanceModal.tsx` | atomic-builder | ~3 | `npm run build` green; ipc types mirror Rust (camelCase); result surfaced (installed/failed/skipped) |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Zip-slip via crafted `path`/override entry escapes instance dir | med | Strict relative-path validation (reject `..`, absolute, drive prefix); CP2+CP3 success criteria; resolve under mc dir and verify prefix |
| Untrusted `downloads` URL → arbitrary jar on classpath | med | Host allowlist enforced in CP2; disallowed host aborts import |
| Download engine hits real network in tests | med | Planner + extractor are pure/fixture-tested; CP4 asserts plan shape + wiring, not live bytes |
| Tauri file-dialog plugin not yet a dependency | med | CP5 adds `tauri-plugin-dialog` (capabilities + Cargo + npm); if blocked, accept a path string arg as fallback |
| `instances::create` requires a valid loader build version that the pack pins but metadata can't resolve | low | Store the pack's pinned loader version directly in `Loader.version`; launch-time resolver already handles explicit versions |
| Forge/NeoForge loader version format in mrpack differs from launch expectations | low | Slice A targets Fabric/Quilt packs first in manual verify; Forge pack e2e tracked as follow-up |

## Slice B — CurseForge `.zip` import (active)

### Goal

Import a local CurseForge modpack `.zip` into a new instance: parse `manifest.json`, create the
instance with the pack's MC version + loader, resolve each `(projectID, fileID)` to a download URL
via the CF API, download the auto-distributable files (hash-verified) into `mc/mods/`, surface
distribution-disabled files as a manual-download list, apply `overrides/` verbatim, and record
each installed file as a `ModEntry` (provider `"curseforge"`, ids = projectID/fileID).

### Non-goals (slice B)

- Batch file resolution (`POST /v1/mods/files`) — single-file GET per entry for slice B;
  batch is a slice-D optimization.
- Auto-downloading distribution-disabled files — surfaced as manual only.
- `client-overrides`/`server-overrides` split — CF packs use one `overrides/` dir.
- Browse / one-click CF pack install — slice C.

### Success criteria (slice B)

- [ ] `parse_cf_manifest` parses `manifest.json` into a `CfManifest` (name, version, mc version,
      loader kind+version, `files[]` of `{ project_id, file_id, required }`).
- [ ] Loader id split: primary `modLoaders[].id` `forge-47.2.0`→(`forge`,`47.2.0`),
      `neoforge-…`→neoforge, `fabric-…`→fabric, `quilt-…`→quilt; no loader entry → `vanilla`.
      `minecraft.version` is the MC version. Malformed/missing manifest fields → error (not panic).
- [ ] CF provider gains a single-file resolver (`get_file(project_id, file_id)`) that returns a
      normalized file (url `Option`, filename, hash, size); `downloadUrl: null` → `url: None`.
      Mock-HTTP tested via the existing `ProviderHttpClient` seam — no live network.
- [ ] `build_cf_pack_plan(manifest, resolved_files, mc_dir)` (pure) produces `DownloadItem`s
      (`dest` = `<mc>/mods/<fileName>`, hash + size) for files with a URL, `ModEntry`s for each
      installed file (provider `"curseforge"`, project_id=projectID, version_id=fileID), and a
      `manual` list for files whose resolved `url` is `None` or whose hash is unusable.
- [ ] A manual file does not abort the import; counts surface in the result.
- [ ] Any override entry or computed `mods/<fileName>` path containing `..`, absolute, or a
      drive prefix is rejected (zip-slip guard) before any write — reuse slice-A path validation.
- [ ] `extract_overrides` is reused unchanged for the CF `overrides/` dir.
- [ ] `import_curseforge_zip` Tauri command wires end to end: open zip → parse manifest →
      `instances::create` → resolve files (CF API) → `build_cf_pack_plan` → `execute_plan` →
      extract overrides → write `ModEntry`s → return `CfImportResult { slug, name, installed,
      failed, manual }`. Tested with a mock provider — no live network.
- [ ] All parse/plan logic unit-tested against fixture JSON + a fixture CF `.zip`; resolution
      tested via the mock provider seam. `cargo test` green (Windows toolchain); `npm run build` green.

### Approaches (slice B) — file resolution

Copied from `docs/design/modpack-import.md` (slice-B file-resolution decision):

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | Single-file GET per entry (`get_file`) on the existing GET seam | reuses `ProviderHttpClient::get`; N requests; mirrors `get_versions` | med | low — slow for big packs, but import is one-shot |
| B | Batch `POST /v1/mods/files` | one request resolves all | low latency | med — needs POST on a GET-only seam |
| C | Reuse `get_versions(projectID)` + filter to fileID | no new endpoint | low code | high — pulls whole version histories |

### Recommendation (slice B)

**Approach A.** One focused `get_file` method on the existing GET seam, mock-testable exactly
like `get_versions`, no seam widening mid-slice. Import latency is non-interactive; the N-request
cost is acceptable. Batch (B) is a slice-D optimization once a second caller justifies the POST
seam. Full rationale in the design doc.

### Checkpoints (slice B)

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| B1 | CF manifest model + parser: `CfManifest`, `CfManifestFile { project_id, file_id, required }`; `parse_cf_manifest(&str) -> Result<CfManifest, ModpackError>`; loader-id split + mc mapping | `src-tauri/src/core/modpack.rs`, fixture `manifest_*.json` | atomic-builder | ~2 | unit tests w/ JSON fixtures: full parse; each loader prefix→kind+version; no loader→vanilla; malformed/missing fields→error |
| B2 | CF single-file resolver: `get_file(project_id, file_id)` on `CurseForgeProvider` returning a normalized file (url Option, filename, hash, size) via the GET seam | `src-tauri/src/core/curseforge.rs`, `core/providers.rs` (if a shared type is needed) | atomic-builder | ~2 | mock-HTTP unit tests: happy path maps fields; `downloadUrl: null`→`url: None`; api-key header carried; key-absent path |
| B3 | CF pack planner (pure): `build_cf_pack_plan(&CfManifest, &[ResolvedFile], mc_dir) -> Result<CfPackPlan { items, mods, manual, skipped }, _>`; url None or no-hash → manual; `mods/<fileName>` dest; path-safety reuse; `ModEntry` with CF ids | `src-tauri/src/core/modpack.rs` | atomic-builder | ~1 | unit tests: dest under mc/mods; ModEntry ids = projectID/fileID; url None→manual; no-hash→manual; unsafe filename→err |
| B4 | `import_curseforge_zip` command + `CfImportResult`: open zip → read `manifest.json` → parse → `instances::create` → resolve files (CF API) → `build_cf_pack_plan` → `execute_plan` → reuse `extract_overrides` → write `ModEntry`s | `src-tauri/src/lib.rs`, `src-tauri/src/core/modpack.rs` | atomic-builder | ~2 | test (mock provider, no live net): malformed zip→error; result counts (installed/failed/manual); wiring routes through pure plan |
| B5 | Frontend: `importCurseforgeZip` ipc wrapper + `CfImportResult` type; extend New Instance modal Import tab (file picker accepts `.zip`; route by archive kind); surface manual list (links) + navigate | `src/lib/ipc.ts`, `src/components/NewInstanceModal.tsx` | atomic-builder | ~2 | `npm run build` green; ipc types mirror Rust (camelCase); manual files surfaced |

### Risks (slice B)

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Large pack → ~100 sequential CF requests slow / rate-limited | med | Accept for slice B (one-shot import); bounded concurrency or batch endpoint is a slice-D follow-up; surface progress in UI |
| CF file record lacks a usable hash → unverified jar | low | Treat hashless resolved file as manual (link page) rather than download blind |
| `manifest.json` `overrides` key names a non-default dir | low | Read the `overrides` key; default `"overrides"` when absent |
| Distribution-disabled file's manual page URL needs mod slug not in the file record | med | Surface projectID/fileID + filename with a `curseforge.com/projects/<projectID>` link; richer slug-based link is a follow-up |
| Partial instance left on disk if resolution/download fails after `instances::create` | med | Same gap as slice A (follow-up `modpack-import-partial-cleanup`); not re-litigated in slice B |

## Slice C — Browse → one-click install (active)

### Goal (slice C)

From the Browse modpack feed, install a pack in one click instead of opening its provider page.
Given a `ProjectSummary` (provider + project id + `projectType: Modpack`), resolve the project's
latest pack file, download the archive, and run it through the *same* import path slices A/B
prove (`import_*_from_bytes`). Modrinth → `.mrpack` path; CurseForge → `.zip` path. A pack file
that is not distributable (CF `url: None`) surfaces a manual-download outcome (open `page_url`),
never a silent failure.

### Non-goals (slice C)

- Choosing a non-latest version (version dropdown) — folds into slice D.
- Live per-file download progress — executors run `NoOpSink`; card shows pending + result toast.
  Live progress is a follow-up.
- Pack update / re-resolve of an already-installed pack — slice D.
- Any change to the local-file import (NewInstanceModal Import tab) behavior.

### Success criteria (slice C)

- [ ] The mrpack and CF import executors are refactored so their byte-processing body is a
      shared inner fn (`import_mrpack_from_bytes`, `import_cf_zip_from_bytes`); the existing
      path-based commands call it. The refactor's correctness is verified by **the existing
      slice A/B test suite passing unmodified** (no observable behavior change).
- [ ] A backend resolver picks a modpack project's **latest** version + its **primary** file
      (`primary == true`, else first file) via `get_versions`. "Latest" = the **first version
      returned** — both providers return newest-first and the normalized `ProjectVersion` carries
      no date field to sort on. Adding a date field for robust sorting is a deferred follow-up if
      live testing shows the order is unreliable.
- [ ] CurseForge pack whose primary file has `url: None` returns a manual outcome carrying
      `page_url` — no instance is created, no blind download.
- [ ] The pack archive is staged under `cache/installers/` before parsing (reuses
      `cache_installers_dir`).
- [ ] The command returns a tagged result distinguishing mrpack / CF (carrying `manual[]`) /
      not-distributable, so the frontend renders the correct toast without re-deriving provider.
- [ ] The pure resolve helper (latest-version + primary-file selection, both providers, CF
      `url: None`→manual) is unit-tested headlessly via the mock `ProviderHttpClient` seam (no
      live HTTP). The `install_modpack` command wiring is verified by `lib_tests.rs`-style
      shape/plan-shape assertions, matching the established command-test pattern (commands can't
      run unit-level without an `AppHandle`).
- [ ] `ModpackCard` in `Browse.tsx` has an Install action (primary) beside the existing
      open-page action (secondary); click runs the install mutation and surfaces a completion
      toast reusing the existing import-result toast pattern.
- [ ] `npm run build` green; `src/lib/ipc.ts` mirrors the new Rust command + result union
      (camelCase).
- [ ] Full Rust lib test suite green on the Windows cargo toolchain.

### Approaches (slice C) — archive acquisition

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | Backend command owns resolve+download+dispatch; reuse `*_from_bytes` seam | `install_modpack(provider, id)` → `get_versions` → primary file → stage to `cache/installers` → dispatch | low | version ordering assumption |
| B | Frontend orchestrates: `getModVersions` in TS, then a generic download command, then `importMrpack(path)` | FE picks file + owns manual/provider routing | med | duplicates provider/manual logic in TS; loses backend-owns-archive guarantee |
| C | New full parallel executor that streams provider file → parse | re-implements A/B body | high | duplicates proven logic; against the pure-planner split |

### Recommendation (slice C)

**A.** The executor bodies already operate on bytes after step 1 — extracting a `*_from_bytes`
inner fn is a near-zero-risk refactor that lets the new command reuse the *exact* proven import
path. Backend owning the whole archive (resolve → download → parse) preserves the slice-A/B
security guarantees (host handling, path safety, manual surfacing) that the design doc's
"backend owns the file end to end" rejected-approach demands. B leaks format/provider logic into
TS; C duplicates code. One new command, one small refactor, one UI action.

### Checkpoints (slice C)

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| C1 | Refactor executors: extract `import_mrpack_from_bytes` / `import_cf_zip_from_bytes` inner fns (take bytes + the existing `name_override`); existing `import_mrpack` / `import_curseforge_zip` read file → call inner. No behavior change. **Commit + confirm A/B test-green before C2–C4.** | `src-tauri/src/lib.rs` | atomic-surgeon | ~1 | existing slice A/B modpack + lib tests pass unmodified |
| C2 | Pack-file resolver (seam): given provider + project id + mock `ProviderHttpClient`, call `get_versions` → take first version (newest-first; no date field to sort) → primary file (`primary == true`, else first) → resolved `(url: Option, fileName)`; `url: None` signalled distinctly; empty version list → clear error | `src-tauri/src/core/modpack.rs` (or `providers.rs`) | atomic-builder | ~2 | mock-HTTP unit tests: first version picked; primary picked; no-primary→first file; `url:None`→manual signal; empty versions→err; both providers |
| C3 | `install_modpack` command + tagged `ModpackInstallResult`: resolve (C2) → if no url, manual outcome w/ `page_url`; else stage archive to `cache/installers/<fileName>` (reqwest GET) → dispatch to C1 inner fn by provider, passing pack display name (from version metadata or `fileName`) as `name_override` | `src-tauri/src/lib.rs` | atomic-builder | ~2 | mock-provider shape tests (`lib_tests.rs`-style): modrinth→mrpack result; cf→cf result; cf url None→manual variant (no instance); routes through C1+C2 |
| C4 | Frontend: `installModpack` ipc wrapper + `ModpackInstallResult` union type; `ModpackCard` Install action (primary) + open-page (secondary); install mutation + completion toast reusing existing pattern | `src/lib/ipc.ts`, `src/routes/Browse.tsx` | atomic-builder | ~2 | `npm run build` green; ipc mirrors Rust (camelCase); install + manual toasts surfaced |

### Risks (slice C)

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `get_versions` first-returned is not newest → installs stale version | med | Take first returned (both providers return newest-first); `ProjectVersion` has no date field to sort on. If live testing shows wrong order, add a date field as a follow-up |
| Modpack `get_versions` needs no mc/loader filter but providers expect them | low | Call with `None, None`; signature already takes `Option`s (slice-B/Phase-5 seam) |
| CF pack-level distribution disabled (`url: None`) | low | Distinct manual variant opens `page_url`; no instance created |
| Pack archive large → slow single GET with no progress | med | Accept for slice C (card pending state); live progress is a follow-up |
| Partial instance on mid-import failure | med | Same pre-existing gap as A/B (`modpack-import-partial-cleanup`); not re-litigated here |
| Refactor accidentally changes A/B behavior | low | C1 is byte-identical body extraction; A/B tests gate it before C2+ build on top |

## Change log

### 2026-06-16 — Slice C: "latest version" = first returned (no date sort)

**What changed:** C2's latest-version selection now takes the **first version returned** by
`get_versions` rather than sorting by date. The normalized `ProjectVersion` type (`providers.rs`)
carries no date field, so date-sorting is not possible without widening that type (and every
version consumer). Updated success criterion 2, the C2 checkpoint, and the ordering risk row.

**Why:** Discovered during C2 planning — `ProjectVersion` has no `date_published`/`fileDate`
field. Both providers return versions newest-first from their APIs, so first-returned is correct
in practice. Adding a date field is deferred to a follow-up if live testing shows the order is
unreliable. **Superseded:** the prior C2 contract said "version ordering made deterministic
(newest first; sort if not guaranteed)" — sorting was not feasible.

### 2026-06-16 — Slice C (Browse → one-click install) added

**What changed:** Added the `## Slice C` contract (goal, non-goals, success criteria,
approaches, recommendation, checkpoints C1–C4, risks). Retitled the spec to cover three slices;
marked slices A and B shipped. Removed "Browse / one-click pack install from a provider — slice
C" from the top-level Non-goals — now in scope as slice C. Updated the header note to mark slice
C active.

**Why:** Slices A/B shipped; slice C is the next roadmap slice (the headline "one-click install"
behavior). Reuses the proven A/B byte-processing path via a small `*_from_bytes` refactor plus a
new provider-resolve+download front-end. No A/B behavior changes.

### 2026-06-16 — Import entry point moved to New Instance modal

**What changed:** The modpack import entry points (`.mrpack` and CurseForge `.zip`) are no
longer buttons on the Instances list header. They live inside a Create/Import tab switch in
the New Instance modal (`src/components/NewInstanceModal.tsx`). The modal's Import tab opens
a single file picker accepting both extensions and routes to the matching IPC command by
extension. On success the modal closes, the caller navigates to the new instance, and the
existing result-summary toasts (`ImportResultToast` / `CfImportResultToast`) are surfaced from
`Home.tsx` via callbacks (`onMrpackImport` / `onCfImport`). `Home.tsx` header now contains
only the New instance button.

**Why:** Consolidates instance creation paths into a single modal entry point, reducing header
clutter. Decided in the `ui-modpack-rework` spec (CP4).

**Superseded:** Import was triggered from two standalone buttons (`Import .mrpack`, `Import
CurseForge .zip`) in the `Home.tsx` header alongside the New instance button.

### 2026-06-15 — Slice B (CurseForge `.zip` import) added

**What changed:** Added the `## Slice B` contract (goal, success criteria, approaches,
checkpoints B1–B5, risks). Retitled the spec to cover both slices and marked slice A shipped
(commit `505670b`). Removed "CurseForge `.zip` import" from the top-level Non-goals — it is now
in scope as slice B.

**Why:** Slice A shipped; slice B is the next roadmap slice. Extends the proven slice-A
pure-planner/thin-executor split to CF packs, whose files need per-(projectID,fileID) URL
resolution through the CF API and manual surfacing for distribution-disabled files.

## Implementation log

### built — 2026-06-15 (slice A, `.mrpack`; shipped `505670b`)

Built across 5 checkpoints via subagent implement→review loop.

- CP1–3 — `core/modpack.rs` (new): `parse_modrinth_index`, `build_pack_plan` (pure), `extract_overrides` (zip-slip safe). atomic-builder; reviewed PASS; 3 reviewer risks fixed by atomic-surgeon (see below). 30 unit tests + JSON fixtures.
- CP4 — `import_mrpack` command + `MrpackImportResult` in `lib.rs`; pure `read_mrpack(bytes, mc_dir)` seam in `core/modpack.rs`. atomic-builder; reviewed PASS. 35 modpack tests; full lib 405.
- CP5 — `tauri-plugin-dialog` added (Cargo + npm + `.plugin()` + `dialog:default` capability); `importMrpack` ipc wrapper + `MrpackImportResult` TS type; `Home.tsx` "Import .mrpack" button (file picker → import → invalidate `["instances"]` → navigate to `/instances/:slug` → result toast). `npm run build` green; full lib 405.

Verified: full Rust lib **405 tests pass** (Windows toolchain); `npm run build` green. No live network in tests (pure parse/plan/extract fixture-tested).

**Reviewer findings addressed in-iteration:**
- `is_safe_dest` was logically broken (doubled-base walk could false-pass `..`); rewritten to a purely structural relative-path guard, dropping the `canonicalize` dependency (so `mc/` need not exist pre-extraction).
- Empty `downloads: []` produced a misleading "disallowed host ''" error → distinct `ModpackError::NoDownloadUrls`.
- Command re-implemented the zip-parse inline instead of using the tested `read_mrpack` seam → command now routes its plan through `read_mrpack` (tested code = shipped code; a pre-create `parse_modrinth_index` still runs to build `CreateInstanceReq` before the slug exists — unavoidable ordering).

**Deferred (follow-up `modpack-import-partial-cleanup`):** no rollback if `build_pack_plan`/`execute_plan` fails after `instances::create` — a half-populated instance is left on disk. Acceptable for slice A; revisit in slice D.

**Not done (needs GUI, not testable in WSL):** manual end-to-end import of a real `.mrpack` + launch. Backend + build verified; GUI run pending the WSLg/Windows-launch decision.

### built — 2026-06-15 (slice B, CurseForge `.zip`)

Built across 5 checkpoints (B1–B5) via the `/subagent-implementation` loop. Commits (chronological):

- `cfbabf2` — B1 CF `manifest.json` parser (`CfManifest`/`CfManifestFile`, `parse_cf_manifest`; loader-id split; reuses `MalformedManifest` + `PackLoader`). 9 tests.
- `4f6f966` — B2 `CurseForgeProvider::get_file` single-file resolver over the existing GET seam (`downloadUrl: null → url None`; sha1 preferred over md5). 9 mock-HTTP tests.
- `dce8bda` — B3 pure `build_cf_pack_plan` → `CfPackPlan { items, mods, manual, skipped }`; url-None/no-sha1 → manual; ModEntry CF ids; reuses `validate_relative_path`. 6 tests. (Reviewer nit — unused `manifest` param — fixed in-iteration by atomic-surgeon.)
- `fda2c8a` — B4 `import_curseforge_zip` command + `CfImportResult`; `resolve_and_build_cf_plan` seam (injectable client, mock-tested). **Round 2** after CHANGES_REQUESTED: error routing branched by kind — `KeyMissing` aborts; network/HTTP/JSON → `failed` (`CfResolveFailure`); only successful distribution-disabled → `manual`; `IndexNotFound` message generalized.
- `f6a6556` — B5 `importCurseforgeZip` ipc wrapper + `CfImportResult`/`CfManualFile` TS types; `Home.tsx` CF `.zip` button + `CfImportResultToast` (manual list links). Caught + fixed missing `rename_all = "camelCase"` on `CfManualFile`.

Verified by orchestrator: full Rust lib **436 tests pass** (Windows toolchain); `npm run build` green. No live network in any test (parse/plan pure; resolution via mock `ProviderHttpClient` seam).

**Out-of-scope work performed during this build:**
- Added `#[serde(rename_all = "camelCase")]` to `CfManualFile` (B5) — required for IPC wire-shape parity, not anticipated in the B4 checkpoint.

**Unforeseens:**
- B4 round-1 conflated all `get_file` errors into `manual`. Reviewer caught it; round-2 split KeyMissing (abort) / resolution-error (`failed`) / distribution-disabled (`manual`).

**Deferred items still open:**
- `modpack-import-cf-overrides-dir` (risk) — CF non-default `overrides` key dir name not honored (reuses slice-A `extract_overrides`).
- `modpack-import-cf-manual-slug-link` (nit) — manual link uses numeric `projects/<id>`, not the exact file page.
- `modpack-import-partial-cleanup` (shared with slice A) — no rollback of a half-populated instance on mid-import failure.
- Batch CF file resolution (`POST /v1/mods/files`) — slice-D perf optimization; slice B resolves sequentially per file.
