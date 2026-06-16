//! Modrinth `.mrpack` modpack import — pure parse, plan, and overrides extraction.
//!
//! This module is I/O-free except for [`extract_overrides`], which reads from a
//! provided [`zip::ZipArchive`] and writes to a caller-supplied directory.
//! The [`import_mrpack`] Tauri command (CP4) wires these pieces together.
//!
//! # Checkpoints implemented here
//! - **CP1** — Manifest model ([`MrpackManifest`], [`MrpackFile`], [`PackLoader`]) +
//!   [`parse_modrinth_index`].
//! - **CP2** — Pure plan builder: [`build_pack_plan`] → [`PackPlan`].
//! - **CP3** — Zip-slip-safe overrides extraction: [`extract_overrides`].
//! - **CP4** — In-memory zip reader: [`read_mrpack`] (testable seam used by `import_mrpack`).

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::core::download::{DownloadItem, ExpectedHash};
use crate::core::instances::ModEntry;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors produced by the modpack parse/plan/extract pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ModpackError {
    /// The `modrinth.index.json` content is not valid JSON or is missing required fields.
    #[error("malformed modrinth.index.json: {0}")]
    MalformedManifest(String),

    /// A file in `files[]` carries no usable hash (`sha512` or `sha1`).
    #[error("file '{0}' has no sha512 or sha1 hash — refusing to download unverified")]
    MissingHash(String),

    /// A file in `files[]` has an empty `downloads` list.
    #[error("file '{0}' has no download URLs")]
    NoDownloadUrls(String),

    /// A `downloads` URL's host is not on the trusted allowlist.
    #[error("disallowed download host '{host}' in file '{path}' — import aborted")]
    DisallowedHost { host: String, path: String },

    /// A `files[].path` or override entry escapes the instance directory.
    #[error("path '{0}' is unsafe (contains '..', is absolute, or has a drive prefix)")]
    UnsafePath(String),

    /// A zip-slip attempt was detected in an override entry.
    #[error("override entry '{0}' would escape the target directory (zip-slip)")]
    ZipSlip(String),

    /// The `modrinth.index.json` entry was not found inside the archive.
    #[error("modrinth.index.json not found in archive")]
    IndexNotFound,

    /// An I/O error occurred during overrides extraction.
    #[error("I/O error during overrides extraction: {0}")]
    Io(#[from] io::Error),

    /// An error from the zip crate.
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

// ── CP1: Manifest model ───────────────────────────────────────────────────────

/// A loader declared by the pack (mapped from the raw dependency key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackLoader {
    /// Normalized loader kind: `"vanilla"` | `"fabric"` | `"quilt"` | `"forge"` | `"neoforge"`.
    pub kind: String,
    /// Loader version string declared in `dependencies`, or `None` for vanilla.
    pub version: Option<String>,
}

/// Per-file environment declaration from `modrinth.index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEnv {
    /// `"required"` | `"optional"` | `"unsupported"`.
    pub client: String,
    /// `"required"` | `"optional"` | `"unsupported"`.
    pub server: String,
}

/// A single file entry from `modrinth.index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrpackFile {
    /// Relative path inside the instance `mc/` directory (e.g. `"mods/sodium.jar"`).
    pub path: String,
    /// Hashes keyed by algorithm (`"sha1"`, `"sha512"`).
    pub hashes: BTreeMap<String, String>,
    /// Environment declaration; `None` means client-supported.
    pub env: Option<FileEnv>,
    /// Download URLs (at least one expected by the format).
    pub downloads: Vec<String>,
    /// File size in bytes.
    pub file_size: u64,
}

impl MrpackFile {
    /// Returns `true` if this file should be installed on the client.
    ///
    /// A file with `env.client == "unsupported"` is skipped; absent `env` means supported.
    pub fn client_supported(&self) -> bool {
        match &self.env {
            None => true,
            Some(e) => e.client != "unsupported",
        }
    }

    /// Returns the normalized side string for a `ModEntry` (`"client"`, `"server"`, or `"both"`).
    pub fn side(&self) -> String {
        match &self.env {
            None => "both".to_string(),
            Some(e) => match (e.client.as_str(), e.server.as_str()) {
                ("unsupported", _) => "server".to_string(),
                (_, "unsupported") => "client".to_string(),
                _ => "both".to_string(),
            },
        }
    }
}

/// The parsed contents of `modrinth.index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrpackManifest {
    /// Pack display name.
    pub name: String,
    /// Pack version id (semver or arbitrary string).
    pub version_id: String,
    /// Optional human summary.
    pub summary: Option<String>,
    /// Minecraft version string (from `dependencies.minecraft`).
    pub minecraft: String,
    /// Resolved loader for this pack.
    pub loader: PackLoader,
    /// All files declared in the pack index.
    pub files: Vec<MrpackFile>,
}

// Raw deserialization shape — matches the on-disk JSON exactly.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    name: String,
    version_id: String,
    summary: Option<String>,
    dependencies: serde_json::Map<String, serde_json::Value>,
    files: Vec<RawFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    path: String,
    hashes: BTreeMap<String, String>,
    env: Option<RawEnv>,
    downloads: Vec<String>,
    file_size: u64,
}

#[derive(Deserialize)]
struct RawEnv {
    client: String,
    server: String,
}

