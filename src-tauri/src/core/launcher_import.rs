//! Prism / MultiMC / PolyMC launcher instance importer — pure parse/plan layer.
//!
//! All functions in this module are **pure** (no I/O, no Tauri, no network) except
//! for [`copy_game_dir`] (CP-3, performs filesystem writes into a staging dir) and
//! [`resolve_icon_path`] (CP-4, filesystem stat only).
//! Higher-level orchestration (stage+promote, Tauri job) lives in `lib.rs` (CP-5+).
//!
//! # Checkpoints implemented here
//! - **CP-1** — `instance.cfg` parser → [`PrismInstanceCfg`].
//! - **CP-2** — `mmc-pack.json` parser + uid→loader mapping → [`MmcPack`].
//! - **CP-3** — Game-dir resolution ([`resolve_game_dir`]) + safe recursive copy
//!   ([`copy_game_dir`]).
//! - **CP-4** — Icon resolution helper ([`resolve_icon_path`]).

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors produced by the launcher-import parse + copy pipeline.
#[derive(Debug, thiserror::Error)]
pub enum LauncherImportError {
    /// A required field is absent (e.g. `net.minecraft` component missing from `mmc-pack.json`).
    #[error("missing required field: {0}")]
    MissingField(String),

    /// The `mmc-pack.json` content is not valid JSON or is structurally invalid.
    #[error("malformed mmc-pack.json: {0}")]
    MalformedMmcPack(String),

    /// A key in `instance.cfg` carries a value that cannot be parsed to the expected type.
    #[error("malformed field '{field}' in instance.cfg: {reason}")]
    MalformedField { field: String, reason: String },

    /// Neither `.minecraft/` nor `minecraft/` exists under the instance directory.
    #[error("no game directory found under {path}")]
    NoGameDir { path: String },

    /// A path in the source tree would escape the destination root (e.g. a symlink).
    /// Symlinks are skipped silently; this variant is for other structural violations.
    #[error("unsafe path rejected: {0}")]
    UnsafePath(String),

    /// The instance configuration is explicitly unsupported (e.g. `InstanceType=Legacy`).
    /// The message string is the user-facing reason.
    #[error("{0}")]
    Rejected(String),

