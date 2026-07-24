# resolver

## Overview

Turns a MC version id into a `DownloadPlan` + `LaunchMeta`. Fetches piston-meta `version_manifest_v2.json`, fetches and disk-caches the per-version manifest via `meta::cached_text`, evaluates OS-specific library `rules`, selects classpath and native jars, expands the asset index, and assembles one flat `DownloadPlan`. `merge_loader_profile` overlays a Fabric/Quilt/Forge/NeoForge loader profile onto a resolved `DownloadPlan` + `LaunchMeta`. Pure — no downloads executed here.

URL routing in `merge_loader_profile`: `url.ends_with(".jar")` → full artifact URL (Forge/NeoForge); otherwise `base_url + "/" + maven_path` (Fabric/Quilt); `url=None` or `url=""` → classpath-only, no `DownloadItem`.

## CLI code

- `src-tauri/src/core/resolver.rs` — typed serde structs (`ManifestEntry`, `VersionManifest`, `DownloadSpec`, `JavaVersion`, `AssetIndex`, `Arguments`, `ArgumentEntry`, `ConditionalArgument`, `ArgumentValue`, `Library`, `LibraryDownloads`, `NativeArtifact`, `VersionSpec`, `AssetObject`, `AssetIndexData`, `LaunchMeta`, `ResolveResult`, `LoggingFile`, `LoggingClient`, `Logging`); `eval_rules`, `select_classpath`, `select_natives`, `asset_objects_to_items`, `fetch_asset_index`, `fetch_version_spec`, `assemble` (pure, testable with recorded fixtures), `host_os_name`; `merge_loader_profile(plan, launch, profile, target_os, data_dir)` — overrides `main_class`, prepends loader libs (client jar kept last), appends OS-filtered loader jvm/game args; ends with `#[cfg(test)] #[path = "resolver_tests.rs"] mod tests;`
- `src-tauri/src/core/resolver_tests.rs` — 43 unit tests using JSON fixtures (no live HTTP)
- `src-tauri/src/core/fixtures/` — `version_manifest_modern.json`, `version_manifest_legacy.json`, `asset_index_sample.json`, `fabric_profile.json`, `neoforge_profile.json`
- `src-tauri/src/lib.rs` — `resolve_vanilla` async Tauri command (vanilla-only; loader merge happens in `launch_instance`)

## Artifacts

- `src/lib/ipc.ts` — `LaunchMeta`, `ResolveResult`; `resolveVanilla(versionId)` wrapper

## Docs

- `docs/spec/vanilla-resolver.md` — Phase 2 slice B spec
- `docs/spec/fabric-quilt-launch.md` — CP2: `merge_loader_profile` contract
- `docs/spec/neoforge-forge-launch.md` — CP2: full-artifact URL vs base URL, url=None classpath-only

## Coupling

- `download` domain — `assemble` + `merge_loader_profile` produce `DownloadPlan`; changes to `DownloadItem`/`ExpectedHash` require updates here.
- `metadata` domain — `fetch_version_spec` calls `meta::cached_text`; `loader_profile.rs` imports `Arguments`/`ArgumentEntry` from this module.
- `launch` domain — `LaunchMeta` carries `${...}` placeholder arg templates; substitution + native extraction are launch's responsibility. `merge_loader_profile` called from `lib.rs::launch_instance`, not from `resolve_vanilla`.

## Conventions

- `VERSION_TTL = 365d` (per-version manifests are immutable by id). `MANIFEST_TTL = 6h`. `ASSET_INDEX_TTL = 365d`.
- `eval_rules`: empty rules → allowed; non-empty → default disallowed, last matching rule wins.
- `host_os_name()` maps `std::env::consts::OS` → Mojang names (`macos`→`osx`; unknown → pass-through).
- Native classifier `${arch}` → `"64"`.
- Asset objects dest: `<data_dir>/assets/objects/<hash[..2]>/<hash>`. Client jar: `<data_dir>/versions/<id>/<id>.jar`. Libraries: `<data_dir>/libraries/<maven_path>`.
- Feature-gated `ConditionalArgument` entries (those with a `features` key) excluded from jvm/game args.
- `assemble` is pure — no `AppHandle`, no I/O.
- `merge_loader_profile` classpath contract: pop client jar, extend with loader libs in profile order, push client jar back.
- `neoforge_profile.json` fixture: 4 libs with artifact URL, 1 with empty `downloads` (url=None, processor-produced); used to test None-url classpath-only behaviour.
