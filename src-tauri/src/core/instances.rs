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

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: String,
    pub hashes: BTreeMap<String, String>,
    pub enabled: bool,
    pub side: String,
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

// --- helpers ---------------------------------------------------------------

fn read_manifest(path: &Path) -> Result<Instance, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("malformed instance.json: {e}"))
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
fn validate_slug(slug: &str) -> Result<String, String> {
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