/// Parse a `modrinth.index.json` JSON string into a [`MrpackManifest`].
///
/// # Errors
/// - [`ModpackError::MalformedManifest`] if the JSON is invalid or missing required fields.
pub fn parse_modrinth_index(json: &str) -> Result<MrpackManifest, ModpackError> {
    let raw: RawManifest = serde_json::from_str(json)
        .map_err(|e| ModpackError::MalformedManifest(e.to_string()))?;

    // Extract minecraft version (required).
    let minecraft = raw
        .dependencies
        .get("minecraft")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ModpackError::MalformedManifest("missing 'minecraft' in dependencies".to_string())
        })?
        .to_string();

    // Detect loader: try each known key in priority order.
    let loader = {
        const LOADER_KEYS: &[(&str, &str)] = &[
            ("fabric-loader", "fabric"),
            ("quilt-loader", "quilt"),
            ("neoforge", "neoforge"),
            ("forge", "forge"),
        ];
        let mut found: Option<PackLoader> = None;
        for (dep_key, kind) in LOADER_KEYS {
            if let Some(ver) = raw.dependencies.get(*dep_key).and_then(|v| v.as_str()) {
                found = Some(PackLoader {
                    kind: kind.to_string(),
                    version: Some(ver.to_string()),
                });
                break;
            }
        }
        found.unwrap_or(PackLoader {
            kind: "vanilla".to_string(),
            version: None,
        })
    };

    let files = raw
        .files
        .into_iter()
        .map(|f| MrpackFile {
            path: f.path,
            hashes: f.hashes,
            env: f.env.map(|e| FileEnv {
                client: e.client,
                server: e.server,
            }),
            downloads: f.downloads,
            file_size: f.file_size,
        })
        .collect();

    Ok(MrpackManifest {
        name: raw.name,
        version_id: raw.version_id,
        summary: raw.summary,
        minecraft,
        loader,
        files,
    })
}

// ── B1: CurseForge manifest model ───────────────────────────────────────────

/// A single file entry from a CurseForge `manifest.json` `files[]` array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CfManifestFile {
    /// CurseForge project (mod) id.
    pub project_id: u64,
    /// CurseForge file id (specific version of the mod).
    pub file_id: u64,
    /// Whether this file is required (vs. optional).
    pub required: bool,
}

/// The parsed contents of a CurseForge `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfManifest {
    /// Pack display name.
    pub name: String,
    /// Pack version string.
    pub version: String,
    /// Pack author.
    pub author: String,
    /// Minecraft version string (from `minecraft.version`).
    pub minecraft: String,
    /// Resolved loader for this pack.
    pub loader: PackLoader,
    /// All files declared in the pack manifest.
    pub files: Vec<CfManifestFile>,
    /// Name of the overrides directory inside the zip (default `"overrides"`).
    pub overrides: String,
}

// Raw deserialization shapes — match the on-disk CF manifest.json exactly.
#[derive(Deserialize)]
struct RawCfManifest {
    minecraft: RawCfMinecraft,
    name: String,
    version: String,
    author: String,
    files: Vec<RawCfFile>,
    #[serde(default = "default_overrides_dir")]
    overrides: String,
}

fn default_overrides_dir() -> String {
    "overrides".to_string()
}

#[derive(Deserialize)]
struct RawCfMinecraft {
    version: String,
    #[serde(default, rename = "modLoaders")]
    mod_loaders: Vec<RawCfModLoader>,
}

#[derive(Deserialize)]
struct RawCfModLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct RawCfFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    required: bool,
}

/// Parse a CurseForge `manifest.json` JSON string into a [`CfManifest`].
///
/// The primary `minecraft.modLoaders[].id` is split on the first `-` into
/// loader kind + version (e.g. `"forge-47.2.0"` → `("forge", "47.2.0")`).
/// No `modLoaders` entries → `vanilla`.
///
/// # Errors
/// - [`ModpackError::MalformedManifest`] if the JSON is invalid, missing required
///   fields, or the primary loader id has no `-` separator.
pub fn parse_cf_manifest(json: &str) -> Result<CfManifest, ModpackError> {
    let raw: RawCfManifest = serde_json::from_str(json)
        .map_err(|e| ModpackError::MalformedManifest(e.to_string()))?;

    let loader = {
        let primary = raw
            .minecraft
            .mod_loaders
            .iter()
            .find(|m| m.primary)
            .or_else(|| raw.minecraft.mod_loaders.first());

        match primary {
            None => PackLoader {
                kind: "vanilla".to_string(),
                version: None,
            },
            Some(m) => match m.id.split_once('-') {
                Some((kind, version)) => PackLoader {
                    kind: kind.to_string(),
                    version: Some(version.to_string()),
                },
                None => {
                    return Err(ModpackError::MalformedManifest(format!(
                        "primary modLoaders[].id '{}' has no '-' separator",
                        m.id
                    )));
                }
            },
        }
    };

    let files = raw
        .files
        .into_iter()
        .map(|f| CfManifestFile {
            project_id: f.project_id,
            file_id: f.file_id,
            required: f.required,
        })
        .collect();

    Ok(CfManifest {
        name: raw.name,
        version: raw.version,
        author: raw.author,
        minecraft: raw.minecraft.version,
        loader,
        files,
        overrides: raw.overrides,
    })
}

// ── B3: CurseForge pack planner ──────────────────────────────────────────────

/// A CurseForge file the user must download manually (distribution-disabled,
/// or lacking a usable hash to verify an automated download).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CfManualFile {
    /// CurseForge project (mod) id.
    pub project_id: u64,
    /// CurseForge file id (specific version of the mod).
    pub file_id: u64,
    /// Filename as declared by the resolved file.
    pub file_name: String,
    /// Best-effort project page URL (projectID-based; slug-based link is a follow-up).
    pub page_url: String,
}

/// The result of [`build_cf_pack_plan`]: items to download, mod entries to record,
/// files requiring manual download, and files skipped (reserved for future filters).
#[derive(Debug, Clone)]
pub struct CfPackPlan {
    /// Download items ready for the download engine.
    pub items: Vec<DownloadItem>,
    /// Mod entries for files that were auto-resolved to a download.
    pub mods: Vec<ModEntry>,
    /// Files the user must download manually (no URL, or no usable hash).
    pub manual: Vec<CfManualFile>,
    /// Paths/names of files skipped (reserved; currently always empty).
    pub skipped: Vec<String>,
}