    /// I/O error during game-dir copy.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── CP-1: instance.cfg → PrismInstanceCfg ────────────────────────────────────

/// Fields parsed from Prism/MultiMC/PolyMC `instance.cfg`.
///
/// All fields are `Option<T>` or bool-defaulting-false. The parser captures
/// whatever is present; the import job (CP-5) applies the `Override*` gates.
#[derive(Debug, Default)]
pub struct PrismInstanceCfg {
    /// Display name of the instance (`name` key).
    pub name: Option<String>,
    /// Icon identifier: built-in theme name or custom-file stem (`iconKey` key).
    pub icon_key: Option<String>,
    /// Raw `InstanceType` value. `"OneSix"` = modern; `"Legacy"` = pre-1.6 (job rejects it).
    pub instance_type: Option<String>,
    /// Gate for memory overrides (`OverrideMemory`).
    pub override_memory: bool,
    /// `-Xms` MiB (`MinMemAlloc`). Meaningful only when `override_memory` is true.
    pub min_mem_mb: Option<u32>,
    /// `-Xmx` MiB (`MaxMemAlloc`). Meaningful only when `override_memory` is true.
    pub max_mem_mb: Option<u32>,
    /// Gate for Java path override (`OverrideJavaLocation`).
    pub override_java_location: bool,
    /// Absolute path to the `java`/`javaw` binary (`JavaPath`). Meaningful only when gate is true.
    pub java_path: Option<String>,
    /// Gate for JVM args override (`OverrideJavaArgs`).
    pub override_java_args: bool,
    /// Extra JVM arguments (`JvmArgs`). Meaningful only when gate is true.
    pub jvm_args: Option<String>,
}

/// Parse the text content of a Prism/MultiMC/PolyMC `instance.cfg` file.
///
/// The format is a flat `key=value` file (INI-ish). An optional `[General]`
/// section header is tolerated. Unknown keys are silently ignored. Bool values
/// are `true`/`false` (case-insensitive). The parser does NOT reject `Legacy`
/// instances — that check is left to the job layer (CP-5, field-mapping step 1).
pub fn parse_instance_cfg(text: &str) -> Result<PrismInstanceCfg, LauncherImportError> {
    let mut cfg = PrismInstanceCfg::default();

    for raw_line in text.lines() {
        let line = raw_line.trim();

        // Skip blank lines, section headers (`[…]`), and comment markers (`#`/`;`).
        if line.is_empty()
            || line.starts_with('[')
            || line.starts_with('#')
            || line.starts_with(';')
        {
            continue;
        }

        // Split on the FIRST `=` so values that themselves contain `=`
        // (e.g. `-Dproperty=value` inside JvmArgs) are captured whole.
        let (key, value) = match line.split_once('=') {
            Some(pair) => pair,
            None => continue, // malformed line — skip silently
        };

        let key = key.trim();
        let value = value.trim();

        match key {
            "name" => cfg.name = Some(value.to_string()),
            "iconKey" => cfg.icon_key = Some(value.to_string()),
            "InstanceType" => cfg.instance_type = Some(value.to_string()),
            "OverrideMemory" => cfg.override_memory = parse_bool(value),
            "MinMemAlloc" => cfg.min_mem_mb = Some(parse_u32("MinMemAlloc", value)?),
            "MaxMemAlloc" => cfg.max_mem_mb = Some(parse_u32("MaxMemAlloc", value)?),
            "OverrideJavaLocation" => cfg.override_java_location = parse_bool(value),
            "JavaPath" => cfg.java_path = Some(value.to_string()),
            "OverrideJavaArgs" => cfg.override_java_args = parse_bool(value),
            "JvmArgs" => cfg.jvm_args = Some(value.to_string()),
            // All other keys (notes, window size, perf toggles, env, etc.) are silently ignored.
            _ => {}
        }
    }

    Ok(cfg)
}

/// Parse `"true"` (case-insensitive) as `true`; anything else as `false`.
fn parse_bool(val: &str) -> bool {
    val.trim().eq_ignore_ascii_case("true")
}

/// Parse a string as `u32`, returning a [`LauncherImportError::MalformedField`] on failure.
fn parse_u32(field: &str, val: &str) -> Result<u32, LauncherImportError> {
    val.trim().parse::<u32>().map_err(|_| LauncherImportError::MalformedField {
        field: field.to_string(),
        reason: format!("expected unsigned integer, got '{}'", val.trim()),
    })
}

// ── CP-2: mmc-pack.json → MmcPack ────────────────────────────────────────────

/// The resolved loader from an `mmc-pack.json` component list.
#[derive(Debug, PartialEq)]
pub enum ImportedLoader {
    /// No loader component was present — this is a vanilla Minecraft instance.
    Vanilla,
    /// A recognized mod loader (`fabric`, `quilt`, `forge`, `neoforge`).
    Loader { kind: String, version: String },
    /// A loader uid that ApexLauncher cannot install (e.g. `liteloader`).
    /// The import job records a warning and proceeds as vanilla.
    Unsupported(String),
}

/// The result of parsing an `mmc-pack.json` file.
#[derive(Debug)]
pub struct MmcPack {
    /// The Minecraft version string (`net.minecraft` component `version`).
    pub minecraft: String,
    /// The resolved mod loader (or [`ImportedLoader::Vanilla`] if none).
    pub loader: ImportedLoader,
}

// ── CP-2: raw serde types ─────────────────────────────────────────────────────

/// One component entry in the `components[]` array of `mmc-pack.json`.
#[derive(Debug, Deserialize)]
struct MmcComponent {
    uid: String,
    /// Authoritative version string (NOT `cachedVersion`).
    #[serde(default)]
    version: String,
    /// Auto-pulled dependency (e.g. Fabric Intermediary). Skip entirely.
    #[serde(default, rename = "dependencyOnly")]
    dependency_only: bool,
}

/// Top-level `mmc-pack.json` shape.
#[derive(Debug, Deserialize)]
struct RawMmcPack {
    #[serde(default)]
    components: Vec<MmcComponent>,
}

/// Parse the text content of an `mmc-pack.json` file.
///
/// ## uid → loader mapping (from Prism `meta-launcher/index.json`)
///
/// | uid | ApexLauncher kind |
/// |-----|-------------------|
/// | `net.fabricmc.fabric-loader` | `"fabric"` |
/// | `org.quiltmc.quilt-loader` | `"quilt"` |
/// | `net.minecraftforge` | `"forge"` (bare build number, e.g. `47.2.0`) |
/// | `net.neoforged` | `"neoforge"` (bare build number, e.g. `21.1.209`) |
/// | `com.mumfrey.liteloader` | [`ImportedLoader::Unsupported`]`("liteloader")` |
///
/// Ignored uids (never a loader): `net.fabricmc.intermediary`, `org.lwjgl`,
/// `org.lwjgl3`, any uid ending in `.java` (Java runtime components), and any
/// component with `dependencyOnly: true`.
///
/// If no loader component is found → [`ImportedLoader::Vanilla`].
///
/// When multiple loader uids appear (unusual), the **first** one in the array wins.
///
/// **Forge legacy edge:** ancient Forge (1.7.10) uses a doubled `mc-build-mc` version
/// form (e.g. `1.7.10-10.13.4.1614-1.7.10`) identifiable by the presence of `-` in
/// the version string. Modern Forge (`47.2.0`) and NeoForge (`21.1.209`) use
/// dots-only bare build numbers. The legacy form maps to
/// [`ImportedLoader::Unsupported`]`("forge-legacy")` as it is out of scope for v1.
pub fn parse_mmc_pack(text: &str) -> Result<MmcPack, LauncherImportError> {
    let raw: RawMmcPack = serde_json::from_str(text)
        .map_err(|e| LauncherImportError::MalformedMmcPack(e.to_string()))?;

    let mut minecraft: Option<String> = None;
    let mut loader: Option<ImportedLoader> = None;

    for comp in &raw.components {
        // dependencyOnly components (e.g. Fabric Intermediary) are auto-pulled
        // dependencies — they are never the authoritative loader.
        if comp.dependency_only {
            continue;
        }

        match comp.uid.as_str() {
            // Sets Instance.minecraft; not a loader.
            "net.minecraft" => {
                minecraft = Some(comp.version.clone());
            }

            // Vanilla substrate — skip by uid (may or may not carry dependencyOnly).
            "net.fabricmc.intermediary" | "org.lwjgl" | "org.lwjgl3" => {}

            // Recognized loader uids — first match wins.
            "net.fabricmc.fabric-loader" => {
                if loader.is_none() {
                    loader = Some(ImportedLoader::Loader {
                        kind: "fabric".to_string(),
                        version: comp.version.clone(),
                    });
                }
            }
            "org.quiltmc.quilt-loader" => {
                if loader.is_none() {
                    loader = Some(ImportedLoader::Loader {
                        kind: "quilt".to_string(),
                        version: comp.version.clone(),
                    });
                }
            }
            "net.minecraftforge" => {
                if loader.is_none() {
                    // Ancient 1.7.10 Forge uses a doubled "mc-build-mc" version form
                    // (contains `-`). Modern Forge bare build numbers are dots-only.
                    if comp.version.contains('-') {
                        loader =
                            Some(ImportedLoader::Unsupported("forge-legacy".to_string()));
                    } else {
                        loader = Some(ImportedLoader::Loader {
                            kind: "forge".to_string(),
                            version: comp.version.clone(),
                        });
                    }
                }
            }
            "net.neoforged" => {
                if loader.is_none() {
                    // Unlike Forge, NeoForge versions legitimately carry a `-beta`/
                    // `-alpha` suffix (e.g. `21.1.209-beta`), and that IS the real
                    // maven artifact (`neoforge-21.1.209-beta.jar`). So we deliberately
                    // do NOT treat `-` as a legacy marker here — pass the version through
                    // verbatim. (Contrast the `net.minecraftforge` arm above.)
                    loader = Some(ImportedLoader::Loader {
                        kind: "neoforge".to_string(),
                        version: comp.version.clone(),
                    });
                }
            }
            "com.mumfrey.liteloader" => {
                if loader.is_none() {
                    loader = Some(ImportedLoader::Unsupported("liteloader".to_string()));
                }
            }

            // Java runtime components — ApexLauncher manages its own Java.
            // Covers: net.adoptium.java, net.minecraft.java, com.azul.java, com.ibm.java, …
            uid if uid.ends_with(".java") => {}

            // Unknown uids: skip silently (forward-compatible with new Prism components).
            _ => {}
        }
    }

    let minecraft = minecraft
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LauncherImportError::MissingField("net.minecraft".to_string()))?;

