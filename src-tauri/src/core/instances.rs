//! The `instance.json` model and its create/list/get/delete operations (Phase 1).
//!
//! On-disk shape per instance (see `docs/ARCHITECTURE.md` §2–3):
//! ```text
//! instances/<slug>/
//!   instance.json        ← serialized `Instance`
//!   mc/mods/             ← the real game mods dir (reconciled on `get`)
//! ```
//! `mods[]` in the manifest tracks *managed* content; jars dropped into `mc/mods/`
//! by hand are surfaced separately by reconciling the folder on open.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::core::store;

/// Bumped when the on-disk manifest shape changes (for future migrations).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Loader {
    /// "vanilla" | "fabric" | "quilt" | "forge" | "neoforge"
    pub kind: String,
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JavaCfg {
    pub major: Option<u32>,
    pub args_override: Option<String>,
    pub memory_mb: u32,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub provider: String,
    pub project_id: String,
    pub file_id: String,
    pub pack_version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: String,
    pub hashes: BTreeMap<String, String>,
    pub enabled: bool,
    pub side: String,
    /// `true` when this entry was written by a pack importer (slice D provenance).
    /// `false` (default) for user-added mods. Old manifests missing this field
    /// deserialize as `false` — no schema bump required.
    #[serde(default)]
    pub from_pack: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub slug: String,
    pub icon: Option<String>,
    pub minecraft: String,
    pub loader: Loader,
    pub java: JavaCfg,
    pub source: Option<Source>,
    /// When `true`, mod-mutation commands (`add_mod`, `set_mod_enabled`,
    /// `remove_mod`, `update_mod`) are blocked. Set by `set_pack_lock` (slice D4).
    /// Old manifests missing this field deserialize as `false`.
    #[serde(default)]
    pub pack_locked: bool,
    pub mods: Vec<ModEntry>,
    pub created: String,
    pub last_played: Option<String>,
    pub total_playtime_sec: u64,
}

/// A jar actually present in `mc/mods/`, whether or not it's tracked in `mods[]`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderMod {
    pub file_name: String,
    /// File ends in `.disabled` (the common enable/disable convention).
    pub disabled: bool,
    pub size_bytes: u64,
    /// Matches an entry in the manifest's `mods[]` (managed vs. hand-dropped).
    pub managed: bool,
}

/// `get` result: the manifest plus the reconciled mods-folder listing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDetail {
    pub instance: Instance,
    pub folder_mods: Vec<FolderMod>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceReq {
    pub name: String,
    pub minecraft: String,
    pub loader: Loader,
}

// ---------------------------------------------------------------------------

/// List all instances, oldest first. Corrupt/unreadable manifests are skipped.
pub fn list(app: &AppHandle) -> Result<Vec<Instance>, String> {
    let dir = store::instances_dir(app)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let manifest = entry.path().join("instance.json");
        if manifest.is_file() {
            if let Ok(inst) = read_manifest(&manifest) {
                out.push(inst);
            }
        }
    }
    out.sort_by(|a, b| a.created.cmp(&b.created));
    Ok(out)
}

/// Create a fresh (empty) instance: unique slug, folder skeleton, manifest.
pub fn create(app: &AppHandle, req: CreateInstanceReq) -> Result<Instance, String> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err("Instance name cannot be empty".into());
    }
    let minecraft = req.minecraft.trim().to_string();
    if minecraft.is_empty() {
        return Err("Minecraft version is required".into());
    }

    let dir = store::instances_dir(app)?;
    let slug = unique_slug(&dir, &name);
    let inst_dir = dir.join(&slug);
    fs::create_dir_all(inst_dir.join("mc").join("mods"))
        .map_err(|e| format!("could not create instance folder: {e}"))?;

    // New instances inherit the global default memory; args use the global
    // default (no per-instance override) until the user sets one.
    let default_memory_mb = crate::core::settings::load(app)?.default_memory_mb;

    let inst = Instance {
        schema: SCHEMA_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name,
        slug,
        icon: None,
        minecraft,
        loader: req.loader,
        java: JavaCfg {
            major: None,
            args_override: None,
            memory_mb: default_memory_mb,
        },
        source: None,
        pack_locked: false,
        mods: Vec::new(),
        created: chrono::Utc::now().to_rfc3339(),
        last_played: None,
        total_playtime_sec: 0,
    };
    write_manifest(&inst_dir.join("instance.json"), &inst)?;
    Ok(inst)
}

