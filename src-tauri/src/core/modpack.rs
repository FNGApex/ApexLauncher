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

use crate::core::atl::{AtlConfigsManifest, AtlMod};
use crate::core::download::{DownloadItem, ExpectedHash};
use crate::core::instances::{ModEntry, PendingManual};

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

    /// The requested version id was not found in the version list.
    #[error("version '{0}' not found for this project")]
    VersionNotFound(String),

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CfManualFile {
    /// CurseForge project (mod) id.
    pub project_id: u64,
    /// CurseForge file id (specific version of the mod).
    pub file_id: u64,
    /// Filename as declared by the resolved file.
    pub file_name: String,
    /// Best-effort CurseForge page URL — slug-based exact-file page when available
    /// (`resolve_and_build_cf_plan`), else the numeric `…/projects/{id}` redirect.
    pub page_url: String,
    /// SHA-1 of the file if CF declared one; `None` → name/size match only when the
    /// user later drops the jar in (auto-recovery, Pillar 3).
    #[serde(default)]
    pub expected_sha1: Option<String>,
    /// Declared file size in bytes, if known.
    #[serde(default)]
    pub size: Option<u64>,
}

impl From<&CfManualFile> for PendingManual {
    /// Map a planner-side manual entry to its persisted manifest form. Ids are
    /// stringified to match the `ModEntry`/`PendingManual` convention.
    fn from(m: &CfManualFile) -> Self {
        PendingManual {
            project_id: m.project_id.to_string(),
            file_id: m.file_id.to_string(),
            file_name: m.file_name.clone(),
            page_url: m.page_url.clone(),
            expected_sha1: m.expected_sha1.clone(),
            size: m.size,
        }
    }
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

/// Build the best CurseForge page URL for a manual-download file.
///
/// With a known project `slug` this is the **exact file page**
/// `…/minecraft/mc-mods/{slug}/files/{file_id}`; without one (slug fetch failed or the
/// project carried none) it falls back to the numeric `…/projects/{project_id}` redirect.
pub fn cf_file_page_url(slug: Option<&str>, project_id: u64, file_id: u64) -> String {
    match slug {
        Some(slug) => format!(
            "https://www.curseforge.com/minecraft/mc-mods/{slug}/files/{file_id}"
        ),
        None => format!("https://www.curseforge.com/projects/{project_id}"),
    }
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
                    from_pack: true,
                    name: None,
                    icon_url: None,
                    summary: None,
                });
            }
            _ => {
                let reason = if file.url.is_none() {
                    "distribution disabled (url=None)"
                } else {
                    "no sha1 hash (md5-only or hashless)"
                };
                log::warn!(
                    "modpack: CF file project_id={} file_id={} '{}' routed to manual — {reason}",
                    manifest_file.project_id,
                    manifest_file.file_id,
                    file.file_name
                );
                manual.push(CfManualFile {
                    project_id: manifest_file.project_id,
                    file_id: manifest_file.file_id,
                    file_name: file.file_name.clone(),
                    // Numeric fallback; `resolve_and_build_cf_plan` rewrites this to the
                    // slug-based exact-file URL when it can fetch the project slug.
                    page_url: cf_file_page_url(
                        None,
                        manifest_file.project_id,
                        manifest_file.file_id,
                    ),
                    expected_sha1: sha1.cloned(),
                    size: file.size,
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
            Err(e) => {
                log::warn!(
                    "modpack: resolve_and_build_cf_plan — could not resolve project_id={} file_id={}: {e}",
                    entry.project_id, entry.file_id
                );
                failed.push(CfResolveFailure {
                    project_id: entry.project_id,
                    file_id: entry.file_id,
                    reason: e.to_string(),
                });
            }
        }
    }

    let mut plan = build_cf_pack_plan(&resolved, mc_dir)?;
    plan.failed = failed;

    // Slug enrichment (Pillar 1): only the manual entries need an exact file-page URL,
    // and the slug lives on the parent mod (`/v1/mods/{id}`) which `get_file` does not
    // fetch. Look it up once per distinct project (api-frugality) and rewrite `page_url`.
    // A failed/absent slug leaves the numeric fallback already set by `build_cf_pack_plan`.
    let mut slug_cache: BTreeMap<u64, Option<String>> = BTreeMap::new();
    for entry in &mut plan.manual {
        let slug = match slug_cache.get(&entry.project_id) {
            Some(cached) => cached.clone(),
            None => {
                let fetched = provider
                    .get_mod_slug(client, &entry.project_id.to_string())
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!(
                            "modpack: get_mod_slug failed for project_id={}: {e}",
                            entry.project_id
                        );
                        None
                    });
                slug_cache.insert(entry.project_id, fetched.clone());
                fetched
            }
        };
        if slug.is_some() {
            entry.page_url =
                cf_file_page_url(slug.as_deref(), entry.project_id, entry.file_id);
        }
    }

    Ok(plan)
}

