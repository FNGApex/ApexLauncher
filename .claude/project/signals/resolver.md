# resolver

## What it does

Turns a Minecraft version id into a `DownloadPlan` (consumed by the download engine) plus a `LaunchMeta` (consumed by launch). Fetches the piston-meta `version_manifest_v2.json` to find each version's per-version manifest URL, fetches and disk-caches that per-version manifest via `meta::cached_text`, evaluates OS-specific library `rules`, selects classpath and native jars, fetches and caches the asset index, and assembles everything (client jar + libs + natives + asset index file + asset objects + optional logging config) into one flat `DownloadPlan`. Also provides `merge_loader_profile` which overlays a Fabric/Quilt loader profile onto a resolved vanilla `DownloadPlan` + `LaunchMeta`. No downloads executed here — pure metadata → plan + launch contract.

## CLI code

- `src-tauri/src/core/resolver.rs` — full resolver: typed serde structs (`ManifestEntry`, `VersionManifest`, `DownloadSpec`, `JavaVersion`, `AssetIndex`, `ManifestDownloads`, `Arguments`, `ArgumentEntry`, `ConditionalArgument`, `ArgumentValue`, `Library`, `LibraryDownloads`, `NativeArtifact`, `VersionSpec`, `AssetObject`, `AssetIndexData`, `LaunchMeta`, `ResolveResult`, `LoggingFile`, `LoggingClient`, `Logging`); `eval_rules` (last-match-wins, OS-named rules); `select_classpath`; `select_natives`; `asset_objects_to_items`; `asset_index_file_item`; `fetch_asset_index`; `fetch_version_spec`; `assemble`; `host_os_name` (maps `consts::OS` → Mojang names: `macos`→`osx`); `merge_loader_profile(plan, launch, profile, target_os, data_dir)` — overrides `main_class`, prepends loader libs to classpath (profile order, client jar kept last, `expected_hash: None`), appends OS-filtered loader jvm/game args, skips empty-url libs; 42 unit tests using JSON fixtures (no live HTTP)
- `src-tauri/src/core/fixtures/` — 4 JSON fixtures: `version_manifest_modern.json`, `version_manifest_legacy.json`, `asset_index_sample.json`, `fabric_profile.json`
- `src-tauri/src/lib.rs` — `resolve_vanilla` async Tauri command: calls `fetch_version_spec` + `fetch_asset_index` + `assemble`, returns `ResolveResult` (vanilla-only; loader merge happens in `launch_instance`, not here)

## Artifacts

- `src/lib/ipc.ts` — `LaunchMeta`, `ResolveResult` interfaces; `resolveVanilla(versionId)` wrapper (line ~216+)

## Docs

- `docs/spec/vanilla-resolver.md` — Phase 2 slice B spec: success criteria, 4 checkpoints, approach table (A selected: typed structs), risks, implementation log (5 commits, shipped 2026-06-06)
- `docs/design/vanilla-launch.md` — upstream design: approach table, plan/execute seam rationale, future slices (C Java mgr, D launch)
- `docs/spec/fabric-quilt-launch.md` — Phase 4 slice A spec; CP2 defines `merge_loader_profile` contract (classpath order, arg appending, empty-url skip)
- `docs/design/fabric-quilt-launch.md` — design rationale for profile-overlay approach; explains why vanilla client jar must remain last on classpath

## Coupling

- **download domain:** `assemble` produces a `DownloadPlan` composed of `DownloadItem` values defined in `core/download.rs`; `merge_loader_profile` also pushes `DownloadItem` entries (loader libs, `expected_hash: None`); changes to `DownloadItem` or `ExpectedHash` require updates here.
- **metadata domain:** `fetch_version_spec` calls `meta::cached_text` (same helper used by `versions.rs`/`loaders.rs`) with `VERSION_TTL = 365d` and `MANIFEST_TTL = 6h`; `meta::cached_text` builds a new `reqwest::Client` per call (no shared client, same pattern as rest of metadata domain). `loader_profile.rs` imports `Arguments` / `ArgumentEntry` from this module — type changes propagate to both.
- **frontend-shell / ipc.ts:** `LaunchMeta` and `ResolveResult` are hand-mirrored in `src/lib/ipc.ts` with camelCase rename (no specta/ts-rs); any Rust field change requires a manual `ipc.ts` update.
- **launch domain:** `LaunchMeta` carries `${...}` placeholder arg templates; substitution and native extraction are launch's responsibility. `merge_loader_profile` is called from `lib.rs::launch_instance` (not from `resolve_vanilla`) — vanilla resolver stays vanilla-only.

## Conventions worth knowing

- `VERSION_TTL = 365d` (per-version manifests are immutable by id); `MANIFEST_TTL = 6h` (top-level version list); `ASSET_INDEX_TTL = 365d` (asset indexes are content-addressed).
- `eval_rules` semantics: empty rules → allowed; non-empty rules → default disallowed, apply in order, last matching rule wins; malformed rule entries are silently skipped via `from_value` error handling.
- `host_os_name()` maps `std::env::consts::OS` → Mojang names (`macos`→`osx`; `windows`→`windows`; `linux`→`linux`; unknown → pass-through).
- Native classifier `${arch}` token is substituted with `"64"` (32-bit not needed for supported MC versions).
- Asset objects dest path: `<data_dir>/assets/objects/<hash[..2]>/<hash>`. Asset index file dest: `<data_dir>/assets/indexes/<id>.json`. Client jar: `<data_dir>/versions/<id>/<id>.jar`. Libraries: `<data_dir>/libraries/<maven_path>`. Logging config: `<data_dir>/assets/log_configs/<file.id>`.
- Legacy asset layout (`virtual` or `map_to_resources` flags) is detected and surfaced as `assets_legacy: bool` in `LaunchMeta`; virtual-path mapping itself is deferred (supported floor is 1.7.10, which uses modern `objects/` layout).
- Feature-gated `ConditionalArgument` entries (those with a `features` key in any rule) are excluded from `jvm_args`/`game_args` — non-default features (demo mode, custom resolution) are off by default.
- `assemble` is pure (no `AppHandle`, no I/O) — testable with recorded fixtures; network calls are isolated to `fetch_version_spec` and `fetch_asset_index`.
- `merge_loader_profile` is also pure (no `AppHandle`, no I/O) — takes mutable refs to an existing `DownloadPlan` and `LaunchMeta`; 9 of the 42 tests cover it using inline `LoaderProfile` values and `fabric_profile.json` fixture.
- Classpath contract from `assemble`: vanilla client jar is always the last entry. `merge_loader_profile` preserves this by popping the jar, extending with loader libs, then pushing it back.
