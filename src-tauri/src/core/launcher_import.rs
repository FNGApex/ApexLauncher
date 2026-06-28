//! Prism / MultiMC / PolyMC launcher instance importer — pure parse/plan layer.
//!
//! All functions in this module are **pure** (no I/O, no Tauri, no network).
//! Higher-level orchestration (copy, promote, Tauri job) lives in `lib.rs` (CP-5+).
//!
//! # Checkpoints implemented here
//! - **CP-1** — `instance.cfg` parser → [`PrismInstanceCfg`].
//! - **CP-2** — `mmc-pack.json` parser + uid→loader mapping → [`MmcPack`].

use serde::Deserialize;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors produced by the launcher-import parse pipeline.
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "launcher_import_tests.rs"]
mod tests;