// ── FTB pack planner ──────────────────────────────────────────────────────────

/// The result of [`build_ftb_pack_plan`] — same shape family as [`CfPackPlan`], so
/// it feeds the existing `remap_to_staging`/`promote_staging`/`execute_plan_cancellable`
/// pipeline unchanged.
#[derive(Debug, Clone)]
pub struct FtbPackPlan {
    /// Download items ready for the engine (FTB-CDN + resolved CF files).
    pub items: Vec<DownloadItem>,
    /// Mod entries for files that auto-resolved to a download (`type=="mod"`).
    pub mods: Vec<ModEntry>,
    /// CF-referenced files the user must download manually (distribution disabled).
    pub manual: Vec<CfManualFile>,
    /// Files skipped (`serveronly`, or neither url nor CF ref).
    pub skipped: Vec<String>,
    /// CF-referenced files whose `get_file` resolution itself failed.
    pub failed: Vec<CfResolveFailure>,
}

/// Dest path for an FTB manifest file: strip a leading `./` / `/` from `path`,
/// join `name`, forward-slash relative (fed to `validate_relative_path`).
pub fn ftb_dest_path(path: &str, name: &str) -> String {
    let p = path
        .trim_start_matches("./")
        .trim_start_matches('/')
        .trim_end_matches('/');
    if p.is_empty() {
        name.to_string()
    } else {
        format!("{p}/{name}")
    }
}

