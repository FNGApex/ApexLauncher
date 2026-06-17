# modpack

## What it does

Imports a local modpack archive (Modrinth `.mrpack` slice A, CurseForge `.zip` slice B) or installs a pack directly from Browse one-click (slice C) into a new instance: parse/plan/extract the archive, create the instance with the pack's MC version + loader, resolve/verify each file, download into `mc/`, apply `overrides/`, and record installed files as `ModEntry`s. `core/modpack.rs` is a pure parse/plan module (no Tauri commands, no filesystem writes except `extract_overrides`); `import_mrpack`, `import_curseforge_zip`, and `install_modpack` in `lib.rs` are the thin executors. Slice C adds `resolve_pack_file` (fetches the latest version + selects primary file from a `ModProvider`) and `ModpackInstallResult` (tagged enum: `Mrpack` | `Curseforge` | `Manual`).

## Artifacts

- `src/components/NewInstanceModal.tsx` — Import tab: single file picker via `@tauri-apps/plugin-dialog`'s `open()`, routes by extension (`.mrpack` → `importMrpack`, `.zip` → `importCurseforgeZip`); on success invalidates `["instances"]`, closes the modal, and navigates to the new instance; `onMrpackImport`/`onCfImport` callbacks let `Home.tsx` surface result toasts
- `src/routes/Home.tsx` — exports `ImportResultToast` (installed/skipped/failed counts + failed file list) and `CfImportResultToast` (installed/failed/manual counts; manual list with project-page links) as named exports; renders them via modal callbacks; no import buttons directly on this page
- `src/routes/Browse.tsx` — `ModpackCard` has a primary **Install** button (calls `installModpack(pack.provider, pack.id, pack.pageUrl ?? undefined)`) and a secondary open-page button; imports `ImportResultToast`/`CfImportResultToast` from `src/routes/Home.tsx`; `installResult` state holds `ModpackInstallResult | null`

## CLI code

- `src-tauri/src/core/modpack.rs` — pure parse/plan/extract module; slice A: `MrpackManifest`/`MrpackFile`/`FileEnv`/`PackLoader` types, `parse_modrinth_index`, `PackPlan`, `build_pack_plan` (host allowlist, hash pick sha512>sha1, env filter, path-safety guard), `extract_overrides`/`extract_prefix`/`is_safe_dest` (zip-slip-safe, applies `overrides/` then `client-overrides/`, ignores `server-overrides/`), `read_mrpack` (in-memory zip seam); slice B: `CfManifest`/`CfManifestFile`/`CfManualFile`/`CfResolveFailure` types, `parse_cf_manifest`, `CfPackPlan`, `build_cf_pack_plan`, `resolve_and_build_cf_plan` (async, injectable `ProviderHttpClient`); slice C: `ResolvedPackFile { url: Option<String>, file_name: String, provider: ProviderKind }`, `resolve_pack_file(provider, client, project_id)` (fetches versions newest-first, picks primary file, returns `url: None` for distribution-disabled without erroring); shared `ModpackError` enum (+ `NoVersions`/`NoFiles`/`ResolverError` variants for slice C); shared `validate_relative_path`; 990 lines; 65 unit tests in sibling `src-tauri/src/core/modpack_tests.rs` (1319 lines), wired via `#[cfg(test)] #[path = "modpack_tests.rs"] mod tests;` stub
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider::get_file(client, project_id, file_id)` (single-file resolver for slice B, `GET /v1/mods/{projectId}/files/{fileId}`)
- `src-tauri/src/lib.rs` — `import_mrpack` (async Tauri command): reads file bytes, pre-parses manifest for `CreateInstanceReq`, calls `instances::create`, re-parses through `read_mrpack(bytes, mc_dir)` seam, runs `download::execute_plan`, calls `extract_overrides`, merges `ModEntry`s, returns `MrpackImportResult { slug, name, installed, failed, skipped }`; `import_curseforge_zip` (async): reads bytes, `read_cf_manifest`, `instances::create`, resolves CF key, builds `CurseForgeProvider` + `ReqwestProviderClient`, calls `resolve_and_build_cf_plan`, runs `execute_plan`, `extract_overrides`, merges `ModEntry`s, returns `CfImportResult { slug, name, installed, failed, manual }`; `install_modpack(app, provider, project_id, page_url)` (slice C, async): constructs the real provider by `provider` string, calls `resolve_pack_file`, returns `ModpackInstallResult::Manual` if `url: None` (no instance created), else stages the archive to `cache/installers/<file_name>` via reqwest GET, dispatches to `import_mrpack_from_bytes`/`import_cf_zip_from_bytes` by `ProviderKind`; `ModpackInstallResult` tagged enum (`#[serde(tag = "kind", rename_all = "camelCase")]`): `Mrpack(MrpackImportResult)`, `Curseforge(CfImportResult)`, `Manual { pageUrl, fileName }`

