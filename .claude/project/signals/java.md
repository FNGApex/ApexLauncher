# java

## Overview

Two files. `java.rs`: JRE detection and provisioning — detects installed system JREs or provisions Temurin from Adoptium (traversal-safe tar.gz/zip extraction); `ensure_java(major)` returns a `JavaInstallation`; `validate_java_path(path)` probes a custom binary and returns `JavaProbe { major, version }`.

`java_resolve.rs`: pure helper — no I/O — applies 3-tier precedence (pack-recommended → per-instance override → global settings) to produce `EffectiveJava { xmx_mb, xms_mb, extra_args, java_path }`.

## CLI code

- `src-tauri/src/core/java.rs` — `TargetOs`, `JavaInstallation`, `JavaSource`, `parse_major_from_release`, `probe_installation` (requires `release` file with `JAVA_VERSION=`), `detect` (injectable candidates list), `default_candidates`, `ArchiveKind`, `adoptium_query_url`, `adoptium_arch`, `parse_adoptium_response`, `provision_java`, `extract_archive` → `extract_tar_gz`/`extract_zip` (traversal guard), `locate_java_bin`, `ensure_java_core` (injectable), `ensure_java`; `JavaProbe { major, version }` + `validate_java_path_core` (runs `java -version`, parses output); 43 tests in `java_tests.rs`
- `src-tauri/src/core/java_resolve.rs` — `EffectiveJava { xmx_mb: u32, xms_mb: Option<u32>, extra_args: Vec<String>, java_path: Option<PathBuf> }`; `resolve_effective_java(inst, settings)` pure; 3-tier: (1) `inst.source.recommended` (pack-recommended), (2) per-instance `inst.java` when `use_pack_settings == true`, (3) `settings.default_memory_mb`/`default_java_args` global fallback; 11 tests in `java_resolve_tests.rs`
- `src-tauri/src/core/store.rs` — `cache_java_dir`: `<data>/cache/java/`, creates on demand
- `src-tauri/src/lib.rs` — `ensure_java` + `validate_java_path` Tauri commands

## Artifacts

- `src/lib/ipc.ts` — `JavaSource`, `JavaInstallation`, `ensureJava(major)`; `JavaCfg`; `setInstanceJava`; `validateJavaPath(path) -> Promise<JavaProbe>`
- `src/routes/instance-tabs/JavaTab.tsx` — memory slider (256–32768 MB), min-memory field, JVM args textarea, java path override with file picker + `validateJavaPath` probe; `use_pack_settings` toggle; calls `setInstanceJava` on save

## Docs

- `docs/spec/java-manager.md` — Phase 2 slice C spec
- `src-tauri/src/core/fixtures/adoptium_latest.json` — fixture for `parse_adoptium_response` tests

## Coupling

- `launch_instance` calls `java_resolve::resolve_effective_java` then calls `ensure_java` only when `effective.java_path` is `None`; threads `xmx_mb`, `xms_mb`, `extra_args`, `java_path` into argv assembly.
- `provision_java` calls `download::execute_plan` + `download::DownloadPlan`.
- `settings.auto_download_java: bool` (default true) — when false, `launch_instance` aborts if no JRE is detected.

## Conventions

- `detect` is fully injectable — no env reads or filesystem side effects in tests.
- `probe_installation` requires a `release` file with `JAVA_VERSION=`. `validate_java_path_core` runs the binary (`java -version`) instead — used for user-supplied paths where no `release` file may exist.
- Adoptium OS param: `"mac"` not `"osx"`. Archive ext detection uses `ends_with`, not `Path::extension()`.
- Traversal guard: `normalize_path` resolves `..`/`.` lexically; every entry's resolved path prefix-checked before write.
- `xms_mb = None` means omit `-Xms`; callers check `Some` before emitting `-Xms<n>m`.
- `extra_args` from `split_whitespace()` — empties dropped; `None`/blank → empty `Vec`.
- `java_resolve.rs` tier 1 (`source.recommended`) activates for FTB instances (`specs.recommended`) and ATL instances (`manifest.memory > 0`).