/// Build an [`FtbPackPlan`] from a version manifest + the resolved CF-referenced files.
///
/// `resolved_cf` maps `(curseforge.project, curseforge.file)` → the `VersionFile` from
/// `CurseForgeProvider::get_file`. FTB-hosted files (non-empty `url`) use their own
/// url+sha1; CF-referenced files use the resolved `VersionFile`, falling back to a
/// `CfManualFile` when distribution is disabled (url=None) — reusing the CF manual
/// pipeline verbatim. `serveronly` files are skipped; `optional` files are included.
/// CF refs absent from `resolved_cf` (resolution failed upstream) are skipped here and
/// recorded in `failed` by [`resolve_and_build_ftb_plan`].
///
/// # Errors
/// - [`ModpackError::UnsafePath`] — a computed dest path is unsafe (`..`, absolute, …).
pub fn build_ftb_pack_plan(
    manifest: &crate::core::ftb::FtbVersionManifest,
    resolved_cf: &std::collections::HashMap<(u64, u64), crate::core::providers::VersionFile>,
    mc_dir: &Path,
) -> Result<FtbPackPlan, ModpackError> {
    let mut items: Vec<DownloadItem> = Vec::new();
    let mut mods: Vec<ModEntry> = Vec::new();
    let mut manual: Vec<CfManualFile> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for file in &manifest.files {
        if file.serveronly {
            skipped.push(file.name.clone());
            continue;
        }

        let rel = ftb_dest_path(&file.path, &file.name);
        validate_relative_path(&rel)?;
        let dest = mc_dir.join(&rel);

        if !file.url.is_empty() {
            // FTB-CDN hosted file.
            let expected_hash = if file.sha1.is_empty() {
                None
            } else {
                Some(ExpectedHash::Sha1(file.sha1.clone()))
            };
            items.push(DownloadItem {
                url: file.url.clone(),
                dest,
                expected_hash,
                size: if file.size > 0 { Some(file.size) } else { None },
            });
            // FTB-hosted jars have no provider project/version id; record a bare
            // pack-managed entry only for actual mods (config/script/resource files
            // are pure download items, like mrpack overrides).
            if file.file_type.eq_ignore_ascii_case("mod") {
                let mut hashes = BTreeMap::new();
                if !file.sha1.is_empty() {
                    hashes.insert("sha1".to_string(), file.sha1.clone());
                }
                mods.push(ModEntry {
                    provider: "ftb".to_string(),
                    project_id: String::new(),
                    version_id: String::new(),
                    file_name: file.name.clone(),
                    hashes,
                    enabled: true,
                    side: "both".to_string(),
                    from_pack: true,
                    name: None,
                    icon_url: None,
                    summary: None,
                });
            }
        } else if let Some(cf) = &file.curseforge {
            // CurseForge-referenced — resolved out-of-band via `get_file`.
            match resolved_cf.get(&(cf.project, cf.file)) {
                Some(vf) => {
                    let sha1 = vf.hashes.get("sha1");
                    match (&vf.url, sha1) {
                        (Some(url), Some(hash)) => {
                            items.push(DownloadItem {
                                url: url.clone(),
                                dest,
                                expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
                                size: vf.size,
                            });
                            mods.push(ModEntry {
                                provider: "curseforge".to_string(),
                                project_id: cf.project.to_string(),
                                version_id: cf.file.to_string(),
                                file_name: file.name.clone(),
                                hashes: vf.hashes.clone().into_iter().collect(),
                                enabled: true,
                                side: "both".to_string(),
                                from_pack: true,
                                name: None,
                                icon_url: None,
                                summary: None,
                            });
                        }
                        // Distribution disabled / hashless → manual (reuse CF pipeline).
                        _ => {
                            manual.push(CfManualFile {
                                project_id: cf.project,
                                file_id: cf.file,
                                file_name: file.name.clone(),
                                page_url: cf_file_page_url(None, cf.project, cf.file),
                                expected_sha1: vf.hashes.get("sha1").cloned(),
                                size: vf.size,
                            });
                        }
                    }
                }
                // Resolution failed upstream (recorded in `failed`); skip here.
                None => {
                    log::warn!(
                        "modpack: FTB CF-referenced file '{}' (project={} file={}) not resolved — counted as failed",
                        file.name, cf.project, cf.file
                    );
                }
            }
        } else {
            // Neither a direct url nor a CF ref — nothing to download.
            log::warn!(
                "modpack: FTB file '{}' has neither url nor curseforge ref — skipped",
                file.name
            );
            skipped.push(file.name.clone());
        }
    }

    Ok(FtbPackPlan {
        items,
        mods,
        manual,
        skipped,
        failed: Vec::new(),
    })
}