/// Build a [`CfPackPlan`] from a parsed [`CfManifest`] and its resolved files.
///
/// `resolved` pairs each manifest file entry with its [`crate::core::providers::VersionFile`]
/// resolution (from `CurseForgeProvider::get_file`). `mc_dir` is the absolute path to the
/// instance's `mc/` directory; every dest resolves under `mc_dir/mods/`.
///
/// A manifest file routes to `manual` (not `items`) when the resolved `url` is `None`
/// (distribution disabled) or no usable `sha1` hash is present (md5-only/hashless files
/// are not auto-downloaded unverified). Only auto-installed files get a [`ModEntry`].
///
/// # Errors
/// - [`ModpackError::UnsafePath`] — the computed `mods/<fileName>` path is unsafe
///   (contains `..`, is absolute, or has a drive-letter prefix).
pub fn build_cf_pack_plan(
    resolved: &[(CfManifestFile, crate::core::providers::VersionFile)],
    mc_dir: &Path,
) -> Result<CfPackPlan, ModpackError> {
    let mut items: Vec<DownloadItem> = Vec::new();
    let mut mods: Vec<ModEntry> = Vec::new();
    let mut manual: Vec<CfManualFile> = Vec::new();
    let skipped: Vec<String> = Vec::new();

    for (manifest_file, file) in resolved {
        let rel = format!("mods/{}", file.file_name);
        validate_relative_path(&rel)?;

        let sha1 = file.hashes.get("sha1");

        match (&file.url, sha1) {
            (Some(url), Some(hash)) => {
                let dest = mc_dir.join(&rel);

                items.push(DownloadItem {
                    url: url.clone(),
                    dest,
                    expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
                    size: file.size,
                });

                mods.push(ModEntry {
                    provider: "curseforge".to_string(),
                    project_id: manifest_file.project_id.to_string(),
                    version_id: manifest_file.file_id.to_string(),
                    file_name: file.file_name.clone(),
                    hashes: file.hashes.clone().into_iter().collect(),
                    enabled: true,
                    side: "both".to_string(),
                });
            }
            _ => {
                manual.push(CfManualFile {
                    project_id: manifest_file.project_id,
                    file_id: manifest_file.file_id,
                    file_name: file.file_name.clone(),
                    page_url: format!(
                        "https://www.curseforge.com/projects/{}",
                        manifest_file.project_id
                    ),
                });
            }
        }
    }

    Ok(CfPackPlan {
        items,
        mods,
        manual,
        skipped,
    })
}

// ── CP2: Plan builder ─────────────────────────────────────────────────────────

/// The result of [`build_pack_plan`]: items to download, mod entries to record, and
/// files skipped due to `env.client == "unsupported"`.
#[derive(Debug, Clone)]
pub struct PackPlan {
    /// Download items ready for the download engine.
    pub items: Vec<DownloadItem>,
    /// Mod entries (only for files whose path starts with `mods/`).
    pub mods: Vec<ModEntry>,
    /// Paths of files skipped because they are client-unsupported.
    pub skipped: Vec<String>,
}

/// Trusted download hosts as per the Modrinth mrpack specification.
const ALLOWED_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];

/// Validate that a relative path string is safe to use under a base directory.
///
/// Rejects paths that:
/// - Are absolute (start with `/` or a drive letter like `C:`).
/// - Contain a `..` component.
/// - Have a Windows drive-letter prefix (e.g. `C:\`).
fn validate_relative_path(p: &str) -> Result<(), ModpackError> {
    // Reject absolute paths and Windows drive letters.
    if p.starts_with('/') || p.starts_with('\\') {
        return Err(ModpackError::UnsafePath(p.to_string()));
    }
    // Windows drive-letter prefix: "X:" or "X:\"
    if p.len() >= 2 {
        let b = p.as_bytes();
        if b[1] == b':' && b[0].is_ascii_alphabetic() {
            return Err(ModpackError::UnsafePath(p.to_string()));
        }
    }
    // Reject any `..` component or root-dir component.
    let path = Path::new(p);
    for component in path.components() {
        match component {
            Component::ParentDir => return Err(ModpackError::UnsafePath(p.to_string())),
            // RootDir means absolute — caught above on Unix, but guard here too.
            Component::RootDir => return Err(ModpackError::UnsafePath(p.to_string())),
            // Prefix means a Windows drive letter (e.g. `C:`) — also caught above.
            Component::Prefix(_) => return Err(ModpackError::UnsafePath(p.to_string())),
            _ => {}
        }
    }
    Ok(())
}

/// Extract the host from a URL string without a heavy URL parsing dependency.
///
/// Expects `scheme://host/...`.  Returns `None` if the URL is malformed.
fn url_host(url: &str) -> Option<&str> {
    // Strip scheme.
    let after_scheme = url.split_once("://")?.1;
    // Host ends at the first `/`, `?`, `#`, or end-of-string.
    let host = after_scheme
        .split(|c| c == '/' || c == '?' || c == '#')
        .next()?;
    // Strip optional `user@` prefix.
    let host = host.split('@').last()?;
    // Strip optional `:port` suffix.
    let host = host.split(':').next()?;
    Some(host)
}

