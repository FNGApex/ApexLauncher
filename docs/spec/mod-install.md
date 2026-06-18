# Mod install (Phase 5 slice B)

## Goal

Install a mod from a provider into an instance with automatic required-dependency
resolution; enable/disable, update, and remove installed mods; degrade
distribution-disabled files to a manual-download prompt. Modrinth path fully functional
with no API key; CurseForge rides the same code path, using the available CF API key
(env / `settings.curseforge_api_key` / baked tier).

## Non-goals

- Modpack import (`.mrpack` / CF zip) — Phase 6.
- CF fingerprint matching / provider detection for hand-dropped jars.
- Background update polling / update-all batch.
- Cross-provider dependency resolution.
- Client/server side filtering beyond storing the declared `side`.

## Success criteria

- [ ] Adding a mod by `(provider, projectId, versionId, slug, mcVersion, loader)` downloads
      its primary file into `<instances>/<slug>/mc/mods/` and appends a `ModEntry` to
      `instance.json`.
- [ ] Required transitive dependencies are resolved and installed; `optional` surfaced as
      suggestions, `incompatible` as warnings, `embedded` ignored.
- [ ] A dependency or mod already present in the manifest (by `projectId`) is skipped, not
      re-downloaded or duplicated.
- [ ] A resolved file with `url == None` produces a manual-download entry (filename +
      page URL), and does **not** abort the rest of the install.
- [ ] A dependency with no compatible version produces an `unresolved` entry, not a failure
      of the whole operation.
- [ ] Enabling/disabling an installed mod renames its file by adding/removing the
      `.disabled` suffix and flips `ModEntry.enabled`, with no re-download.
- [ ] Updating an installed mod to a newer compatible version downloads the new file,
      deletes the old file, and updates the `ModEntry` (version id, file name, hashes).