/// Resolve a version manifest's CF-referenced files via `CurseForgeProvider::get_file`,
/// then build the [`FtbPackPlan`]. The async seam (mirrors [`resolve_and_build_cf_plan`]);
/// injectable `ProviderHttpClient` → mock-testable, no live network.
///
/// # Errors
/// - [`ModpackError::ResolverKeyMissing`] — no CF API key (FTB jars are CF-hosted, so
///   nothing can resolve; abort rather than produce an all-manual pack).
/// - [`ModpackError::UnsafePath`] — propagated from [`build_ftb_pack_plan`].
pub async fn resolve_and_build_ftb_plan(
    cf_provider: &crate::core::curseforge::CurseForgeProvider,
    client: &dyn crate::core::providers::ProviderHttpClient,
    manifest: &crate::core::ftb::FtbVersionManifest,
    mc_dir: &Path,
) -> Result<FtbPackPlan, ModpackError> {
    use std::collections::{HashMap, HashSet};

    let mut resolved_cf: HashMap<(u64, u64), crate::core::providers::VersionFile> = HashMap::new();
    let mut failed: Vec<CfResolveFailure> = Vec::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();

    for file in &manifest.files {
        if file.serveronly || !file.url.is_empty() {
            continue;
        }
        let Some(cf) = &file.curseforge else { continue };
        let key = (cf.project, cf.file);
        if !seen.insert(key) {
            continue; // resolve each distinct CF file once
        }
        match cf_provider
            .get_file(client, cf.project as u32, cf.file as u32)
            .await
        {
            Ok(vf) => {
                resolved_cf.insert(key, vf);
            }
            Err(crate::core::providers::ProviderError::KeyMissing) => {
                return Err(ModpackError::ResolverKeyMissing);
            }
            Err(e) => {
                log::warn!(
                    "modpack: resolve_and_build_ftb_plan — could not resolve CF project={} file={}: {e}",
                    cf.project, cf.file
                );
                failed.push(CfResolveFailure {
                    project_id: cf.project,
                    file_id: cf.file,
                    reason: e.to_string(),
                });
            }
        }
    }

    let mut plan = build_ftb_pack_plan(manifest, &resolved_cf, mc_dir)?;
    plan.failed = failed;
    Ok(plan)
}

// ── ATL pack planner ──────────────────────────────────────────────────────────

/// The result of [`build_atl_pack_plan`].
///
/// Mirrors the [`FtbPackPlan`]/[`CfPackPlan`] family so it feeds the same
/// `remap_to_staging`/`promote_staging`/`execute_plan_cancellable` pipeline.
#[derive(Debug, Clone)]
pub struct AtlPackPlan {
    /// Download items ready for the engine (ATL CDN + direct-URL files).
    pub items: Vec<DownloadItem>,
    /// Mod entries for files that auto-resolved to a download (mods-family types).
    pub mods: Vec<ModEntry>,
    /// Files requiring manual download (`download == "browser"`; rare — O-2).
    pub manual: Vec<CfManualFile>,
    /// Files skipped: legacy/exotic types (`decomp`, `extract`, …) or unknown
    /// `download` values.
    pub skipped: Vec<String>,
}

/// ATL mod types that map to the `mods/` folder (and receive a `ModEntry`).
const ATL_MODS_FAMILY: &[&str] = &[
    "mods",
    "dependency",
    "depandency", // real ATL typo seen in the wild
    "coremods",
    "ic2lib",
    "denlib",
    "flan",
    "plugins",
];

/// ATL mod types that cannot be installed automatically.
/// The planner silently records them in [`AtlPackPlan::skipped`].
const ATL_LEGACY_TYPES: &[&str] = &[
    "extract",
    "decomp",
    "texturepackextract",
    "resourcepackextract",
    "millenaire",
];

/// Returns `true` when `mod_type` (compared case-insensitively) belongs to the
/// mods-family set that maps to `mods/` and gets a `ModEntry`.
fn is_atl_mods_family(mod_type: &str) -> bool {
    let t = mod_type.to_ascii_lowercase();
    ATL_MODS_FAMILY.iter().any(|&s| s == t.as_str())
}

