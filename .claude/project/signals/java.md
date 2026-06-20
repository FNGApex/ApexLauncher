# java

## What it does

JRE detection and provisioning (`java.rs`) plus per-instance Java/RAM config resolution (`java_resolve.rs`). `java.rs` detects installed system JREs or provisions Temurin from Adoptium (traversal-safe tar.gz/zip extraction); `ensure_java(major)` command returns a `JavaInstallation`. `validate_java_path(path)` probes a custom binary and returns `JavaProbe { major, version }` (D-3, ui-overhaul). `java_resolve.rs` is a pure helper — no I/O — that applies 3-tier precedence (pack-recommended → per-instance override → global settings default) to produce `EffectiveJava { xmx_mb, xms_mb, extra_args, java_path }` consumed by `launch_instance`.

## CLI code

- `src-tauri/src/core/java.rs` (752+ lines) — `TargetOs`, `JavaInstallation`, `JavaSource`, `parse_major_from_release`, `probe_installation`, `detect` (injectable candidates), `default_candidates`, `ArchiveKind`, `adoptium_query_url`, `adoptium_arch`, `parse_adoptium_response`, `provision_java`, `extract_archive` → `extract_tar_gz` / `extract_zip` (traversal guard), `locate_java_bin`, `ensure_java_core` (injectable), `ensure_java`; `JavaProbe { major: u32, version: String }` + `validate_java_path_core(path: &Path, run: impl Fn)` (D-3 addition — probes the binary by running `java -version`, parses version output); 43 unit tests in sibling `java_tests.rs`
- `src-tauri/src/core/java_resolve.rs` (106 lines) — `EffectiveJava { xmx_mb: u32, xms_mb: Option<u32>, extra_args: Vec<String>, java_path: Option<PathBuf> }`; `resolve_effective_java(inst: &Instance, settings: &Settings) -> EffectiveJava`; pure function, no I/O; 3-tier precedence: (1) `inst.source.recommended` (always `None` today — plumbing only), (2) per-instance `inst.java` when `inst.java.use_pack_settings == true`, (3) `settings.default_memory_mb` / `settings.default_java_args` global fallback; `xms_mb` from `inst.java.min_memory_mb`; `extra_args` from whitespace-splitting the resolved args string (empties dropped); `java_path` from `inst.java.path_override` when `use_pack_settings == true`; 11 unit tests in sibling `java_resolve_tests.rs`
- `src-tauri/src/core/store.rs` — `java_dir`/`cache_java_dir`: returns `<data>/cache/java/`, creates on demand
- `src-tauri/src/lib.rs` — `ensure_java` Tauri command; `validate_java_path` Tauri command (runs the binary, returns `JavaProbe`); both registered in `invoke_handler`

## Artifacts

- `src/lib/ipc.ts` — `JavaSource` type, `JavaInstallation` interface, `ensureJava(major)` wrapper; `JavaCfg` interface; `setInstanceJava(slug, java)` wrapper; `validateJavaPath(path) -> Promise<JavaProbe>` wrapper
- `src/routes/instance-tabs/JavaTab.tsx` (295 lines) — per-instance Java/RAM config form; memory slider (256–32768 MB), min-memory field; JVM args textarea; java path override with `@tauri-apps/plugin-dialog` file picker + `validateJavaPath` probe showing detected version; `use_pack_settings` toggle; reads global settings for defaults display; calls `setInstanceJava` on save

## Docs

- `docs/spec/java-manager.md` — Phase 2 slice C spec
- `docs/design/vanilla-launch.md` — design doc §C: detect-first + Temurin-download decision
- `docs/spec/ui-overhaul.md` WS-A — per-instance Java/RAM config spec (A-1 through A-4)

## Coupling

- `launch_instance` in `lib.rs` calls `java_resolve::resolve_effective_java(inst, &settings)` to get `EffectiveJava`, then calls `ensure_java` only when `effective.java_path` is `None`; threads `xmx_mb`, `xms_mb`, `extra_args`, `java_path` into argv assembly. Any change to `EffectiveJava` fields requires updating launch argv assembly in `lib.rs`.
- `instances.rs` `JavaCfg` struct is what `java_resolve` reads; `settings.rs` `Settings.default_memory_mb`/`default_java_args` are the fallback. Field renames in either require updating `java_resolve.rs`.
- `download` domain: `provision_java` calls `download::execute_plan` + `download::DownloadPlan`.
- `JavaTab` reads `getSettings` for global defaults display; actual launch uses `resolve_effective_java` on the Rust side — the frontend precedence display and Rust logic must agree.
- `validate_java_path` is called from `JavaTab.tsx` via IPC; result used only for UI feedback, not persisted.
- `settings.auto_download_java: bool` (default true) — when `false`, `launch_instance` skips `ensure_java` and aborts with an error if no JRE is detected (ui-overhaul addition).

## Conventions worth knowing

- `detect` is fully injectable (candidates list + OS + optional cache_prefix) — no env reads or filesystem side effects in tests.
- `probe_installation` requires a `release` file with `JAVA_VERSION=` — no `java -version` shell-out. If `release` is absent, the candidate is skipped.
- `JavaProbe` from `validate_java_path` is the opposite: it explicitly runs the binary to parse `java -version` output; used for user-supplied custom paths where no `release` file may exist.
- Adoptium OS param: `"mac"` not `"osx"`. Archive ext detection uses `ends_with` (`.tar.gz` / `.zip`) not `Path::extension()`.
- Traversal guard: `normalize_path` resolves `..`/`.` lexically (no `canonicalize`); every entry's resolved path prefix-checked before write.
- `java_resolve.rs` tier 1 (`source.recommended`) is always `None` currently. When providers start returning Java requirements, tier 1 wins for memory/args; tier 2 wins for custom binary path.
- `xms_mb = None` means omit `-Xms` from argv (`§8 Q5`); callers check `Some` before emitting `-Xms<n>m`.
- `extra_args` produced by `split_whitespace()` — empties dropped automatically; `None`/blank args string → empty `Vec`.
- Tests in `java_tests.rs` (43) and `java_resolve_tests.rs` (11) are siblings wired via `#[path]` stubs.
- `src-tauri/src/core/fixtures/adoptium_latest.json` — fixture for `parse_adoptium_response` tests.