    Ok(MmcPack {
        minecraft,
        loader: loader.unwrap_or(ImportedLoader::Vanilla),
    })
}

// ── CP-3: Game-dir resolution + safe recursive copy ───────────────────────────

/// Resolve the Prism/MultiMC game directory inside an instance directory.
///
/// Checks for `.minecraft/` first (canonical Prism layout), then falls back to
/// `minecraft/` (some older MultiMC / PolyMC exports).
///
/// # Errors
/// Returns [`LauncherImportError::NoGameDir`] if neither subdirectory exists.
pub fn resolve_game_dir(instance_dir: &Path) -> Result<PathBuf, LauncherImportError> {
    let dot_mc = instance_dir.join(".minecraft");
    if dot_mc.is_dir() {
        return Ok(dot_mc);
    }
    let mc = instance_dir.join("minecraft");
    if mc.is_dir() {
        return Ok(mc);
    }
    Err(LauncherImportError::NoGameDir { path: instance_dir.display().to_string() })
}

/// Recursively copy a game directory tree from `src` into `dest`.
///
/// ## Path safety
/// All destination paths are validated to remain within `dest`:
/// - **Symlinks are skipped entirely** (whether to files or directories). A symlink
///   target could point anywhere on the filesystem; skipping is the safe default.
/// - Path component validation (local equivalent of `validate_relative_path` and
///   `is_safe_dest` in `core/modpack.rs`) rejects any `..`, absolute, or
///   prefix-rooted components before the write.
///
/// ## Skip behaviour
/// When `skip_logs` is `true`, the top-level `logs/` and `crash-reports/`
/// directories are omitted entirely (files inside them are not counted).
///
/// ## Returns
/// The number of regular files written to `dest`.
///
/// # Errors
/// Returns [`LauncherImportError::Io`] on any I/O failure, or
/// [`LauncherImportError::UnsafePath`] if a structural path violation is detected.
pub fn copy_game_dir(src: &Path, dest: &Path, skip_logs: bool) -> Result<u32, LauncherImportError> {
    // Canonicalize the source root once so that strip_prefix and containment
    // decisions are relative to the REAL directory on disk. Following the root
    // itself through a symlink or junction is intentional (the user pointed us
    // at it), but canonicalizing ensures all recursive paths are consistent with
    // the real layout. Nested reparse points inside the tree are skipped by
    // copy_dir_recursive via is_reparse_or_symlink (FIX 2).
    let src_real = src.canonicalize()?;
    std::fs::create_dir_all(dest)?;
    let mut count = 0u32;
    copy_dir_recursive(&src_real, &src_real, dest, skip_logs, &mut count)?;
    Ok(count)
}