/// Build a [`PackPlan`] from a parsed [`MrpackManifest`].
///
/// `instance_mc_dir` is the absolute path to the instance's `mc/` directory.
/// All `dest` paths in returned [`DownloadItem`]s resolve under this directory.
///
/// # Errors
/// - [`ModpackError::MissingHash`] — a file has neither `sha512` nor `sha1`.
/// - [`ModpackError::DisallowedHost`] — a `downloads` URL's host is not allowlisted.
/// - [`ModpackError::UnsafePath`] — a `files[].path` contains `..`, is absolute, or has
///   a drive-letter prefix.
pub fn build_pack_plan(
    manifest: &MrpackManifest,
    instance_mc_dir: &Path,
) -> Result<PackPlan, ModpackError> {
    let mut items: Vec<DownloadItem> = Vec::new();
    let mut mods: Vec<ModEntry> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for file in &manifest.files {
        // Env filter.
        if !file.client_supported() {
            skipped.push(file.path.clone());
            continue;
        }

        // Path safety.
        validate_relative_path(&file.path)?;

        // Hash pick: sha512 > sha1; missing both → error.
        let expected_hash = if let Some(h) = file.hashes.get("sha512") {
            ExpectedHash::Sha512(h.clone())
        } else if let Some(h) = file.hashes.get("sha1") {
            ExpectedHash::Sha1(h.clone())
        } else {
            return Err(ModpackError::MissingHash(file.path.clone()));
        };

        // Reject files with no download URLs before the host-allowlist check.
        if file.downloads.is_empty() {
            return Err(ModpackError::NoDownloadUrls(file.path.clone()));
        }

        // Host allowlist: take the first URL whose host is allowed.
        let url = {
            let mut chosen: Option<&str> = None;
            for candidate in &file.downloads {
                let host = url_host(candidate).unwrap_or("");
                if ALLOWED_HOSTS.contains(&host) {
                    chosen = Some(candidate.as_str());
                    break;
                }
            }
            match chosen {
                Some(u) => u.to_string(),
                None => {
                    // Report the host of the first URL (or empty string if none).
                    let bad_host = file
                        .downloads
                        .first()
                        .and_then(|u| url_host(u))
                        .unwrap_or("")
                        .to_string();
                    return Err(ModpackError::DisallowedHost {
                        host: bad_host,
                        path: file.path.clone(),
                    });
                }
            }
        };

        let dest = instance_mc_dir.join(&file.path);

        items.push(DownloadItem {
            url,
            dest: dest.clone(),
            expected_hash: Some(expected_hash),
            size: Some(file.file_size),
        });

        // ModEntry only for files under mods/.
        if file.path.starts_with("mods/") {
            let file_name = Path::new(&file.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.path.clone());

            mods.push(ModEntry {
                provider: "modrinth".to_string(),
                project_id: String::new(),
                version_id: String::new(),
                file_name,
                hashes: file.hashes.clone(),
                enabled: true,
                side: file.side(),
            });
        }
    }

    Ok(PackPlan {
        items,
        mods,
        skipped,
    })
}

// ── CP4: In-memory zip reader (testable seam) ─────────────────────────────────

/// Open an `.mrpack` zip from raw bytes, read `modrinth.index.json`, parse it,
/// and build a [`PackPlan`] relative to `instance_mc_dir`.
///
/// This is the pure testable seam used by the `import_mrpack` Tauri command.
/// It is I/O-free beyond reading the in-memory byte slice — no `AppHandle`, no
/// network, no filesystem writes.
///
/// The caller is responsible for running `extract_overrides` on the same zip
/// bytes (re-opened from a fresh `Cursor`) to apply overrides after downloading.
///
/// # Errors
/// - [`ModpackError::IndexNotFound`] — `modrinth.index.json` absent from archive.
/// - [`ModpackError::MalformedManifest`] — JSON parse error.
/// - [`ModpackError::Zip`] — zip crate error reading the archive.
/// - All errors from [`build_pack_plan`].
pub fn read_mrpack(
    bytes: &[u8],
    instance_mc_dir: &Path,
) -> Result<(MrpackManifest, PackPlan), ModpackError> {
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    // Read modrinth.index.json from the archive.
    let index_json = {
        let mut entry = archive
            .by_name("modrinth.index.json")
            .map_err(|_| ModpackError::IndexNotFound)?;
        let mut buf = String::new();
        entry.read_to_string(&mut buf)?;
        buf
    };

    let manifest = parse_modrinth_index(&index_json)?;
    let plan = build_pack_plan(&manifest, instance_mc_dir)?;
    Ok((manifest, plan))
}

// ── CP3: Overrides extraction ─────────────────────────────────────────────────

/// Copy `overrides/` then `client-overrides/` from the archive into `mc_dir`,
/// stripping the prefix in each case.  `server-overrides/` is ignored.
///
/// Returns the total number of files extracted (directories are not counted).
///
/// # Safety
/// Every entry path is validated to resolve strictly under `mc_dir`.  Any entry
/// that would escape (zip-slip) is rejected with [`ModpackError::ZipSlip`].
///
/// # Errors
/// - [`ModpackError::UnsafePath`] — an entry path is structurally unsafe.
/// - [`ModpackError::ZipSlip`] — an entry resolves outside `mc_dir`.
/// - [`ModpackError::Io`] — an I/O error during extraction.
/// - [`ModpackError::Zip`] — a zip-crate error reading the archive.
pub fn extract_overrides<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    mc_dir: &Path,
) -> Result<u32, ModpackError> {
    let mut count = 0u32;

    for prefix in &["overrides/", "client-overrides/"] {
        count += extract_prefix(archive, mc_dir, prefix)?;
    }

    Ok(count)
}