/// Load one instance plus its reconciled mods folder.
pub fn get(app: &AppHandle, slug: &str) -> Result<InstanceDetail, String> {
    let slug = validate_slug(slug)?;
    let inst_dir = store::instances_dir(app)?.join(&slug);
    let manifest = inst_dir.join("instance.json");
    if !manifest.is_file() {
        return Err(format!("Instance '{slug}' not found"));
    }
    let instance = read_manifest(&manifest)?;
    let folder_mods = scan_mods(&inst_dir.join("mc").join("mods"), &instance);
    Ok(InstanceDetail {
        instance,
        folder_mods,
    })
}

/// Delete an instance folder and everything under it.
pub fn delete(app: &AppHandle, slug: &str) -> Result<(), String> {
    let slug = validate_slug(slug)?;
    let inst_dir = store::instances_dir(app)?.join(&slug);
    if inst_dir.exists() {
        fs::remove_dir_all(&inst_dir).map_err(|e| format!("could not delete instance: {e}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Playtime accounting
// ---------------------------------------------------------------------------

/// Increment `total_playtime_sec` by `elapsed_secs` and set `last_played` to
/// `now` (ISO-8601), then persist the instance manifest.
///
/// Both `elapsed_secs` and `now` are injectable so unit tests exercise the
/// accounting logic with a fake clock — no real JVM required.
///
/// `inst_dir` is the per-instance directory (e.g. `<instances>/<slug>/`).
pub fn record_playtime(
    inst_dir: &Path,
    elapsed_secs: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    let manifest_path = inst_dir.join("instance.json");
    let mut inst = read_manifest(&manifest_path)?;
    inst.total_playtime_sec += elapsed_secs;
    inst.last_played = Some(now.to_rfc3339());
    write_manifest(&manifest_path, &inst)
}

// ---------------------------------------------------------------------------
// Pub manifest helpers (for commands in lib.rs and sibling modules)
// ---------------------------------------------------------------------------

/// Load the `Instance` manifest for `slug` from disk.
///
/// Validates `slug` (traversal-safe) and returns `Err` if the manifest is
/// absent or malformed. Does NOT reconcile the mods folder — use [`get`] for that.
pub fn load_manifest(app: &AppHandle, slug: &str) -> Result<Instance, String> {
    let slug = validate_slug(slug)?;
    let path = store::instances_dir(app)?.join(&slug).join("instance.json");
    if !path.is_file() {
        return Err(format!("Instance '{slug}' not found"));
    }
    read_manifest(&path)
}

/// Persist the `Instance` manifest for `slug` back to disk.
///
/// Validates `slug` (traversal-safe) and overwrites `instance.json` in place.
/// The instance directory must already exist (created by [`create`]).
pub fn save_manifest(app: &AppHandle, slug: &str, inst: &Instance) -> Result<(), String> {
    let slug = validate_slug(slug)?;
    let path = store::instances_dir(app)?.join(&slug).join("instance.json");
    write_manifest(&path, inst)
}

// --- helpers ---------------------------------------------------------------

fn read_manifest(path: &Path) -> Result<Instance, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("malformed instance.json: {e}"))
}

/// Public version of `read_manifest`, exposed for tests in sibling modules
/// that need to inspect the manifest after playtime recording.
#[cfg(test)]
pub fn read_manifest_pub(path: &Path) -> Result<Instance, String> {
    read_manifest(path)
}

fn write_manifest(path: &Path, inst: &Instance) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(inst).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("could not write instance.json: {e}"))
}

fn scan_mods(mods_dir: &Path, instance: &Instance) -> Vec<FolderMod> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(mods_dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let disabled = name.to_lowercase().ends_with(".disabled");
        let base = name
            .strip_suffix(".disabled")
            .or_else(|| name.strip_suffix(".DISABLED"))
            .unwrap_or(&name)
            .to_string();
        if !base.to_lowercase().ends_with(".jar") {
            continue;
        }
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let managed = instance.mods.iter().any(|m| m.file_name == base);
        out.push(FolderMod {
            file_name: base,
            disabled,
            size_bytes,
            managed,
        });
    }
    out.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));
    out
}