/// Inner recursive worker for [`copy_game_dir`].
///
/// `src_root` is fixed across all recursive calls (used to compute relative
/// paths). `current` advances into each subdirectory.
fn copy_dir_recursive(
    src_root: &Path,
    current: &Path,
    dest_root: &Path,
    skip_logs: bool,
    count: &mut u32,
) -> Result<(), LauncherImportError> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        // Use symlink_metadata (not metadata) so we inspect the entry itself,
        // not the target it points to. This is the prerequisite for is_reparse_or_symlink.
        let meta = path.symlink_metadata()?;
        let file_type = meta.file_type();

        // Skip symlinks AND Windows reparse points (directory junctions).
        // file_type.is_symlink() alone misses junctions on Windows — those carry
        // FILE_ATTRIBUTE_REPARSE_POINT but report is_symlink() == false.
        // is_reparse_or_symlink checks the attribute bit on Windows as well.
        if is_reparse_or_symlink(&meta) {
            continue;
        }

        // Compute relative path from src_root for safety checks and dest construction.
        let rel = match path.strip_prefix(src_root) {
            Ok(r) => r,
            Err(_) => {
                return Err(LauncherImportError::UnsafePath(path.display().to_string()));
            }
        };

        // Path-safety: reject any relative path whose components could escape dest_root.
        // Mirrors validate_relative_path + is_safe_dest in core/modpack.rs (private there).
        if !relative_path_is_safe(rel) {
            return Err(LauncherImportError::UnsafePath(rel.display().to_string()));
        }

        // Skip top-level logs/ and crash-reports/ when requested.
        // `rel.components().count() == 1` → entry is directly under src_root.
        if skip_logs && file_type.is_dir() && rel.components().count() == 1 {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "logs" || name == "crash-reports" {
                continue;
            }
        }

        let dest_path = dest_root.join(rel);

        // Structural containment check on the final joined path.
        if !dest_is_contained(&dest_path, dest_root) {
            return Err(LauncherImportError::UnsafePath(rel.display().to_string()));
        }

        if file_type.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir_recursive(src_root, &path, dest_root, skip_logs, count)?;
        } else if file_type.is_file() {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&path, &dest_path)?;
            *count += 1;
        }
        // Non-file non-dir non-symlink entries (devices, sockets) are silently skipped.
    }
    Ok(())
}

