# Modpack import (Phase 6 slice A — Modrinth `.mrpack`)

## Goal

Import a local Modrinth `.mrpack` file into a new instance: parse `modrinth.index.json`,
create the instance with the pack's MC version + loader, download every client-supported
file (hash-verified, host-allowlisted) to its declared path, apply `overrides/` +
`client-overrides/` verbatim, and record mod files as `ModEntry`s. No API key (mrpack carries
direct URLs).

## Non-goals

- CurseForge `.zip` import (per-file URL resolution, manual surfacing) — slice B.
- Browse / one-click pack install from a provider — slice C.
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
| 5 | Frontend: `importMrpack` ipc wrapper + import entry point (file picker via Tauri dialog plugin, calls command, shows result, navigates to new instance) | `src/lib/ipc.ts`, `src/routes/Home.tsx` (or NewInstanceModal), `src/components/` | atomic-builder | ~3 | `npm run build` green; ipc types mirror Rust (camelCase); result surfaced (installed/failed/skipped) |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Zip-slip via crafted `path`/override entry escapes instance dir | med | Strict relative-path validation (reject `..`, absolute, drive prefix); CP2+CP3 success criteria; resolve under mc dir and verify prefix |
| Untrusted `downloads` URL → arbitrary jar on classpath | med | Host allowlist enforced in CP2; disallowed host aborts import |
| Download engine hits real network in tests | med | Planner + extractor are pure/fixture-tested; CP4 asserts plan shape + wiring, not live bytes |
| Tauri file-dialog plugin not yet a dependency | med | CP5 adds `tauri-plugin-dialog` (capabilities + Cargo + npm); if blocked, accept a path string arg as fallback |
| `instances::create` requires a valid loader build version that the pack pins but metadata can't resolve | low | Store the pack's pinned loader version directly in `Loader.version`; launch-time resolver already handles explicit versions |
| Forge/NeoForge loader version format in mrpack differs from launch expectations | low | Slice A targets Fabric/Quilt packs first in manual verify; Forge pack e2e tracked as follow-up |

## Change log

<!-- Populated on first amendment after approval. -->

## Implementation log

### built — 2026-06-15 (slice A, uncommitted)

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