- [ ] Removing an installed mod deletes its file and drops its `ModEntry`.
- [ ] All file-name inputs crossing the IPC boundary are traversal-validated (no `/`, `\`,
      `..`, absolute paths) before touching the filesystem.
- [ ] Planning logic (dependency BFS, version selection, dedup, manual/unresolved
      partition) is unit-tested via the injected `ProviderHttpClient` mock — no live network.
- [ ] `cargo test` green; `npm run build` green.

## Approaches

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | Split pure planner + thin executor; new `core/mod_install.rs` | `resolve_install` returns `InstallPlan`; executor downloads + writes manifest | med | low |
| B | Monolithic `add_mod` inline | resolve+download+write in one command | low | high — untestable w/o live HTTP |
| C | Resolve deps in frontend TS | backend downloads flat list | med | high — logic duplicated across two langs |

## Recommendation

**Approach A.** Reuses the injectable `ProviderHttpClient` mock seam (`providers.rs:169`)
so dependency BFS and version selection are unit-tested against canned responses, matching
the `resolver.rs` / `auth.rs` test patterns. `update_mod` reuses the same planner. CF needs
no branch — same `ModProvider` trait and normalized types; only the key gates its data.
Full rationale + Mermaid flow in `docs/design/mod-install.md`.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Install planner: `resolve_install` (dependency BFS, newest-compatible version pick, dedup vs installed, partition into download/manual/unresolved/suggestion/warning), `InstallPlan` + entry types | `src-tauri/src/core/mod_install.rs` (new), `core/mod.rs` | atomic-builder | ~3 | unit tests w/ mock `ProviderHttpClient`: required-dep recursion, cycle guard, dedup skip, `url==None`→manual, no-compatible→unresolved, optional/incompatible/embedded handling |
| 2 | Install executor + `add_mod` command: build `DownloadPlan` (dest `mc/mods/<fileName>`, hash from `VersionFile.hashes`), run `execute_plan`, append/merge `ModEntry` into manifest; `AddModResult { added, manual, unresolved, suggestions, warnings }` IPC type | `src-tauri/src/core/mod_install.rs`, `src-tauri/src/lib.rs` | atomic-builder | ~2 | tests: planned downloads → correct `DownloadItem` shape + dest path; manifest gains entries; `ExpectedHash` chosen from available hash algos (sha512/sha1) |
| 3 | Mod state ops + commands: `set_mod_enabled` (rename ±`.disabled`, flip flag), `remove_mod` (delete file + drop entry); traversal-safe `file_name` validation | `src-tauri/src/core/instances.rs` (or `mod_install.rs`), `src-tauri/src/lib.rs` | atomic-builder | ~2 | tests: enable/disable renames + flips flag idempotently; remove deletes file + entry; invalid file_name rejected |
| 4 | Update: `update_mod` command — resolve newest compatible for a tracked mod's project, download new file, delete old, update `ModEntry`; no-op when already newest | `src-tauri/src/core/mod_install.rs`, `src-tauri/src/lib.rs` | atomic-builder | ~2 | tests: newer version → swap (entry version/file/hash updated, old file gone); same version → no-op |
| 5 | Frontend: ipc wrappers + InstanceDetail install UI (Manage-installs slide-over with mod search+add and enable/disable/remove/update controls; manual-download → open project page in browser) | `src/lib/ipc.ts`, `src/routes/InstanceDetail.tsx`, `src/components/SlideOver.tsx` | atomic-builder | ~3 | `npm run build` green; ipc types mirror Rust structs (camelCase); manual entries open `page_url` via Tauri opener |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Provider returns versions unordered → wrong "newest" picked | low | Rely on documented newest-first order now; add explicit date sort if evidence shows otherwise (Open question in design) |
| CF distribution-disabled dep reached by id-only → unbuildable page URL | low (Modrinth-first; top-level add has slug) | Fall back to id-based URL; tracked as follow-up; CF data not live yet |
| Path traversal via crafted `file_name` / `slug` over IPC | med | Validate every file_name (reject `/ \ .. ` + absolute) + reuse `validate_slug`; checkpoint 3 success criterion |
| Download engine hits real network in tests | med | Planner is pure + mock-tested (CP1/CP4); executor tests assert plan shape, not live bytes |
| Hash algo absent from `VersionFile.hashes` (CF often empty) | med | `expected_hash = None` when no usable algo (engine allows it); prefer sha512 then sha1 |
| IPC type drift (`ipc.ts` hand-mirrored) | med | CP5 updates `ipc.ts` in lockstep; cross-cutting risk noted in signals |

## Change log

### 2026-06-16 — Mod add/manage moves to per-instance slide-over (ui-modpack-rework CP5)

**What changed:** The mod-add entry point moved from the global Browse page
(`AddToInstanceModal` with an instance picker) into a "Manage installs" slide-over
accessible from the instance page. The slide-over contains two tabs: **Installed**
(enable/disable/update/remove — unchanged behavior) and **Add mod** (search + source
toggle CF/Modrinth, version resolve for the current instance, `addMod`, result summary).
A reusable `SlideOver` component (`src/components/SlideOver.tsx`) was introduced.

**Why:** Browse was redesigned as a modpack discovery feed (ui-modpack-rework CP3) and
no longer carries an add-to-instance flow. Consolidating all per-instance mod management
into one slide-over improves discoverability and keeps Browse focused.

**Superseded:** CP5 previously described the frontend as "add from Browse version picker
+ instance detail controls" with `Browse.tsx` as a delivery file. `Browse.tsx` is no
longer part of the mod-install surface; the entry point is `InstanceDetail.tsx` +
`SlideOver.tsx` only.

## Implementation log

### shipped — 2026-06-15

Built across 5 checkpoints (7 implement→review iterations) of /subagent-implementation.
Commits (chronological):

- `09d3b13` — CP1 dependency-resolving install planner (`resolve_install`, `InstallPlan`)
- `2ad6cbd` — CP2 install executor + `add_mod` command (URL-based outcome attribution)
- `6717db6` — CP3 enable/disable + remove commands + `validate_mod_file_name` traversal guard
- `ba5a3a9` — CP4 `update_mod` command + provider-file-name traversal validation in both paths
- `3553b97` — CP5 frontend UI (Browse add-to-instance modal, InstanceDetail per-mod controls)
- `96a9d48` — polish: surface version-fetch errors in add-to-instance modal (F-12)

Verified: 369 Rust lib tests pass (+54 from this work); `npm run build` green.

**Out-of-scope work performed during this build:**
- Provider-supplied `file_name` traversal validation added to the `add_mod` path too (not
  just CP4's `update_mod`) — same security class, fixed together (CP4 review).
- Windows drive-letter prefix rejection added to `validate_mod_file_name` (CP3 review).

**Unforeseens — surprises that emerged during implementation:**
- `resolve_install` needs `root_project_id` passed explicitly — `ProjectVersion.id` is the
  version id, in a different namespace than `Dependency.project_id` used by the dedup guard.
- `download::execute_plan` returns outcomes in completion order (FuturesUnordered) with
  already-skipped items prepended; index-matching outcomes to the plan misattributes them.
  Fixed with URL-based `attribute_outcomes` (CP2 review).
- Provider `file_name` is network-trusted input; treated as a traversal vector and validated.

**Deferred items still open:**
- `mod-install-f-1` — thread provider client/server `side` into `ModEntry` (hardcoded
  "unknown" today). Tracked in `.claude/project/followups/`.
- `mod-install-f-6` — enable/disable rename + manifest write not atomic (self-heals via
  `scan_mods` disk-truth). Tracked.
- 11 cosmetic nits (F-2..F-5, F-7..F-11, F-13, F-14) — reviewed and dropped.
- Pre-existing `cp4_concurrency_bound_not_exceeded` download-engine flake — unrelated to this
  work, already tracked separately.