/// Compute the relative dest path for an ATLauncher mod entry.
///
/// - If `m.path` is non-empty the path override is normalised (strips leading
///   `./`, `/` and trailing `/`) and joined with `m.file`.
/// - Otherwise `m.mod_type` is mapped to its standard folder.
/// - Legacy/exotic types (`decomp`, `extract`, …) return `Ok(None)` — the caller
///   records them in [`AtlPackPlan::skipped`] and continues.
/// - Unknown types also return `Ok(None)`.
/// - A leading `/` on a `path` override is stripped (normalised to a relative
///   path, mirroring `ftb_dest_path`), so a unix-absolute override resolves
///   inside the instance dir rather than escaping it.
/// - `..` components and Windows drive-letter prefixes (`C:\…`) are rejected
///   with `Err(ModpackError::UnsafePath)` (via `validate_relative_path`).
pub fn atl_dest_path(m: &AtlMod) -> Result<Option<String>, ModpackError> {
    let rel: String = if !m.path.is_empty() {
        // Normalise path override: strip leading "./" / "/" and trailing "/",
        // then join with the filename (mirrors ftb_dest_path normalisation).
        let base = m
            .path
            .trim_start_matches("./")
            .trim_start_matches('/')
            .trim_end_matches('/');
        if base.is_empty() {
            m.file.clone()
        } else {
            format!("{base}/{}", m.file)
        }
    } else {
        let t = m.mod_type.to_ascii_lowercase();
        let ts = t.as_str();
        let folder: &str = if ATL_MODS_FAMILY.iter().any(|&s| s == ts) {
            "mods"
        } else if ts == "resourcepack" || ts == "texturepack" {
            "resourcepacks"
        } else if ts == "shaderpack" {
            "shaderpacks"
        } else if ts == "datapack" {
            "datapacks"
        } else if ts == "jar" || ts == "forge" || ts == "mcpc" {
            "jarmods"
        } else if ATL_LEGACY_TYPES.iter().any(|&s| s == ts) {
            return Ok(None); // legacy type → skip
        } else {
            return Ok(None); // unknown type → skip
        };
        format!("{folder}/{}", m.file)
    };

    validate_relative_path(&rel)?;
    Ok(Some(rel))
}

/// Build an [`AtlPackPlan`] from a parsed [`AtlConfigsManifest`].
///
/// `cdn_base` is the ATL CDN root (e.g.
/// `"https://download.nodecdn.net/containers/atl"`). `server` mod URLs are
/// expanded as `{cdn_base}/{m.url}`. `direct` mod URLs are used verbatim.
/// `browser` mods go to the pending-manual pipeline (O-2). `optional` mods are
/// included by default (O-4). `client == false` mods are silently dropped.
///
/// # Errors
/// - [`ModpackError::UnsafePath`] — a computed dest path contains `..`, is
///   absolute, or has a Windows drive-letter prefix.
pub fn build_atl_pack_plan(
    manifest: &AtlConfigsManifest,
    cdn_base: &str,
    mc_dir: &Path,
) -> Result<AtlPackPlan, ModpackError> {
    let mut items: Vec<DownloadItem> = Vec::new();
    let mut mods: Vec<ModEntry> = Vec::new();
    let mut manual: Vec<CfManualFile> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    let cdn = cdn_base.trim_end_matches('/');

    for m in &manifest.mods {
        // Skip server-only mods silently (mirrors FTB serveronly behaviour).
        if !m.client {
            continue;
        }

        // Compute dest; legacy/unknown types → push to skipped and continue.
        let rel = match atl_dest_path(m)? {
            Some(r) => r,
            None => {
                skipped.push(m.file.clone());
                continue;
            }
        };
        let dest = mc_dir.join(&rel);
        let is_mods_fam = is_atl_mods_family(&m.mod_type);

        match m.download.as_str() {
            "server" => {
                let url = format!("{cdn}/{}", m.url);
                let expected_hash =
                    (!m.md5.is_empty()).then(|| ExpectedHash::Md5(m.md5.clone()));
                let size = (m.filesize > 0).then_some(m.filesize);
                items.push(DownloadItem { url, dest, expected_hash, size });
                if is_mods_fam {
                    mods.push(make_atl_mod_entry(m));
                }
            }
            "direct" => {
                let expected_hash =
                    (!m.md5.is_empty()).then(|| ExpectedHash::Md5(m.md5.clone()));
                let size = (m.filesize > 0).then_some(m.filesize);
                items.push(DownloadItem {
                    url: m.url.clone(),
                    dest,
                    expected_hash,
                    size,
                });
                if is_mods_fam {
                    mods.push(make_atl_mod_entry(m));
                }
            }
            "browser" => {
                // Rare — map to the pending-manual pipeline (O-2). No ModEntry:
                // mirrors the CF manual-file pipeline (mod not yet downloaded).
                let size = (m.filesize > 0).then_some(m.filesize);
                manual.push(CfManualFile {
                    project_id: 0,
                    file_id: 0,
                    file_name: m.file.clone(),
                    page_url: m.url.clone(),
                    expected_sha1: None,
                    size,
                });
            }
            other => {
                log::warn!(
                    "modpack: ATL mod '{}' has unknown download type '{}' — skipped",
                    m.file,
                    other
                );
                skipped.push(m.file.clone());
            }
        }
    }

    Ok(AtlPackPlan { items, mods, manual, skipped })
}