/// Lowercase, alphanumerics kept, runs of anything else collapsed to a single `-`.
fn slugify(name: &str) -> String {
    let mut s = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !s.is_empty() {
            s.push('-');
            prev_dash = true;
        }
    }
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() {
        "instance".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `slugify`, then suffix `-2`, `-3`, … until the folder name is free.
fn unique_slug(dir: &Path, name: &str) -> String {
    let base = slugify(name);
    if !dir.join(&base).exists() {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Guard against path traversal: slugs we hand to the FS must be our own shape.
pub fn validate_slug(slug: &str) -> Result<String, String> {
    let ok = !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(slug.to_string())
    } else {
        Err(format!("Invalid instance slug: '{slug}'"))
    }
}

/// Guard against path traversal for mod file names crossing the IPC boundary.
///
/// A valid file name must:
/// - end in `.jar` (case-insensitive),
/// - contain no `/` or `\` (directory separators),
/// - contain no `..` component,
/// - not be an absolute path.
///
/// Returns the name unchanged on success, or an error string describing why it
/// was rejected.
pub fn validate_mod_file_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("mod file name cannot be empty".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!("mod file name contains path separator: '{name}'"));
    }
    // `:` is invalid in a file name on all target platforms; also guards against
    // Windows drive-relative paths like `C:mod.jar` which Path::join would resolve
    // relative to drive C's CWD, escaping the mods directory.
    if name.contains(':') {
        return Err(format!("mod file name contains invalid character ':': '{name}'"));
    }
    // Absolute path check (POSIX)
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(format!("mod file name must not be absolute: '{name}'"));
    }
    // `..` anywhere (already caught by separator check, but be explicit)
    if name == ".." || name.starts_with("../") || name.starts_with("..\\") {
        return Err(format!("mod file name contains traversal component: '{name}'"));
    }
    if !name.to_lowercase().ends_with(".jar") {
        return Err(format!("mod file name must end in .jar: '{name}'"));
    }
    Ok(name.to_string())
}

// ---------------------------------------------------------------------------
// Pack Lock operations (D4)
// ---------------------------------------------------------------------------

/// Pure guard: returns `Err` when `inst.pack_locked` is `true`.
///
/// Call this before any mod-mutation logic. Keeps the check pure
/// (no I/O) so it can be unit-tested without an `AppHandle`.
pub fn ensure_not_locked(inst: &Instance) -> Result<(), String> {
    if inst.pack_locked {
        Err(format!(
            "instance '{}' is pack-locked; unlock it before modifying mods",
            inst.slug
        ))
    } else {
        Ok(())
    }
}

/// Persist `pack_locked = locked` to the manifest at `manifest_path`.
///
/// Pure-path helper; call via [`set_pack_lock`] for normal use.
pub fn set_pack_lock_on_disk(manifest_path: &Path, locked: bool) -> Result<(), String> {
    let mut inst = read_manifest(manifest_path)?;
    inst.pack_locked = locked;
    write_manifest(manifest_path, &inst)
}

/// AppHandle-aware wrapper for [`set_pack_lock_on_disk`].
pub fn set_pack_lock(app: &AppHandle, slug: &str, locked: bool) -> Result<(), String> {
    let slug = validate_slug(slug)?;
    let path = store::instances_dir(app)?.join(&slug).join("instance.json");
    if !path.is_file() {
        return Err(format!("Instance '{slug}' not found"));
    }
    set_pack_lock_on_disk(&path, locked)
}

// ---------------------------------------------------------------------------
// Mod state operations
// ---------------------------------------------------------------------------