/// Return `true` if all components of `rel` are safe (no `..`, no root, no prefix).
///
/// Mirrors `validate_relative_path` in `core/modpack.rs` exactly, including the
/// string-level checks. The string checks are needed because `Path` parsing is
/// platform-specific: `"C:foo"` is `Component::Normal` on Linux (not `Prefix`),
/// and `"\\server\share"` is `Normal` on Linux (not `Prefix`). String guards
/// catch these before the component walk.
fn relative_path_is_safe(rel: &Path) -> bool {
    // String-level guards (platform-independent, matches modpack::validate_relative_path).
    let s = rel.to_str().unwrap_or("");
    if s.starts_with('/') || s.starts_with('\\') {
        return false;
    }
    // Windows drive-letter prefix: "C:foo" or "C:\..." — b[1] == ':' with alpha b[0].
    if s.len() >= 2 {
        let b = s.as_bytes();
        if b[1] == b':' && b[0].is_ascii_alphabetic() {
            return false;
        }
    }
    // Component-level guards.
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            Component::Normal(_) | Component::CurDir => {}
        }
    }
    true
}

/// Return `true` if the metadata (obtained via `symlink_metadata`) indicates a
/// symlink or Windows reparse point (e.g. a directory junction).
///
/// **Why not `file_type.is_symlink()` alone?**
/// On Windows, directory junctions carry the `FILE_ATTRIBUTE_REPARSE_POINT`
/// attribute (0x400) but `FileType::is_symlink()` returns `false` for them
/// (only symbolic links proper set that bit in the file-type sense). Junctions
/// can redirect a directory walk to an arbitrary location — skipping them is
/// therefore mandatory for the same reason we skip symlinks.
///
/// The caller must pass metadata obtained with `symlink_metadata` (not `metadata`),
/// which inspects the entry itself rather than following any link.
///
/// Junction creation requires elevated rights / Developer Mode, so we cannot
/// exercise the Windows reparse branch in a hermetic unit test. The negative
/// case (regular files/dirs are NOT flagged) is tested in the test suite.
fn is_reparse_or_symlink(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Return `true` if `dest` resolves strictly within `base` with no escape.
///
/// Local equivalent of `is_safe_dest` in `core/modpack.rs` (private there).
fn dest_is_contained(dest: &Path, base: &Path) -> bool {
    let rel = dest.strip_prefix(base).unwrap_or(dest);
    let mut resolved = base.to_path_buf();
    for component in rel.components() {
        match component {
            Component::ParentDir => return false,
            Component::Normal(s) => resolved.push(s),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    resolved.starts_with(base)
}

// ── CP-4: Icon resolution helper ──────────────────────────────────────────────

/// Allowed extensions for Prism instance icon files.
///
/// Matches the allowlist in `core/instances::write_instance_icon`.
/// Order determines priority when multiple files exist with the same stem.
const ICON_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// Return `true` if `key` is a safe icon-file stem: non-empty, single-component,
/// no path separators (`/` or `\`), no `..`, not absolute, not a Windows
/// drive-letter prefix (`C:…`).
///
/// `icon_key` comes from the untrusted `instance.cfg`. A bare stem like `"myicon"`
/// or `"flames"` is accepted; anything that could traverse out of the `icons/`
/// directory is rejected (return `None` from [`resolve_icon_path`]).
fn icon_key_is_safe(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    // Reject any path separator — the key must be a bare filename stem.
    if key.contains('/') || key.contains('\\') {
        return false;
    }
    // Windows drive-letter prefix: "C:foo" or "C:\..." etc.
    if key.len() >= 2 {
        let b = key.as_bytes();
        if b[1] == b':' && b[0].is_ascii_alphabetic() {
            return false;
        }
    }
    // Path-level check: must parse as exactly one Normal component.
    // This catches `..`, `.` (CurDir), RootDir, and any Prefix.
    let mut components = Path::new(key).components();
    match components.next() {
        Some(Component::Normal(_)) => {}
        _ => return false,
    }
    // No second component allowed (guards exotic encodings not caught above).
    if components.next().is_some() {
        return false;
    }
    true
}

/// Resolve a Prism `iconKey` to an icon file in the launcher's central `icons/` directory.
///
/// Prism/MultiMC stores custom icons in `<dataRoot>/icons/<key>.<ext>`.
/// The data root is inferred by going two levels up from `instance_dir`
/// (`<dataRoot>/instances/<name>/` → `<dataRoot>/`).
///
/// Returns the path to the first matching file (in [`ICON_EXTS`] order), or
/// `None` when:
/// - `icon_key` fails the safety check (contains `/`, `\`, `..`, drive letter, etc.).
/// - The icon key corresponds to a built-in theme icon (no file on disk).
/// - The `icons/` directory does not exist.
/// - No file with an allowed extension is found.
/// - The parent chain cannot be resolved (missing ancestors).
pub fn resolve_icon_path(instance_dir: &Path, icon_key: &str) -> Option<PathBuf> {
    // Reject any icon_key that could escape the icons/ directory (FIX 1 — BLOCKER).
    // icon_key comes from untrusted instance.cfg: "../../../etc/passwd", "/abs", "C:\x",
    // "a/b" etc. must all be rejected before they are joined into a filesystem path.
    if !icon_key_is_safe(icon_key) {
        return None;
    }
    // instance_dir is <dataRoot>/instances/<name>/ — data root is two levels up.
    let data_root = instance_dir.parent()?.parent()?;
    let icons_dir = data_root.join("icons");
    for ext in ICON_EXTS {
        let candidate = icons_dir.join(format!("{}.{}", icon_key, ext));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

// ── CP-5: Plan function (pure) ────────────────────────────────────────────────

/// The resolved plan for importing a Prism/MultiMC/PolyMC instance (output of
/// [`plan_external_import`]).
///
/// Contains everything the orchestrator (`ImportExternalJob` in `lib.rs`) needs to
/// create the ApexLauncher instance, apply Java/memory overrides, and copy the
/// game directory tree. No I/O is performed by the plan function itself.
#[derive(Debug)]
pub struct ExternalImportPlan {
    /// Resolved instance name (`name_override ?? cfg.name ?? "Imported"`).
    pub name: String,
    /// Minecraft version string from `mmc-pack.json`.
    pub minecraft: String,
    /// Loader kind: `"vanilla"`, `"fabric"`, `"quilt"`, `"forge"`, or `"neoforge"`.
    pub loader_kind: String,
    /// Loader version (e.g. `"0.16.9"` for Fabric). `None` for vanilla.
    pub loader_version: Option<String>,
    /// When `true`, the orchestrator must apply `max_mem_mb`/`min_mem_mb` to `JavaCfg`.
    pub override_memory: bool,
    /// Max heap MiB (`-Xmx`). Meaningful only when `override_memory` is `true`.
    pub max_mem_mb: Option<u32>,
    /// Min heap MiB (`-Xms`). Meaningful only when `override_memory` is `true`.
    pub min_mem_mb: Option<u32>,
    /// When `true`, the orchestrator must apply `java_path` to `JavaCfg.path_override`.
    pub override_java_location: bool,
    /// Absolute path to a Java binary. Meaningful only when `override_java_location` is `true`.
    pub java_path: Option<String>,
    /// When `true`, the orchestrator must apply `jvm_args` to `JavaCfg.args_override`.
    pub override_java_args: bool,
    /// Extra JVM arguments. Meaningful only when `override_java_args` is `true`.
    pub jvm_args: Option<String>,
    /// Non-fatal warnings accumulated during planning (e.g. unsupported loader mapped to vanilla).
    pub warnings: Vec<String>,
}

/// Map parsed `PrismInstanceCfg` + `MmcPack` into an [`ExternalImportPlan`].
///
/// Implements the field-mapping contract from the spec (steps 1–6):
///
/// 1. Reject `instance_type == "Legacy"` with [`LauncherImportError::Rejected`].
/// 2. Name: `name_override ?? cfg.name ?? "Imported"`.
/// 3. Minecraft version from `pack.minecraft`.
/// 4. Loader:
///    - [`ImportedLoader::Vanilla`] → `loader_kind = "vanilla"`, `loader_version = None`.
///    - [`ImportedLoader::Loader`] → pass kind + version through unchanged.
///    - [`ImportedLoader::Unsupported(x)`] → `loader_kind = "vanilla"`, `loader_version = None`,
///      push a warning ("loader '{x}' is not supported …").
/// 5. Java/memory overrides are threaded through the plan verbatim; the orchestrator
///    applies them only when the corresponding `override_*` gate is `true` (step 6).
///
/// # Errors
/// Returns [`LauncherImportError::Rejected`] for Legacy (pre-1.6) instances, which
/// cannot be imported (they lack a `mmc-pack.json` component list).
pub fn plan_external_import(
    cfg: &PrismInstanceCfg,
    pack: &MmcPack,
    name_override: Option<&str>,
) -> Result<ExternalImportPlan, LauncherImportError> {
    // Step 1: reject Legacy instances (field-mapping step 1).
    if cfg.instance_type.as_deref() == Some("Legacy") {
        return Err(LauncherImportError::Rejected(
            "legacy MultiMC instances are not supported (pre-1.6 format)".to_string(),
        ));
    }

    // Step 2: resolve name (override ?? cfg.name ?? "Imported").
    let name = name_override
        .filter(|s| !s.is_empty())
        .or(cfg.name.as_deref())
        .unwrap_or("Imported")
        .to_string();

    // Steps 3–4: resolve loader from the parsed MmcPack.
    let mut warnings = Vec::new();
    let (loader_kind, loader_version) = match &pack.loader {
        ImportedLoader::Vanilla => ("vanilla".to_string(), None),
        ImportedLoader::Loader { kind, version } => (kind.clone(), Some(version.clone())),
        ImportedLoader::Unsupported(uid) => {
            warnings.push(format!(
                "loader '{}' is not supported by ApexLauncher; imported as vanilla",
                uid
            ));
            ("vanilla".to_string(), None)
        }
    };

    // Step 5 / step 6: thread Override* gates + values through unchanged.
    Ok(ExternalImportPlan {
        name,
        minecraft: pack.minecraft.clone(),
        loader_kind,
        loader_version,
        override_memory: cfg.override_memory,
        max_mem_mb: cfg.max_mem_mb,
        min_mem_mb: cfg.min_mem_mb,
        override_java_location: cfg.override_java_location,
        java_path: cfg.java_path.clone(),
        override_java_args: cfg.override_java_args,
        jvm_args: cfg.jvm_args.clone(),
        warnings,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "launcher_import_tests.rs"]
mod tests;