/// Build a [`ModEntry`] for an ATLauncher mod jar.
fn make_atl_mod_entry(m: &AtlMod) -> ModEntry {
    let mut hashes = BTreeMap::new();
    if !m.md5.is_empty() {
        hashes.insert("md5".to_string(), m.md5.clone());
    }
    ModEntry {
        provider: "atlauncher".to_string(),
        project_id: m.curse_id.map(|id| id.to_string()).unwrap_or_default(),
        version_id: String::new(),
        file_name: m.file.clone(),
        hashes,
        enabled: true,
        side: "both".to_string(),
        from_pack: true,
        name: (!m.name.is_empty()).then(|| m.name.clone()),
        icon_url: None,
        summary: None,
    }
}

/// Extract a `Configs.zip` archive **root-relative** into `mc_dir`.
///
/// Every entry is taken as a path relative to `mc_dir` — no prefix is stripped
/// (the archive itself is already root-relative, unlike `.mrpack` overrides which
/// have an `overrides/` wrapper).
///
/// Extraction is zip-slip-safe: every entry path must pass
/// [`validate_relative_path`] and the structural containment check inside
/// [`extract_prefix`].
///
/// Returns the total number of files extracted (directory entries not counted).
///
/// The caller is responsible for skipping this call when
/// `manifest.configs.is_none()` (no archive exists for vanilla packs or packs
/// with `noConfigs`).
///
/// # Errors
/// - [`ModpackError::UnsafePath`] — an entry path is structurally unsafe.
/// - [`ModpackError::ZipSlip`] — an entry resolves outside `mc_dir`.
/// - [`ModpackError::Io`] — an I/O error during extraction.
/// - [`ModpackError::Zip`] — a zip-crate error reading the archive.
pub fn extract_atl_configs<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    mc_dir: &Path,
) -> Result<u32, ModpackError> {
    // An empty prefix causes every entry to be taken as root-relative —
    // exactly the layout expected from a Configs.zip.
    extract_prefix(archive, mc_dir, "")
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
    /// Provider-specific version identifier (e.g. Modrinth base62 id or CF numeric file id).
    /// Used to populate `Instance.source.file_id` on the Browse install path.
    pub version_id: String,
    /// Human-readable version display name (e.g. `"Pack v1.0"`).
    /// Used to populate `Instance.source.pack_version` on the Browse install path.
    pub version_name: String,
}