/// Toggle whether a mod file is enabled or disabled on disk + in the manifest,
/// without re-downloading.
///
/// `mods_dir` — the `mc/mods/` directory for the instance.
/// `manifest_path` — path to `instance.json`.
/// `file_name` — the **base** `.jar` name (no `.disabled` suffix).
/// `enabled` — `true` to enable, `false` to disable.
///
/// Idempotent: already in the target state → no-op success.
pub fn set_mod_enabled_on_disk(
    mods_dir: &Path,
    manifest_path: &Path,
    file_name: &str,
    enabled: bool,
) -> Result<(), String> {
    let jar_path = mods_dir.join(file_name);
    let disabled_path = mods_dir.join(format!("{file_name}.disabled"));

    if enabled {
        // Enable: rename .disabled → .jar (if the disabled form exists)
        if disabled_path.exists() {
            fs::rename(&disabled_path, &jar_path)
                .map_err(|e| format!("could not enable mod '{file_name}': {e}"))?;
        }
        // If only .jar exists (already enabled) → idempotent no-op
    } else {
        // Disable: rename .jar → .disabled (if the enabled form exists)
        if jar_path.exists() {
            fs::rename(&jar_path, &disabled_path)
                .map_err(|e| format!("could not disable mod '{file_name}': {e}"))?;
        }
        // If only .disabled exists (already disabled) → idempotent no-op
    }

    // Flip the flag in the manifest (match by file_name == base jar name).
    let mut inst = read_manifest(manifest_path)?;
    for entry in inst.mods.iter_mut() {
        if entry.file_name == file_name {
            entry.enabled = enabled;
        }
    }
    write_manifest(manifest_path, &inst)
}

/// AppHandle-aware wrapper for [`set_mod_enabled_on_disk`].
pub fn set_mod_enabled(
    app: &AppHandle,
    slug: &str,
    file_name: &str,
    enabled: bool,
) -> Result<(), String> {
    let slug = validate_slug(slug)?;
    let file_name = validate_mod_file_name(file_name)?;
    let inst_dir = store::instances_dir(app)?.join(&slug);
    let mods_dir = inst_dir.join("mc").join("mods");
    let manifest_path = inst_dir.join("instance.json");
    set_mod_enabled_on_disk(&mods_dir, &manifest_path, &file_name, enabled)
}

/// Delete the on-disk file (enabled or disabled form) for a mod and drop its
/// `ModEntry` from the manifest.
///
/// Missing file is not an error — the manifest entry is still dropped so the
/// system converges to "gone".
///
/// `mods_dir` — the `mc/mods/` directory for the instance.
/// `manifest_path` — path to `instance.json`.
/// `file_name` — the **base** `.jar` name (no `.disabled` suffix).
pub fn remove_mod_from_disk(
    mods_dir: &Path,
    manifest_path: &Path,
    file_name: &str,
) -> Result<(), String> {
    let jar_path = mods_dir.join(file_name);
    let disabled_path = mods_dir.join(format!("{file_name}.disabled"));

    if jar_path.exists() {
        fs::remove_file(&jar_path)
            .map_err(|e| format!("could not remove mod file '{file_name}': {e}"))?;
    }
    if disabled_path.exists() {
        fs::remove_file(&disabled_path)
            .map_err(|e| format!("could not remove disabled mod file: {e}"))?;
    }

    // Drop the matching ModEntry (by file_name) from the manifest.
    let mut inst = read_manifest(manifest_path)?;
    inst.mods.retain(|m| m.file_name != file_name);
    write_manifest(manifest_path, &inst)
}

/// Delete the on-disk file(s) for a mod (both `.jar` and `.jar.disabled` forms)
/// WITHOUT touching the manifest. Used by `update_mod` in `lib.rs` which manages
/// the manifest entry separately via `apply_swap`.
///
/// Silently ignores missing files (best-effort cleanup).
pub fn remove_mod_from_disk_files(mods_dir: &Path, file_name: &str) {
    let jar_path = mods_dir.join(file_name);
    let disabled_path = mods_dir.join(format!("{file_name}.disabled"));
    if jar_path.exists() {
        let _ = fs::remove_file(&jar_path);
    }
    if disabled_path.exists() {
        let _ = fs::remove_file(&disabled_path);
    }
}

/// AppHandle-aware wrapper for [`remove_mod_from_disk`].
pub fn remove_mod(app: &AppHandle, slug: &str, file_name: &str) -> Result<(), String> {
    let slug = validate_slug(slug)?;
    let file_name = validate_mod_file_name(file_name)?;
    let inst_dir = store::instances_dir(app)?.join(&slug);
    let mods_dir = inst_dir.join("mc").join("mods");
    let manifest_path = inst_dir.join("instance.json");
    remove_mod_from_disk(&mods_dir, &manifest_path, &file_name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "instances_tests.rs"]
mod tests;