/// Extract all entries under `prefix` from `archive` into `mc_dir`.
fn extract_prefix<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    mc_dir: &Path,
    prefix: &str,
) -> Result<u32, ModpackError> {
    let mut count = 0u32;

    // Collect names to avoid borrow conflict.
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if name.starts_with(prefix) && name.len() > prefix.len() {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    for name in names {
        // Strip the prefix to get the relative path inside mc_dir.
        let rel = &name[prefix.len()..];

        // Structural safety check.
        validate_relative_path(rel).map_err(|_| ModpackError::UnsafePath(name.clone()))?;

        let dest = mc_dir.join(rel);

        // Zip-slip guard: structural check that dest resolves under mc_dir.
        // is_safe_dest operates purely on path components — no canonicalize needed.
        if !is_safe_dest(&dest, mc_dir) {
            return Err(ModpackError::ZipSlip(name.clone()));
        }

        let mut entry = archive.by_name(&name)?;

        if entry.is_dir() {
            std::fs::create_dir_all(&dest)?;
            continue;
        }

        // Create parent directories.
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        let mut out = std::fs::File::create(&dest)?;
        out.write_all(&buf)?;

        count += 1;
    }

    Ok(count)
}

/// Return `true` if `rel` resolves strictly under `base` without escaping.
///
/// This is a purely structural check on the *relative* path — it does not
/// call `canonicalize` and does not require `base` to exist on disk.
/// `base` is the primary gate; `validate_relative_path` in the caller
/// catches the common cases early, but this guard is the authoritative
/// containment check.
///
/// The signature accepts `dest` (the already-joined path) for backward
/// compatibility with the call site, but ignores the base prefix embedded
/// in it — only `rel` matters for the safety decision.
fn is_safe_dest(dest: &Path, base: &Path) -> bool {
    // Recover `rel` by stripping the base prefix from `dest`.  If stripping
    // fails (dest is not under base at the raw string level) that alone is a
    // rejection signal, but we still proceed with a full structural walk to
    // be safe; in practice `dest` was always produced via `base.join(rel)`.
    let rel = dest.strip_prefix(base).unwrap_or(dest);

    // Walk the relative components structurally; any escape attempt returns false.
    let mut resolved = base.to_path_buf();
    for component in rel.components() {
        match component {
            // `..` would escape the base — reject immediately.
            Component::ParentDir => return false,
            Component::Normal(s) => resolved.push(s),
            Component::CurDir => {}
            // Absolute root or drive prefix inside a relative path is illegal.
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    resolved.starts_with(base)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as IoWrite;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    // ── CP1: parse_modrinth_index ─────────────────────────────────────────────

    #[test]
    fn cp1_parse_fabric_manifest() {
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).expect("should parse");
        assert_eq!(manifest.name, "Test Fabric Pack");
        assert_eq!(manifest.version_id, "1.0.0");
        assert_eq!(manifest.minecraft, "1.21.1");
        assert_eq!(manifest.loader.kind, "fabric");
        assert_eq!(manifest.loader.version.as_deref(), Some("0.16.0"));
        assert_eq!(manifest.files.len(), 3);
    }

    #[test]
    fn cp1_loader_fabric_key_maps_correctly() {
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        assert_eq!(manifest.loader.kind, "fabric");
        assert_eq!(manifest.loader.version.as_deref(), Some("0.16.0"));
    }

    #[test]
    fn cp1_loader_quilt_key_maps_correctly() {
        let json = include_str!("fixtures/mrpack_quilt.json");
        let manifest = parse_modrinth_index(json).unwrap();
        assert_eq!(manifest.loader.kind, "quilt");
        assert_eq!(manifest.loader.version.as_deref(), Some("0.27.1"));
    }

    #[test]
    fn cp1_loader_forge_key_maps_correctly() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Forge Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.20.1", "forge": "47.2.0" },
            "files": []
        }"#;
        let manifest = parse_modrinth_index(json).unwrap();
        assert_eq!(manifest.loader.kind, "forge");
        assert_eq!(manifest.loader.version.as_deref(), Some("47.2.0"));
    }

    #[test]
    fn cp1_loader_neoforge_key_maps_correctly() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "NeoForge Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1", "neoforge": "21.1.0" },
            "files": []
        }"#;
        let manifest = parse_modrinth_index(json).unwrap();
        assert_eq!(manifest.loader.kind, "neoforge");
        assert_eq!(manifest.loader.version.as_deref(), Some("21.1.0"));
    }

    #[test]
    fn cp1_no_loader_key_maps_to_vanilla() {
        let json = include_str!("fixtures/mrpack_vanilla.json");
        let manifest = parse_modrinth_index(json).unwrap();
        assert_eq!(manifest.loader.kind, "vanilla");
        assert!(manifest.loader.version.is_none());
        assert_eq!(manifest.minecraft, "1.20.4");
    }

    #[test]
    fn cp1_env_unsupported_client_is_flagged() {
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        // Second file has env.client == "unsupported".
        let server_file = manifest.files.iter().find(|f| f.path == "mods/server-only-mod.jar").unwrap();
        assert!(!server_file.client_supported());
    }

    #[test]
    fn cp1_absent_env_is_client_supported() {
        let json = include_str!("fixtures/mrpack_quilt.json");
        let manifest = parse_modrinth_index(json).unwrap();
        // qfapi.jar has no env field.
        let file = &manifest.files[0];
        assert!(file.env.is_none());
        assert!(file.client_supported());
    }

    #[test]
    fn cp1_malformed_json_returns_error() {
        let result = parse_modrinth_index("{ not json }");
        assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
    }

    #[test]
    fn cp1_missing_minecraft_dep_returns_error() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Bad Pack",
            "versionId": "1.0",
            "dependencies": {},
            "files": []
        }"#;
        let result = parse_modrinth_index(json);
        assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
    }

    // ── B1: parse_cf_manifest ─────────────────────────────────────────────────

    #[test]
    fn b1_parse_forge_manifest() {
        let json = include_str!("fixtures/cf_manifest_forge.json");
        let manifest = parse_cf_manifest(json).expect("should parse");
        assert_eq!(manifest.name, "Test Forge Pack");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.minecraft, "1.20.1");
        assert_eq!(manifest.loader.kind, "forge");
        assert_eq!(manifest.loader.version.as_deref(), Some("47.2.0"));
        assert_eq!(manifest.files.len(), 2);
        assert_eq!(manifest.files[0].project_id, 238222);
        assert_eq!(manifest.files[0].file_id, 4536804);
        assert!(manifest.files[0].required);
        assert!(!manifest.files[1].required);
    }

    #[test]
    fn b1_loader_fabric_prefix_maps_correctly() {
        let json = include_str!("fixtures/cf_manifest_fabric.json");
        let manifest = parse_cf_manifest(json).unwrap();
        assert_eq!(manifest.loader.kind, "fabric");
        assert_eq!(manifest.loader.version.as_deref(), Some("0.16.0"));
    }

    #[test]
    fn b1_loader_quilt_prefix_maps_correctly() {
        let json = r#"{
            "minecraft": { "version": "1.21.1", "modLoaders": [{ "id": "quilt-0.27.1", "primary": true }] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Quilt Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
        let manifest = parse_cf_manifest(json).unwrap();
        assert_eq!(manifest.loader.kind, "quilt");
        assert_eq!(manifest.loader.version.as_deref(), Some("0.27.1"));
    }

    #[test]
    fn b1_loader_neoforge_prefix_maps_correctly() {
        let json = r#"{
            "minecraft": { "version": "1.21.1", "modLoaders": [{ "id": "neoforge-21.1.0", "primary": true }] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "NeoForge Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
        let manifest = parse_cf_manifest(json).unwrap();
        assert_eq!(manifest.loader.kind, "neoforge");
        assert_eq!(manifest.loader.version.as_deref(), Some("21.1.0"));
    }

    #[test]
    fn b1_no_loader_entry_maps_to_vanilla() {
        let json = include_str!("fixtures/cf_manifest_vanilla.json");
        let manifest = parse_cf_manifest(json).unwrap();
        assert_eq!(manifest.loader.kind, "vanilla");
        assert!(manifest.loader.version.is_none());
        assert_eq!(manifest.minecraft, "1.20.4");
        assert!(manifest.files.is_empty());
    }

    #[test]
    fn b1_non_primary_loader_entry_is_ignored() {
        // Only the primary modLoaders[] entry should be used.
        let json = r#"{
            "minecraft": { "version": "1.20.1", "modLoaders": [
                { "id": "forge-47.2.0", "primary": true },
                { "id": "fabric-0.16.0", "primary": false }
            ] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Mixed Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
        let manifest = parse_cf_manifest(json).unwrap();
        assert_eq!(manifest.loader.kind, "forge");
        assert_eq!(manifest.loader.version.as_deref(), Some("47.2.0"));
    }

    #[test]
    fn b1_malformed_json_returns_error() {
        let result = parse_cf_manifest("{ not json }");
        assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
    }

    #[test]
    fn b1_missing_minecraft_field_returns_error() {
        let json = r#"{
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Bad Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
        let result = parse_cf_manifest(json);
        assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
    }

    #[test]
    fn b1_loader_id_without_dash_returns_error() {
        // A modLoaders[].id with no '-' separator can't be split into kind+version.
        let json = r#"{
            "minecraft": { "version": "1.20.1", "modLoaders": [{ "id": "forgeonly", "primary": true }] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Bad Loader Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
        let result = parse_cf_manifest(json);
        assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
    }

    // ── B3: build_cf_pack_plan ─────────────────────────────────────────────────

    use crate::core::providers::VersionFile;

    fn cf_manifest_file(project_id: u64, file_id: u64) -> CfManifestFile {
        CfManifestFile {
            project_id,
            file_id,
            required: true,
        }
    }

    fn version_file_with(
        url: Option<&str>,
        file_name: &str,
        hashes: &[(&str, &str)],
    ) -> VersionFile {
        VersionFile {
            url: url.map(|s| s.to_string()),
            file_name: file_name.to_string(),
            size: Some(1024),
            hashes: hashes
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            primary: true,
        }
    }

    #[test]
    fn b3_dest_resolves_under_mc_mods() {
        let tmp = mc_dir();
        let resolved = vec![(
            cf_manifest_file(238222, 4536804),
            version_file_with(
                Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
                "jei.jar",
                &[("sha1", "aabbcc")],
            ),
        )];
        let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].dest, tmp.path().join("mods/jei.jar"));
    }

    #[test]
    fn b3_mod_entry_ids_are_project_and_file_id() {
        let tmp = mc_dir();
        let resolved = vec![(
            cf_manifest_file(238222, 4536804),
            version_file_with(
                Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
                "jei.jar",
                &[("sha1", "aabbcc")],
            ),
        )];
        let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
        assert_eq!(plan.mods.len(), 1);
        assert_eq!(plan.mods[0].provider, "curseforge");
        assert_eq!(plan.mods[0].project_id, "238222");
        assert_eq!(plan.mods[0].version_id, "4536804");
        assert!(plan.mods[0].enabled);
    }

    #[test]
    fn b3_url_none_routes_to_manual() {
        let tmp = mc_dir();
        let resolved = vec![(
            cf_manifest_file(238222, 4536804),
            version_file_with(None, "jei.jar", &[("sha1", "aabbcc")]),
        )];
        let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
        assert!(plan.items.is_empty());
        assert!(plan.mods.is_empty());
        assert_eq!(plan.manual.len(), 1);
        assert_eq!(plan.manual[0].project_id, 238222);
        assert_eq!(plan.manual[0].file_id, 4536804);
        assert_eq!(
            plan.manual[0].page_url,
            "https://www.curseforge.com/projects/238222"
        );
    }

    #[test]
    fn b3_no_sha1_md5_only_routes_to_manual() {
        let tmp = mc_dir();
        let resolved = vec![(
            cf_manifest_file(238222, 4536804),
            version_file_with(
                Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
                "jei.jar",
                &[("md5", "ddeeff")],
            ),
        )];
        let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
        assert!(plan.items.is_empty());
        assert_eq!(plan.manual.len(), 1);
    }

    #[test]
    fn b3_manual_file_does_not_abort_other_entries() {
        let tmp = mc_dir();
        let resolved = vec![
            (
                cf_manifest_file(1, 2),
                version_file_with(None, "manual-mod.jar", &[("sha1", "aabbcc")]),
            ),
            (
                cf_manifest_file(238222, 4536804),
                version_file_with(
                    Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
                    "jei.jar",
                    &[("sha1", "aabbcc")],
                ),
            ),
        ];
        let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
        assert_eq!(plan.manual.len(), 1);
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.mods.len(), 1);
    }

    #[test]
    fn b3_unsafe_filename_returns_error() {
        let tmp = mc_dir();
        let resolved = vec![(
            cf_manifest_file(238222, 4536804),
            version_file_with(
                Some("https://edge.forgecdn.net/files/1/2/evil.jar"),
                "../../etc/passwd",
                &[("sha1", "aabbcc")],
            ),
        )];
        let result = build_cf_pack_plan(&resolved, tmp.path());
        assert!(matches!(result, Err(ModpackError::UnsafePath(_))));
    }

    // ── CP2: build_pack_plan ──────────────────────────────────────────────────

    fn mc_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn cp2_dest_path_resolves_under_mc_dir() {
        let tmp = mc_dir();
        let mc = tmp.path();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, mc).unwrap();
        for item in &plan.items {
            assert!(
                item.dest.starts_with(mc),
                "dest {} does not start with mc_dir {}",
                item.dest.display(),
                mc.display()
            );
        }
    }

    #[test]
    fn cp2_sha512_preferred_over_sha1() {
        let tmp = mc_dir();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        // Sodium mod has both sha1 and sha512; sha512 must win.
        let sodium = plan.items.iter().find(|i| {
            i.dest.to_string_lossy().contains("sodium")
        }).expect("sodium item");
        assert!(matches!(sodium.expected_hash, Some(ExpectedHash::Sha512(_))));
    }

    #[test]
    fn cp2_sha1_used_when_sha512_absent() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/only-sha1.jar",
                "hashes": { "sha1": "aabbccddee" },
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/only-sha1.jar"],
                "fileSize": 100
            }]
        }"#;
        let tmp = mc_dir();
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        assert!(matches!(plan.items[0].expected_hash, Some(ExpectedHash::Sha1(_))));
    }

    #[test]
    fn cp2_no_hash_returns_error() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/no-hash.jar",
                "hashes": {},
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/no-hash.jar"],
                "fileSize": 100
            }]
        }"#;
        let tmp = mc_dir();
        let manifest = parse_modrinth_index(json).unwrap();
        let result = build_pack_plan(&manifest, tmp.path());
        assert!(matches!(result, Err(ModpackError::MissingHash(_))));
    }

    #[test]
    fn cp2_disallowed_host_returns_error() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/evil.jar",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://evil.example.com/evil.jar"],
                "fileSize": 100
            }]
        }"#;
        let tmp = mc_dir();
        let manifest = parse_modrinth_index(json).unwrap();
        let result = build_pack_plan(&manifest, tmp.path());
        assert!(matches!(result, Err(ModpackError::DisallowedHost { .. })));
    }

    #[test]
    fn cp2_dotdot_path_returns_error() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "../../../etc/passwd",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/evil.jar"],
                "fileSize": 100
            }]
        }"#;
        let tmp = mc_dir();
        let manifest = parse_modrinth_index(json).unwrap();
        let result = build_pack_plan(&manifest, tmp.path());
        assert!(matches!(result, Err(ModpackError::UnsafePath(_))));
    }

    #[test]
    fn cp2_absolute_path_returns_error() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "/etc/passwd",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/evil.jar"],
                "fileSize": 100
            }]
        }"#;
        let tmp = mc_dir();
        let manifest = parse_modrinth_index(json).unwrap();
        let result = build_pack_plan(&manifest, tmp.path());
        assert!(matches!(result, Err(ModpackError::UnsafePath(_))));
    }

    #[test]
    fn cp2_env_unsupported_goes_to_skipped() {
        let tmp = mc_dir();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        assert!(plan.skipped.contains(&"mods/server-only-mod.jar".to_string()));
    }

    #[test]
    fn cp2_only_mods_prefix_files_get_mod_entry() {
        let tmp = mc_dir();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        // Only sodium (mods/) should have a ModEntry; config file should not.
        assert_eq!(plan.mods.len(), 1);
        assert_eq!(plan.mods[0].file_name, "sodium-fabric-0.6.0.jar");
        assert_eq!(plan.mods[0].provider, "modrinth");
        assert!(plan.mods[0].project_id.is_empty());
        assert!(plan.mods[0].version_id.is_empty());
        assert!(plan.mods[0].enabled);
    }

    #[test]
    fn cp2_mod_entry_side_from_env() {
        let tmp = mc_dir();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        // Sodium has client=required, server=unsupported → side should be "client".
        assert_eq!(plan.mods[0].side, "client");
    }

    #[test]
    fn cp2_size_populated_on_download_item() {
        let tmp = mc_dir();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        let sodium = plan.items.iter().find(|i| i.dest.to_string_lossy().contains("sodium")).unwrap();
        assert_eq!(sodium.size, Some(1048576));
    }

    #[test]
    fn cp2_empty_downloads_returns_error() {
        // A file with downloads: [] must produce a clear NoDownloadUrls error,
        // not a misleading DisallowedHost with an empty host string.
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/no-url.jar",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": [],
                "fileSize": 100
            }]
        }"#;
        let tmp = mc_dir();
        let manifest = parse_modrinth_index(json).unwrap();
        let result = build_pack_plan(&manifest, tmp.path());
        assert!(
            matches!(result, Err(ModpackError::NoDownloadUrls(ref p)) if p == "mods/no-url.jar"),
            "expected NoDownloadUrls(\"mods/no-url.jar\"), got: {:?}",
            result
        );
    }

    #[test]
    fn cp2_is_safe_dest_rejects_dotdot_in_rel() {
        // Exercises is_safe_dest directly: a joined path whose relative suffix
        // contains ".." must be rejected even if validate_relative_path already
        // catches it upstream.  This test is the primary coverage gate for the
        // structural guard — do not remove it if validate_relative_path is ever
        // refactored or bypassed.
        use std::path::PathBuf;
        let base = PathBuf::from("/tmp/mc");
        // Simulate what mc_dir.join("../escape") produces.
        let dest = base.join("..").join("escape");
        assert!(
            !is_safe_dest(&dest, &base),
            "is_safe_dest should reject a path with '..' that escapes the base"
        );
    }

    #[test]
    fn cp2_github_host_is_allowed() {
        let tmp = mc_dir();
        let json = include_str!("fixtures/mrpack_fabric.json");
        let manifest = parse_modrinth_index(json).unwrap();
        let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
        // config/some-config.toml downloads from raw.githubusercontent.com.
        let config_item = plan.items.iter().find(|i| i.dest.to_string_lossy().contains("some-config")).unwrap();
        assert!(config_item.url.contains("raw.githubusercontent.com"));
    }

    // ── CP3: extract_overrides ────────────────────────────────────────────────

    /// Build an in-memory zip archive for testing.
    fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn cp3_overrides_files_land_under_mc_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mc_dir = tmp.path();

        let zip_bytes = build_test_zip(&[
            ("overrides/config/my.toml", b"[settings]"),
        ]);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let count = extract_overrides(&mut archive, mc_dir).unwrap();
        assert_eq!(count, 1);
        assert!(mc_dir.join("config/my.toml").exists());
    }

    #[test]
    fn cp3_client_overrides_applied() {
        let tmp = tempfile::tempdir().unwrap();
        let mc_dir = tmp.path();

        let zip_bytes = build_test_zip(&[
            ("client-overrides/options.txt", b"renderDistance:12"),
        ]);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let count = extract_overrides(&mut archive, mc_dir).unwrap();
        assert_eq!(count, 1);
        assert!(mc_dir.join("options.txt").exists());
    }

    #[test]
    fn cp3_server_overrides_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let mc_dir = tmp.path();

        let zip_bytes = build_test_zip(&[
            ("overrides/client.txt", b"client"),
            ("server-overrides/server.txt", b"server"),
        ]);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let count = extract_overrides(&mut archive, mc_dir).unwrap();
        assert_eq!(count, 1);
        assert!(mc_dir.join("client.txt").exists());
        assert!(!mc_dir.join("server.txt").exists());
    }

    #[test]
    fn cp3_zipslip_entry_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mc_dir = tmp.path();

        // A zip-slip entry: overrides/../../../etc/passwd would escape.
        let zip_bytes = build_test_zip(&[
            ("overrides/../../../escape.txt", b"pwned"),
        ]);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let result = extract_overrides(&mut archive, mc_dir);
        // Should return either ZipSlip or UnsafePath.
        assert!(
            matches!(result, Err(ModpackError::ZipSlip(_)) | Err(ModpackError::UnsafePath(_))),
            "expected ZipSlip or UnsafePath, got: {:?}", result
        );
    }

    #[test]
    fn cp3_collision_override_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let mc_dir = tmp.path();

        // Write a file that looks like it came from the download step.
        let target = mc_dir.join("options.txt");
        std::fs::write(&target, b"original").unwrap();

        // Override it.
        let zip_bytes = build_test_zip(&[
            ("client-overrides/options.txt", b"overridden"),
        ]);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        extract_overrides(&mut archive, mc_dir).unwrap();

        let content = std::fs::read(&target).unwrap();
        assert_eq!(content, b"overridden");
    }

    #[test]
    fn cp3_count_excludes_server_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let mc_dir = tmp.path();

        let zip_bytes = build_test_zip(&[
            ("overrides/a.txt", b"a"),
            ("client-overrides/b.txt", b"b"),
            ("server-overrides/c.txt", b"c"),
        ]);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let count = extract_overrides(&mut archive, mc_dir).unwrap();
        assert_eq!(count, 2);
    }

    // ── CP4: read_mrpack ──────────────────────────────────────────────────────

    /// Build an in-memory `.mrpack` zip with a `modrinth.index.json` entry.
    fn build_mrpack(index_json: &str) -> Vec<u8> {
        build_test_zip(&[("modrinth.index.json", index_json.as_bytes())])
    }

    #[test]
    fn cp4_read_mrpack_happy_path_parses_manifest_and_plan() {
        let tmp = mc_dir();
        // Use the fabric fixture JSON (has 3 files: 1 client mod + 1 server-only + 1 config).
        let index_json = include_str!("fixtures/mrpack_fabric.json");
        let mrpack_bytes = build_mrpack(index_json);

        let (manifest, plan) = read_mrpack(&mrpack_bytes, tmp.path())
            .expect("should parse happy-path mrpack");

        assert_eq!(manifest.name, "Test Fabric Pack");
        assert_eq!(manifest.minecraft, "1.21.1");
        assert_eq!(manifest.loader.kind, "fabric");

        // 1 client mod (sodium) + 1 config file = 2 download items
        // (server-only-mod.jar is skipped).
        assert_eq!(plan.items.len(), 2, "expected 2 download items");
        assert_eq!(plan.skipped.len(), 1, "expected 1 skipped file");
        assert_eq!(plan.mods.len(), 1, "expected 1 mod entry");
        assert_eq!(plan.mods[0].file_name, "sodium-fabric-0.6.0.jar");

        // All dest paths must be under the mc_dir.
        for item in &plan.items {
            assert!(item.dest.starts_with(tmp.path()));
        }
    }

    #[test]
    fn cp4_read_mrpack_missing_index_returns_error() {
        // A zip with no modrinth.index.json entry.
        let mrpack_bytes = build_test_zip(&[("README.txt", b"hello")]);
        let tmp = mc_dir();
        let result = read_mrpack(&mrpack_bytes, tmp.path());
        assert!(
            matches!(result, Err(ModpackError::IndexNotFound)),
            "expected IndexNotFound, got: {:?}",
            result
        );
    }

    #[test]
    fn cp4_read_mrpack_malformed_json_returns_error() {
        let mrpack_bytes = build_mrpack("{ not valid json }");
        let tmp = mc_dir();
        let result = read_mrpack(&mrpack_bytes, tmp.path());
        assert!(
            matches!(result, Err(ModpackError::MalformedManifest(_))),
            "expected MalformedManifest, got: {:?}",
            result
        );
    }

    #[test]
    fn cp4_read_mrpack_not_a_zip_returns_error() {
        // Random bytes that are not a valid zip.
        let not_a_zip = b"this is definitely not a zip file";
        let tmp = mc_dir();
        let result = read_mrpack(not_a_zip, tmp.path());
        assert!(
            matches!(result, Err(ModpackError::Zip(_))),
            "expected Zip error for non-zip bytes, got: {:?}",
            result
        );
    }

    #[test]
    fn cp4_read_mrpack_disallowed_host_propagates_error() {
        let index_json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Evil Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.15.0" },
            "files": [{
                "path": "mods/evil.jar",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://evil.example.com/evil.jar"],
                "fileSize": 100
            }]
        }"#;
        let mrpack_bytes = build_mrpack(index_json);
        let tmp = mc_dir();
        let result = read_mrpack(&mrpack_bytes, tmp.path());
        assert!(
            matches!(result, Err(ModpackError::DisallowedHost { .. })),
            "expected DisallowedHost from read_mrpack, got: {:?}",
            result
        );
    }
}
