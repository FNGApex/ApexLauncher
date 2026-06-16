# resolver

## What it does

Turns a Minecraft version id into a `DownloadPlan` (consumed by the download engine) plus a `LaunchMeta` (consumed by launch). Fetches the piston-meta `version_manifest_v2.json` to find each version's per-version manifest URL, fetches and disk-caches that per-version manifest via `meta::cached_text`, evaluates OS-specific library `rules`, selects classpath and native jars, fetches and caches the asset index, and assembles everything (client jar + libs + natives + asset index file + asset objects + optional logging config) into one flat `DownloadPlan`. Also provides `merge_loader_profile` which overlays a Fabric/Quilt/Forge/NeoForge loader profile onto a resolved vanilla `DownloadPlan` + `LaunchMeta`: for libs with a full artifact URL (`.jar`-ending) uses it directly; for libs with a base Maven repo URL appends the Maven coordinate path; for libs with `url=None` or empty URL adds to classpath only (no download item). No downloads executed here — pure metadata → plan + launch contract.

## CLI code

- `src-tauri/src/core/resolver.rs` (853 lines) — full resolver: typed serde structs (`ManifestEntry`, `VersionManifest`, `DownloadSpec`, `JavaVersion`, `AssetIndex`, `ManifestDownloads`, `Arguments`, `ArgumentEntry`, `ConditionalArgument`, `ArgumentValue`, `Library`, `LibraryDownloads`, `NativeArtifact`, `VersionSpec`, `AssetObject`, `AssetIndexData`, `LaunchMeta`, `ResolveResult`, `LoggingFile`, `LoggingClient`, `Logging`); `eval_rules`; `select_classpath`; `select_natives`; `asset_objects_to_items`; `asset_index_file_item`; `fetch_asset_index`; `fetch_version_spec`; `assemble`; `host_os_name`; `merge_loader_profile(plan, launch, profile, target_os, data_dir)` — overrides `main_class`, prepends loader libs to classpath (profile order, client jar kept last), appends OS-filtered loader jvm/game args; for url=None or url="" libs: classpath-only entry, no `DownloadItem`; for full-artifact URL (`.jar`-ending): use as-is; for base-URL: append maven path
- `src-tauri/src/core/resolver_tests.rs` (1060 lines) — 43 unit tests using JSON fixtures (no live HTTP); relocated out of `resolver.rs` via mechanical refactor, wired back with a `#[cfg(test)] #[path = "resolver_tests.rs"] mod tests;` stub at the end of `resolver.rs`
- `src-tauri/src/core/fixtures/` — 5 JSON fixtures: `version_manifest_modern.json`, `version_manifest_legacy.json`, `asset_index_sample.json`, `fabric_profile.json`, `neoforge_profile.json`
- `src-tauri/src/lib.rs` — `resolve_vanilla` async Tauri command: calls `fetch_version_spec` + `fetch_asset_index` + `assemble`, returns `ResolveResult` (vanilla-only; loader merge happens in `launch_instance`, not here)

## Artifacts

- `src/lib/ipc.ts` — `LaunchMeta`, `ResolveResult` interfaces; `resolveVanilla(versionId)` wrapper

## Docs

- `docs/spec/vanilla-resolver.md` — Phase 2 slice B spec: success criteria, 4 checkpoints, approach table, risks, implementation log
- `docs/design/vanilla-launch.md` — upstream design: approach table, plan/execute seam rationale
- `docs/spec/fabric-quilt-launch.md` — Phase 4 slice A spec; CP2 defines `merge_loader_profile` contract (classpath order, arg appending, url-skip)
- `docs/design/fabric-quilt-launch.md` — design rationale for profile-overlay approach
- `docs/spec/neoforge-forge-launch.md` — Phase 4 slice B spec; CP2 defines the `merge_loader_profile` extensions for forge (full-artifact URL vs base URL, url=None classpath-only)
- `docs/design/neoforge-forge-launch.md` — design rationale for forge profile merge; explains full-vs-base URL routing

## Coupling

- **download domain:** `assemble` produces a `DownloadPlan` composed of `DownloadItem` values defined in `core/download.rs`; `merge_loader_profile` also pushes `DownloadItem` entries (loader libs, `expected_hash: None`); changes to `DownloadItem` or `ExpectedHash` require updates here.
- **metadata domain:** `fetch_version_spec` calls `meta::cached_text` with `VERSION_TTL = 365d` and `MANIFEST_TTL = 6h`. `loader_profile.rs` imports `Arguments` / `ArgumentEntry` from this module — type changes propagate to both.
- **frontend-shell / ipc.ts:** `LaunchMeta` and `ResolveResult` are hand-mirrored in `src/lib/ipc.ts` with camelCase rename; any Rust field change requires a manual `ipc.ts` update.
- **launch domain:** `LaunchMeta` carries `${...}` placeholder arg templates; substitution and native extraction are launch's responsibility. `merge_loader_profile` is called from `lib.rs::launch_instance` (not from `resolve_vanilla`) — vanilla resolver stays vanilla-only.

## Conventions worth knowing

- `VERSION_TTL = 365d` (per-version manifests are immutable by id); `MANIFEST_TTL = 6h` (top-level version list); `ASSET_INDEX_TTL = 365d` (asset indexes are content-addressed).
- `eval_rules` semantics: empty rules → allowed; non-empty rules → default disallowed, apply in order, last matching rule wins; malformed rule entries are silently skipped.
- `host_os_name()` maps `std::env::consts::OS` → Mojang names (`macos`→`osx`; `windows`→`windows`; `linux`→`linux`; unknown → pass-through).
- Native classifier `${arch}` token is substituted with `"64"`.
- Asset objects dest path: `<data_dir>/assets/objects/<hash[..2]>/<hash>`. Asset index file dest: `<data_dir>/assets/indexes/<id>.json`. Client jar: `<data_dir>/versions/<id>/<id>.jar`. Libraries: `<data_dir>/libraries/<maven_path>`. Logging config: `<data_dir>/assets/log_configs/<file.id>`.
- Legacy asset layout (`virtual` or `map_to_resources` flags) detected and surfaced as `assets_legacy: bool` in `LaunchMeta`; virtual-path mapping deferred.
- Feature-gated `ConditionalArgument` entries (those with a `features` key in any rule) are excluded from `jvm_args`/`game_args`.
- `assemble` is pure (no `AppHandle`, no I/O) — testable with recorded fixtures.
- `merge_loader_profile` is pure — takes mutable refs to existing `DownloadPlan` and `LaunchMeta`; classpath contract: pop client jar, extend with loader libs in profile order, push client jar back.
- URL routing in `merge_loader_profile`: `url.ends_with(".jar")` → full artifact URL (Forge/NeoForge format); otherwise `base_url.trim_end_matches('/') + "/" + maven_path` (Fabric/Quilt format).
- `neoforge_profile.json` fixture has 5 libs: 4 with artifact URL, 1 with empty `downloads` (url=None, processor-produced); used in resolver tests for None-url classpath-only behaviour.
