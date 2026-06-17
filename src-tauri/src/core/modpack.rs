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

    /// The pack index/manifest entry (`modrinth.index.json` or CF `manifest.json`)
    /// was not found inside the archive.
    #[error("pack index/manifest not found in archive")]
    IndexNotFound,

    /// The CurseForge API key is not configured — no pack file can be resolved,
    /// so the whole import aborts rather than producing a pack full of "manual"
    /// entries that hide a config problem.
    #[error("CurseForge API key is not configured — cannot resolve pack files")]
    ResolverKeyMissing,

    /// A provider error during pack-file resolution (network, key-missing, bad response, etc.).
    #[error("provider error resolving pack file: {0}")]
    ResolverError(#[from] crate::core::providers::ProviderError),

    /// No versions were returned for a project — cannot pick a file to install.
    #[error("no versions found for this project — cannot resolve a pack file")]
    NoVersions,

    /// The latest version returned has no files — cannot pick a file to install.
    #[error("the latest version for this project has no files")]
    NoFiles,

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
#[serde(rename_all = "camelCase")]
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

/// A CurseForge `files[]` entry whose resolution (`get_file`) itself failed
/// (network error, non-2xx HTTP status, or unparseable response) — distinct
/// from a successfully-resolved file with no usable URL/hash (`CfManualFile`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CfResolveFailure {
    /// CurseForge project (mod) id.
    pub project_id: u64,
    /// CurseForge file id (specific version of the mod).
    pub file_id: u64,
    /// Human-readable reason the resolution failed.
    pub reason: String,
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
    /// Entries whose `get_file` resolution itself failed (network/HTTP/JSON
    /// error) — counted as `failed`, not `manual`. Always empty when produced
    /// directly by [`build_cf_pack_plan`]; populated by [`resolve_and_build_cf_plan`].
    pub failed: Vec<CfResolveFailure>,
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
        failed: Vec::new(),
    })
}

// ── B4: CF file resolution + zip read (testable seam) ─────────────────────────

/// Resolve every `files[]` entry in `manifest` via `CurseForgeProvider::get_file`,
/// then build the [`CfPackPlan`].
///
/// This is the seam the `import_curseforge_zip` Tauri command calls; it takes an
/// injectable [`crate::core::providers::ProviderHttpClient`] so tests can drive it
/// with a mock client (canned responses) — no live network. Resolution is
/// per-entry (slice B: single-file GET, no batch endpoint).
///
/// `get_file` errors are branched by kind:
/// - [`crate::core::providers::ProviderError::KeyMissing`] — no CF API key configured;
///   no file can possibly resolve, so the whole import aborts with `Err` rather than
///   silently producing a pack full of "manual" entries that hide a config problem.
/// - Network / HTTP-status / JSON-decode errors — a genuine resolution failure for
///   that one entry; recorded in [`CfPackPlan::failed`] (not `manual`) so the rest of
///   the pack still installs.
///
/// A successfully-resolved file with `url: None` or no usable hash still routes to
/// `manual` via [`build_cf_pack_plan`] (distribution-disabled or hashless — the user
/// must download it themselves; this is not an error).
pub async fn resolve_and_build_cf_plan(
    provider: &crate::core::curseforge::CurseForgeProvider,
    client: &dyn crate::core::providers::ProviderHttpClient,
    manifest: &CfManifest,
    mc_dir: &Path,
) -> Result<CfPackPlan, ModpackError> {
    let mut resolved: Vec<(CfManifestFile, crate::core::providers::VersionFile)> = Vec::new();
    let mut failed: Vec<CfResolveFailure> = Vec::new();

    for entry in &manifest.files {
        match provider
            .get_file(client, entry.project_id as u32, entry.file_id as u32)
            .await
        {
            Ok(file) => resolved.push((entry.clone(), file)),
            Err(crate::core::providers::ProviderError::KeyMissing) => {
                return Err(ModpackError::ResolverKeyMissing);
            }
            Err(e) => failed.push(CfResolveFailure {
                project_id: entry.project_id,
                file_id: entry.file_id,
                reason: e.to_string(),
            }),
        }
    }

    let mut plan = build_cf_pack_plan(&resolved, mc_dir)?;
    plan.failed = failed;
    Ok(plan)
}

/// Open a CurseForge pack `.zip` from raw bytes and parse its `manifest.json`.
///
/// Pure (no network); the in-memory analog of `read_mrpack`'s index read. The
/// caller resolves files (network) separately via [`resolve_and_build_cf_plan`],
/// then re-opens the same bytes for `extract_overrides`.
///
/// # Errors
/// - [`ModpackError::IndexNotFound`] — `manifest.json` absent from archive.
/// - [`ModpackError::MalformedManifest`] — JSON parse error.
/// - [`ModpackError::Zip`] — zip crate error reading the archive.
pub fn read_cf_manifest(bytes: &[u8]) -> Result<CfManifest, ModpackError> {
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    let manifest_json = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|_| ModpackError::IndexNotFound)?;
        let mut buf = String::new();
        entry.read_to_string(&mut buf)?;
        buf
    };

    parse_cf_manifest(&manifest_json)
}

// ── C2: Pack-file resolver seam ───────────────────────────────────────────────

/// The result of resolving a modpack project's latest version to a single installable file.
///
/// `url: None` is a valid outcome when the file's distribution is disabled (CF
/// `allowModDistribution: false`); C3 turns this into a manual-download outcome.
/// It is never an error.
#[derive(Debug, Clone)]
pub struct ResolvedPackFile {
    /// Direct download URL; `None` means distribution is disabled (manual outcome).
    pub url: Option<String>,
    /// Filename declared by the provider (e.g. `"mypack-1.0.mrpack"`).
    pub file_name: String,
    /// Which provider supplied this result (useful for C3 dispatch).
    pub provider: crate::core::providers::ProviderKind,
}

/// Resolve a modpack project to its latest version's primary (or first) file.
///
/// Calls `provider.get_versions(client, project_id, None, None)` — no mc/loader
/// filter, because the pack itself defines those. "Latest" = the **first version
/// returned**; both Modrinth and CurseForge return versions newest-first.
///
/// File selection: `files.iter().find(|f| f.primary)`, falling back to the first
/// file if none is flagged primary.
///
/// `url: None` on the selected file is **not** an error — it is returned as-is;
/// the caller (C3) is responsible for routing that to a manual-download outcome.
///
/// # Errors
/// - [`ModpackError::NoVersions`] — the provider returned an empty version list.
/// - [`ModpackError::NoFiles`] — the latest version carries no files.
/// - Any [`crate::core::providers::ProviderError`] wrapped in [`ModpackError::ResolverError`].
pub async fn resolve_pack_file(
    provider: &dyn crate::core::providers::ModProvider,
    client: &dyn crate::core::providers::ProviderHttpClient,
    project_id: &str,
) -> Result<ResolvedPackFile, ModpackError> {
    let versions = provider
        .get_versions(client, project_id, None, None)
        .await
        .map_err(ModpackError::ResolverError)?;

    let latest = versions.into_iter().next().ok_or(ModpackError::NoVersions)?;

    let file = latest
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| latest.files.first())
        .ok_or(ModpackError::NoFiles)?
        .clone();

    Ok(ResolvedPackFile {
        url: file.url,
        file_name: file.file_name,
        provider: latest.provider,
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
#[path = "modpack_tests.rs"]
mod tests;