## Docs

- `docs/spec/modpack-import.md` — implementation contract for all three slices; slice A marked shipped (commit `505670b`); slice B shipped 2026-06-15; slice C (Browse one-click) planned in spec
- `docs/spec/ui-modpack-rework.md` — CP1-CP2 spec: import flow moved to NewInstanceModal Import tab
- `docs/design/modpack-import.md` — design doc: slice A/B format ground truth, behavior rules, Mermaid architecture diagrams, file-resolution approach comparison, rejected approaches
- `docs/design/ui-modpack-rework.md` — rationale for moving import into NewInstanceModal

## Coupling

- **instances domain** — `import_mrpack`/`import_curseforge_zip` call `instances::create`, `instances::load_manifest`, `instances::save_manifest`; `ModEntry` (defined in `instances.rs`) is the output shape both planners produce
- **providers domain** — `resolve_and_build_cf_plan` calls `CurseForgeProvider::get_file` and depends on `ProviderHttpClient`/`ProviderError`/`VersionFile` from `providers.rs`/`curseforge.rs`; any change to those types or to `cf_api_key_from` requires a matching update here
- **download domain** — both executors build a `DownloadPlan` from planner output and call `download::execute_plan`/`build_client`; `ItemStatus`/`NoOpSink` come from `download.rs`
- **frontend-shell / IPC** — `MrpackImportResult`, `CfImportResult`, `CfManualFile`, `ModpackInstallResult` are hand-mirrored in `src/lib/ipc.ts`; any Rust struct change requires a manual `ipc.ts` edit. Import UI now lives in `NewInstanceModal`'s Import tab; result toasts (`ImportResultToast`, `CfImportResultToast`) are defined and exported from `Home.tsx` and reused in `Browse.tsx`
- **resolver/mod-install domains** — explicitly NOT reused: `mod_install::resolve_install` does provider-id BFS dependency walking, which doesn't apply to pre-resolved mrpack files or pack-manifest-driven CF files

## Conventions worth knowing

- Pure-planner / thin-executor split mirrors `mod_install.rs` and `resolver.rs`: all security-critical logic (host allowlist, path safety, hash verification, env filtering) lives in unit-tested pure functions; the Tauri command only does I/O orchestration.
- mrpack download host allowlist (`ALLOWED_HOSTS`): `cdn.modrinth.com`, `github.com`, `raw.githubusercontent.com`, `gitlab.com` — a `downloads[]` URL outside this list aborts the whole import via `ModpackError::DisallowedHost`. No equivalent allowlist for CF slice B — CF URLs come from the authenticated API response.
- `validate_relative_path` rejects absolute paths, `\`-prefixed paths, Windows drive-letter prefixes, and any `..` component; used by both slice-A file paths and slice-B `mods/<fileName>` dest construction.
- `is_safe_dest` is a purely structural relative-path walk (no `canonicalize`) — chosen deliberately so `mc_dir` need not exist on disk before extraction.
- CF hash preference in `build_cf_pack_plan`: only `sha1` is checked (not md5) — a resolved file with only an md5 hash routes to `manual` rather than downloading unverified.
- `CfPackPlan::failed` (populated only via `resolve_and_build_cf_plan`) is distinct from `CfPackPlan::manual`: `failed` = network/HTTP/JSON call errored; `manual` = call succeeded but the file has no usable URL or hash.
- `CfManualFile.page_url` is built as `https://www.curseforge.com/projects/<projectID>` — numeric-id-based; tracked as follow-up `modpack-import-cf-manual-slug-link`.
- No rollback on partial failure: a half-populated instance may be left on disk — tracked follow-up `modpack-import-partial-cleanup`.
- `src-tauri/tests/curseforge_live.rs` is a `#[ignore]`d integration test hitting the real CF API; reads the key from a gitignored `.env`.