/// Resolve a modpack project to a version's primary (or first) file.
///
/// Calls `provider.get_versions(client, project_id, None, None)` — no mc/loader
/// filter, because the pack itself defines those.
///
/// Version selection:
/// - `target_version_id = None` ⇒ latest = the **first version returned**; both
///   Modrinth and CurseForge return versions newest-first (slice-C / D1 behavior).
/// - `target_version_id = Some(id)` ⇒ the version whose `id` matches exactly.
///   If no version matches, returns [`ModpackError::VersionNotFound`] naming the id.
///
/// File selection: `files.iter().find(|f| f.primary)`, falling back to the first
/// file if none is flagged primary.
///
/// `url: None` on the selected file is **not** an error — it is returned as-is;
/// the caller (C3) is responsible for routing that to a manual-download outcome.
///
/// # Errors
/// - [`ModpackError::NoVersions`] — the provider returned an empty version list.
/// - [`ModpackError::NoFiles`] — the selected version carries no files.
/// - [`ModpackError::VersionNotFound`] — `target_version_id` was given but not found.
/// - Any [`crate::core::providers::ProviderError`] wrapped in [`ModpackError::ResolverError`].
pub async fn resolve_pack_file(
    provider: &dyn crate::core::providers::ModProvider,
    client: &dyn crate::core::providers::ProviderHttpClient,
    project_id: &str,
    target_version_id: Option<&str>,
) -> Result<ResolvedPackFile, ModpackError> {
    let versions = provider
        .get_versions(client, project_id, None, None)
        .await
        .map_err(ModpackError::ResolverError)?;

    let chosen = match target_version_id {
        None => versions.into_iter().next().ok_or(ModpackError::NoVersions)?,
        Some(id) => versions
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| ModpackError::VersionNotFound(id.to_string()))?,
    };

    let version_id = chosen.id.clone();
    let version_name = chosen.name.clone();

    let file = chosen
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| chosen.files.first())
        .ok_or(ModpackError::NoFiles)?
        .clone();

    log::info!(
        "modpack: resolve_pack_file — resolved project_id={project_id} version_id={version_id} file='{}'",
        file.file_name
    );
    Ok(ResolvedPackFile {
        url: file.url,
        file_name: file.file_name,
        provider: chosen.provider,
        version_id,
        version_name,
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
                from_pack: true,
                name: None,
                icon_url: None,
                summary: None,
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

// ── D3: Update-reconcile helper ──────────────────────────────────────────────

/// The result of [`plan_pack_update`]: mods to delete from disk and the merged
/// `mods[]` list to write back to the instance manifest.
#[derive(Debug, Clone)]
pub struct PackUpdatePlan {
    /// Old pack `ModEntry`s whose `file_name` is absent from `new_pack_mods`.
    /// The executor deletes their jar (and `.disabled` twin) from `mc/mods/`.
    pub to_remove: Vec<crate::core::instances::ModEntry>,
    /// The new `mods[]` to write to the manifest.
    ///
    /// Keyed by `file_name` (unique within `mods/`):
    /// - `new_pack_mods` entries are always included (pack wins).
    /// - User-added entries (`from_pack == false`) whose `file_name` is NOT in
    ///   `new_pack_mods` are kept verbatim.
    /// - A user entry whose `file_name` collides with a pack entry is replaced
    ///   by the pack entry (one record, `from_pack = true`).
    pub merged: Vec<crate::core::instances::ModEntry>,
}

/// Compute the mod-level delta between the current instance mods and the new pack plan.
///
/// - `current_mods` — the instance's existing `mods[]` (may include both pack and
///   user-added entries).
/// - `new_pack_mods` — the `mods[]` from the new pack plan; all must have
///   `from_pack == true` (guaranteed by [`build_pack_plan`] / [`build_cf_pack_plan`]).
///
/// # Semantics
///
/// Key by `file_name` (unique within `mods/`):
///
/// - **to_remove**: `current_mods` entries with `from_pack == true` whose
///   `file_name` is absent from `new_pack_mods`.  User mods are NEVER removed.
/// - **kept user mods**: `current_mods` entries with `from_pack == false` whose
///   `file_name` is NOT in `new_pack_mods` (user additions the new pack does not
///   overwrite).
/// - **merged**: `kept_user_mods ++ new_pack_mods`.  A pack entry replaces any
///   same-`file_name` user or old-pack entry; result has one record per
///   `file_name`, winner has `from_pack == true`.
///
/// Pure — no I/O.
pub fn plan_pack_update(
    current_mods: &[crate::core::instances::ModEntry],
    new_pack_mods: &[crate::core::instances::ModEntry],
) -> PackUpdatePlan {
    use std::collections::HashSet;

    // Set of file_names covered by the new plan.
    let new_names: HashSet<&str> = new_pack_mods.iter().map(|m| m.file_name.as_str()).collect();

    // Old pack mods that disappeared from the new plan → delete their jars.
    let to_remove: Vec<crate::core::instances::ModEntry> = current_mods
        .iter()
        .filter(|m| m.from_pack && !new_names.contains(m.file_name.as_str()))
        .cloned()
        .collect();

    // Kept user mods: from_pack=false, file_name not overridden by a pack entry.
    let kept_user: Vec<crate::core::instances::ModEntry> = current_mods
        .iter()
        .filter(|m| !m.from_pack && !new_names.contains(m.file_name.as_str()))
        .cloned()
        .collect();

    // merged = kept user mods ++ new pack mods
    let mut merged = kept_user;
    merged.extend(new_pack_mods.iter().cloned());

    PackUpdatePlan { to_remove, merged }
}

// ── CP-3: Staging helpers ────────────────────────────────────────────────────

/// Remap a list of [`DownloadItem`]s so their `dest` paths point into
/// `staging_dir` instead of `target_dir`.
///
/// Each item whose `dest` begins with `target_dir` has that prefix replaced by
/// `staging_dir`, preserving the relative sub-path. Items whose `dest` does NOT
/// begin with `target_dir` (e.g. shared cache items) are returned unchanged —
/// those download in-place and are not staged.
///
/// Use this before passing a plan's items to [`execute_plan_cancellable`] so
/// instance-bound files land in the staging dir and can be atomically promoted
/// or discarded after downloads finish.
pub fn remap_to_staging(
    items: Vec<DownloadItem>,
    target_dir: &Path,
    staging_dir: &Path,
) -> Vec<DownloadItem> {
    items
        .into_iter()
        .map(|mut item| {
            if let Ok(rel) = item.dest.strip_prefix(target_dir) {
                item.dest = staging_dir.join(rel);
            }
            item
        })
        .collect()
}

// ── CP-3: Stage-and-promote helper ───────────────────────────────────────────

/// Promote all files from `staging_dir` into `target_dir` using atomic renames.
///
/// The two directories **must reside on the same volume** so that `rename` is
/// atomic at the OS level. This is guaranteed by choosing the staging dir as a
/// sibling of the instance dir (e.g. `<instance_dir>/.staging-<task_id>/`).
///
/// For each file found (recursively) under `staging_dir`, the corresponding
/// `target_dir`-relative path is computed and the file is renamed into place.
/// Parent directories under `target_dir` are created as needed.
///
/// On success `staging_dir` is left empty (files moved, not copied).
/// The caller is responsible for removing the empty staging dir afterwards
/// (success path) or `remove_dir_all` on the full staging dir (cancel/fail path).
///
/// # Errors
/// Returns the first I/O error encountered during rename or dir-creation.
pub fn promote_staging(staging_dir: &Path, target_dir: &Path) -> Result<(), io::Error> {
    promote_staging_recursive(staging_dir, staging_dir, target_dir)
}

fn promote_staging_recursive(
    root: &Path,
    current: &Path,
    target_dir: &Path,
) -> Result<(), io::Error> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).expect("path is under root");
        let dest = target_dir.join(rel);

        let ft = entry.file_type()?;
        if ft.is_dir() {
            std::fs::create_dir_all(&dest)?;
            promote_staging_recursive(root, &path, target_dir)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&path, &dest)?;
        }
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "modpack_tests.rs"]
mod tests;
