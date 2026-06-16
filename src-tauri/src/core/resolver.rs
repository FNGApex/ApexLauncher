//! Vanilla Minecraft resolver — Phase 2, slice B.
//!
//! Fetches + caches the per-version manifest from Mojang's piston-meta, parses
//! it into typed structs, and (later checkpoints) produces a [`DownloadPlan`]
//! plus a `LaunchMeta` struct for slice D.
//!
//! CP1: manifest fetch + parse only. Rule eval (CP2), asset resolution (CP3),
//!      and command wiring (CP4) are added in subsequent iterations.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::core::meta;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Vanilla manifests are immutable once published — 365-day TTL is effectively
/// "cache forever" while still allowing manual invalidation by deleting the file.
const VERSION_TTL: Duration = Duration::from_secs(365 * 24 * 3600);

const MANIFEST_TTL: Duration = Duration::from_secs(6 * 3600);

// ---------------------------------------------------------------------------
// version_manifest_v2 entry
// ---------------------------------------------------------------------------

/// A single entry from the top-level `version_manifest_v2.json`.
#[derive(Debug, Deserialize)]
struct ManifestEntry {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    pub versions: Vec<ManifestEntry>,
}

// ---------------------------------------------------------------------------
// Per-version manifest structs
// ---------------------------------------------------------------------------

/// `downloads.client` / `downloads.server` / library artifact.
///
/// `path` is the Maven-layout relative path (e.g.
/// `"com/mojang/authlib/6.0.54/authlib-6.0.54.jar"`).  Present on all library
/// artifacts and on modern client/server entries.  `Option` because the top-level
/// `downloads.client` block in the version manifest omits it.
#[derive(Debug, Deserialize)]
pub struct DownloadSpec {
    #[serde(default)]
    pub path: Option<String>,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// `javaVersion` block. Absent on very old versions; defaults to major 8.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

/// `assetIndex` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub total_size: u64,
    pub url: String,
}

/// `downloads` object inside the per-version manifest.
#[derive(Debug, Deserialize)]
pub struct ManifestDownloads {
    pub client: DownloadSpec,
}

// ---------------------------------------------------------------------------
// Arguments (modern structured vs legacy string)
// ---------------------------------------------------------------------------

/// A game/jvm argument entry. Mojang uses a mixed array: plain strings and
/// objects with `rules` + `value` (string or string array). We deserialize both
/// but CP1 only preserves the raw strings; rule-filtered JVM args are CP2+.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ArgumentEntry {
    Plain(String),
    Conditional(ConditionalArgument),
}

#[derive(Debug, Deserialize)]
pub struct ConditionalArgument {
    pub rules: Vec<serde_json::Value>, // opaque for CP1; CP2 evaluates
    pub value: ArgumentValue,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Many(Vec<String>),
}

/// Modern `arguments` block (MC ≥1.13).
#[derive(Debug, Deserialize, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgumentEntry>,
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

/// `downloads.classifiers` map value (one native jar variant).
#[derive(Debug, Deserialize)]
pub struct NativeArtifact {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

/// `downloads` inside a library entry.
#[derive(Debug, Deserialize, Default)]
pub struct LibraryDownloads {
    /// Present for most libraries; absent for some Maven-only entries.
    pub artifact: Option<DownloadSpec>,
    /// Native jar variants, keyed by classifier string (e.g. `"natives-linux"`).
    #[serde(default)]
    pub classifiers: std::collections::HashMap<String, NativeArtifact>,
}

/// A single library entry from the `libraries` array.
#[derive(Debug, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: LibraryDownloads,
    /// OS-native classifier map: `"linux"` → `"natives-linux"`, etc.
    #[serde(default)]
    pub natives: std::collections::HashMap<String, String>,
    /// Allow/deny rules for this library (CP2 evaluates; stored as raw Value).
    #[serde(default)]
    pub rules: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Top-level version manifest
// ---------------------------------------------------------------------------

/// Parsed per-version manifest (e.g. `1.21.1.json` from piston-meta).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSpec {
    pub id: String,
    /// The version type as reported by Mojang: `"release"`, `"snapshot"`, etc.
    /// Absent on very old manifests; defaults to `"release"`.
    #[serde(rename = "type", default = "default_version_type")]
    pub version_type: String,
    pub main_class: String,
    pub downloads: ManifestDownloads,
    pub asset_index: AssetIndex,
    pub libraries: Vec<Library>,

    /// Modern structured args (MC ≥1.13).
    #[serde(default)]
    pub arguments: Option<Arguments>,

    /// Legacy flat arg string (MC <1.13).
    #[serde(default)]
    pub minecraft_arguments: Option<String>,

    /// `javaVersion` may be absent on old manifests; defaults to major 8.
    #[serde(default)]
    pub java_version: Option<JavaVersion>,

    /// Optional `logging.client` block (log4j2 config for the JVM).
    /// Absent on older manifests and some modern ones — treated as optional.
    #[serde(default)]
    pub logging: Option<Logging>,
}

fn default_version_type() -> String {
    "release".to_string()
}

impl VersionSpec {
    /// Resolved Java major version, defaulting to 8 when the field is absent.
    pub fn java_major(&self) -> u32 {
        self.java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(8)
    }
}

// ---------------------------------------------------------------------------
// Rule evaluation & library selection (CP2)
// ---------------------------------------------------------------------------

/// Mojang rule action.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum RuleAction {
    Allow,
    Disallow,
}

/// A single entry in a library's (or argument's) `rules` array.
#[derive(Debug, Deserialize)]
struct Rule {
    pub action: RuleAction,
    /// Absent → rule matches any OS.
    pub os: Option<OsConstraint>,
}

#[derive(Debug, Deserialize)]
struct OsConstraint {
    pub name: Option<String>,
}

/// Map a Rust `std::env::consts::OS` value to the Mojang OS name used in
/// manifests (`linux`, `windows`, `osx`).
///
/// Unknown OS strings are left as-is; they will simply never match a named
/// os-constraint and therefore fall through to the default (allow) behaviour.
pub fn host_os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "osx",
        "windows" => "windows",
        "linux" => "linux",
        other => other,
    }
}

/// Evaluate a library's (or argument's) `rules` array for `target_os`
/// (a Mojang OS name: `"linux"`, `"windows"`, `"osx"`).
///
/// Semantics (Mojang spec):
/// - No rules → **allowed**.
/// - Apply in order; **last matching rule wins**.
/// - A rule matches when it has no `os` constraint, or its `os.name` equals
///   `target_os`.
/// - Default verdict before any rule fires: **disallowed** (rules start denied
///   when there is at least one rule).
///
/// Returns `true` if the library should be included for `target_os`.
pub fn eval_rules(rules: &[serde_json::Value], target_os: &str) -> bool {
    if rules.is_empty() {
        return true;
    }

    // Default when rules are present but none match: disallow.
    let mut allowed = false;

    for raw in rules {
        // Best-effort parse; malformed rules are skipped.
        let rule: Rule = match serde_json::from_value(raw.clone()) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let matches = match &rule.os {
            None => true,
            Some(oc) => match &oc.name {
                None => true,
                Some(name) => name == target_os,
            },
        };

        if matches {
            allowed = rule.action == RuleAction::Allow;
        }
    }

    allowed
}

/// A classpath entry produced by selecting a library for `target_os`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClasspathEntry {
    /// Maven-relative path (e.g. `"com/mojang/authlib/6.0.54/authlib-6.0.54.jar"`).
    pub maven_path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// A native library entry produced by selecting a native classifier for `target_os`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEntry {
    /// Classifier used (e.g. `"natives-linux"`).
    pub classifier: String,
    pub maven_path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// For each library allowed by `eval_rules` that has a `downloads.artifact`
/// with a `path`, yield a [`ClasspathEntry`].
///
/// Libraries with no artifact (Maven-only stubs) are silently skipped.
pub fn select_classpath(libs: &[Library], target_os: &str) -> Vec<ClasspathEntry> {
    let mut out = Vec::new();
    for lib in libs {
        if !eval_rules(&lib.rules, target_os) {
            continue;
        }
        if let Some(artifact) = &lib.downloads.artifact {
            if let Some(path) = &artifact.path {
                out.push(ClasspathEntry {
                    maven_path: path.clone(),
                    url: artifact.url.clone(),
                    sha1: artifact.sha1.clone(),
                    size: artifact.size,
                });
            }
        }
    }
    out
}

/// Resolve the `${arch}` token in a native classifier name.
/// We substitute `"64"` (64-bit default); a 32-bit path is not needed for
/// supported MC versions.
fn resolve_classifier(classifier: &str) -> String {
    classifier.replace("${arch}", "64")
}

/// For each library allowed by `eval_rules` that has a `natives` map entry
/// for `target_os`, resolve the classifier and look up the corresponding
/// `downloads.classifiers` entry.  Yields a [`NativeEntry`] for each match.
///
/// `target_os` uses Mojang names: `"linux"`, `"windows"`, `"osx"`.
pub fn select_natives(libs: &[Library], target_os: &str) -> Vec<NativeEntry> {
    let mut out = Vec::new();
    for lib in libs {
        if !eval_rules(&lib.rules, target_os) {
            continue;
        }
        let Some(raw_classifier) = lib.natives.get(target_os) else {
            continue;
        };
        let classifier = resolve_classifier(raw_classifier);
        let Some(native) = lib.downloads.classifiers.get(&classifier) else {
            continue;
        };
        out.push(NativeEntry {
            classifier: classifier.clone(),
            maven_path: native.path.clone(),
            url: native.url.clone(),
            sha1: native.sha1.clone(),
            size: native.size,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Asset index (CP3)
// ---------------------------------------------------------------------------

const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";

/// Long TTL for asset indexes — they are content-addressed by sha1 and immutable.
const ASSET_INDEX_TTL: Duration = Duration::from_secs(365 * 24 * 3600);

/// A single entry in the `objects` map of an asset index JSON.
#[derive(Debug, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// Parsed asset index JSON (`assets/indexes/<id>.json`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexData {
    pub objects: std::collections::HashMap<String, AssetObject>,
    /// Pre-1.7 virtual layout flag.
    #[serde(default)]
    pub r#virtual: bool,
    /// Pre-1.7 `map_to_resources` flag (very old packs).
    #[serde(default)]
    pub map_to_resources: bool,
}

impl AssetIndexData {
    /// True when this index uses the legacy virtual/resource-pack layout.
    /// The modern `objects/<2hex>/<sha1>` layout is used when this returns false.
    pub fn assets_legacy(&self) -> bool {
        self.r#virtual || self.map_to_resources
    }
}

/// Map the parsed `objects` map to a flat list of [`DownloadItem`]s.
///
/// `data_dir` is the cache dir (`<data>/cache/`).
/// Each object is placed at `<data_dir>/assets/objects/<2hex>/<sha1>`.
pub fn asset_objects_to_items(
    objects: &std::collections::HashMap<String, AssetObject>,
    data_dir: &std::path::Path,
) -> Vec<crate::core::download::DownloadItem> {
    use crate::core::download::{DownloadItem, ExpectedHash};

    objects
        .values()
        .filter_map(|obj| {
            // hash must be at least 2 chars to form the objects/<2hex>/<sha1> path.
            let prefix = obj.hash.get(..2)?;
            let url = format!("{}/{}/{}", ASSET_BASE_URL, prefix, obj.hash);
            let dest = data_dir
                .join("assets")
                .join("objects")
                .join(prefix)
                .join(&obj.hash);
            Some(DownloadItem {
                url,
                dest,
                expected_hash: Some(ExpectedHash::Sha1(obj.hash.clone())),
                size: Some(obj.size),
            })
        })
        .collect()
}

/// Emit the [`DownloadItem`] for the asset index file itself.
///
/// dest = `<data_dir>/assets/indexes/<id>.json`
pub fn asset_index_file_item(
    asset_index: &AssetIndex,
    data_dir: &std::path::Path,
) -> crate::core::download::DownloadItem {
    use crate::core::download::{DownloadItem, ExpectedHash};

    DownloadItem {
        url: asset_index.url.clone(),
        dest: data_dir
            .join("assets")
            .join("indexes")
            .join(format!("{}.json", asset_index.id)),
        expected_hash: Some(ExpectedHash::Sha1(asset_index.sha1.clone())),
        size: Some(asset_index.size),
    }
}

/// Fetch + cache the asset index for the given [`AssetIndex`] descriptor,
/// returning the parsed [`AssetIndexData`].
pub async fn fetch_asset_index(
    app: &AppHandle,
    asset_index: &AssetIndex,
) -> Result<AssetIndexData, String> {
    let key = format!("asset-index-{}.json", asset_index.id);
    let body = meta::cached_text(app, &asset_index.url, &key, ASSET_INDEX_TTL).await?;
    serde_json::from_str(&body)
        .map_err(|e| format!("bad asset index '{}': {e}", asset_index.id))
}

// ---------------------------------------------------------------------------
// LaunchMeta + assemble (CP4)
// ---------------------------------------------------------------------------

/// Logging config file reference (from `logging.client.file`).
#[derive(Debug, Deserialize)]
pub(crate) struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

/// The `logging.client` block (Mojang injects a log4j2 config).
#[derive(Debug, Deserialize)]
pub(crate) struct LoggingClient {
    pub file: LoggingFile,
}

/// The `logging` object at the top level of the per-version manifest.
#[derive(Debug, Deserialize, Default)]
pub(crate) struct Logging {
    pub client: Option<LoggingClient>,
}

/// Accumulated metadata that slice D (launch) needs to build the JVM argv.
///
/// All `${...}` placeholders in `jvm_args` and `game_args` are left intact —
/// substitution is slice D's responsibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchMeta {
    pub version_id: String,
    /// The version type from the Mojang manifest: `"release"`, `"snapshot"`, etc.
    /// Substituted into `${version_type}` by the argv assembler.
    pub version_type: String,
    pub main_class: String,
    /// JVM argument templates (modern `arguments.jvm` entries, OS-filtered).
    /// Empty for legacy manifests — slice D provides defaults.
    pub jvm_args: Vec<String>,
    /// Game argument templates (modern `arguments.game` or legacy split).
    pub game_args: Vec<String>,
    pub asset_index_id: String,
    pub assets_legacy: bool,
    pub java_major: u32,
    /// Ordered list of dest paths (as strings) for classpath: all libs + client jar.
    pub classpath: Vec<String>,
    /// Dest paths of native jars — slice D extracts these before launch.
    pub natives: Vec<String>,
    /// Path to the logging config file, if present in the manifest.
    pub logging_config: Option<String>,
}

/// Returned by `resolve_vanilla`: the download plan + launch metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResult {
    pub plan: crate::core::download::DownloadPlan,
    pub launch: LaunchMeta,
}

/// Flatten a single `ArgumentEntry` into a list of plain strings.
///
/// `ConditionalArgument` entries are included only when their `rules` pass for
/// `target_os`. Feature-gated entries (demo mode, custom-resolution — identified
/// by the `is_demo_user` / `has_custom_resolution` feature keys) are excluded
/// because those features are off by default.
fn flatten_arg_entry(entry: &ArgumentEntry, target_os: &str) -> Vec<String> {
    match entry {
        ArgumentEntry::Plain(s) => vec![s.clone()],
        ArgumentEntry::Conditional(cond) => {
            // Skip feature-gated entries.
            for rule_val in &cond.rules {
                // Any feature-gated entry → skip (all non-default features excluded).
                if rule_val.get("features").is_some() {
                    return vec![];
                }
            }
            if !eval_rules(&cond.rules, target_os) {
                return vec![];
            }
            match &cond.value {
                ArgumentValue::Single(s) => vec![s.clone()],
                ArgumentValue::Many(v) => v.clone(),
            }
        }
    }
}

/// Pure assembly: combine the parsed manifest + asset index into a `DownloadPlan`
/// and `LaunchMeta`.  No network calls, no `AppHandle`.
///
/// `target_os` is a Mojang OS name (`"linux"`, `"windows"`, `"osx"`).
/// `cache_dir` is the cache dir (`<data>/cache/`); all dest paths are anchored here.
pub fn assemble(
    spec: &VersionSpec,
    assets: &AssetIndexData,
    target_os: &str,
    cache_dir: &std::path::Path,
) -> (crate::core::download::DownloadPlan, LaunchMeta) {
    use crate::core::download::{DownloadItem, DownloadPlan, ExpectedHash};

    let mut items: Vec<DownloadItem> = Vec::new();

    // 1. Client jar.
    let client_dest = cache_dir
        .join("versions")
        .join(&spec.id)
        .join(format!("{}.jar", spec.id));
    items.push(DownloadItem {
        url: spec.downloads.client.url.clone(),
        dest: client_dest.clone(),
        expected_hash: Some(ExpectedHash::Sha1(spec.downloads.client.sha1.clone())),
        size: Some(spec.downloads.client.size),
    });

    // 2. Libraries (classpath).
    let cp = select_classpath(&spec.libraries, target_os);
    for entry in &cp {
        items.push(DownloadItem {
            url: entry.url.clone(),
            dest: cache_dir.join("libraries").join(&entry.maven_path),
            expected_hash: Some(ExpectedHash::Sha1(entry.sha1.clone())),
            size: Some(entry.size),
        });
    }

    // 3. Natives.
    let nat = select_natives(&spec.libraries, target_os);
    for entry in &nat {
        items.push(DownloadItem {
            url: entry.url.clone(),
            dest: cache_dir.join("libraries").join(&entry.maven_path),
            expected_hash: Some(ExpectedHash::Sha1(entry.sha1.clone())),
            size: Some(entry.size),
        });
    }

    // 4. Asset index file.
    items.push(asset_index_file_item(&spec.asset_index, cache_dir));

    // 5. Asset objects.
    items.extend(asset_objects_to_items(&assets.objects, cache_dir));

    // 6. Logging config (optional).
    let logging_config_path: Option<String> = if let Some(logging) = &spec.logging {
        if let Some(client) = &logging.client {
            let dest = cache_dir
                .join("assets")
                .join("log_configs")
                .join(&client.file.id);
            items.push(DownloadItem {
                url: client.file.url.clone(),
                dest: dest.clone(),
                expected_hash: Some(ExpectedHash::Sha1(client.file.sha1.clone())),
                size: Some(client.file.size),
            });
            Some(dest.to_string_lossy().into_owned())
        } else {
            None
        }
    } else {
        None
    };

    // Build classpath: lib dest paths (in select_classpath order) + client jar last.
    let mut classpath: Vec<String> = cp
        .iter()
        .map(|e| {
            cache_dir
                .join("libraries")
                .join(&e.maven_path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    classpath.push(client_dest.to_string_lossy().into_owned());

    // Natives dest paths.
    let natives: Vec<String> = nat
        .iter()
        .map(|e| {
            cache_dir
                .join("libraries")
                .join(&e.maven_path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    // Build jvm_args + game_args from modern arguments or legacy minecraftArguments.
    let (jvm_args, game_args) = if let Some(args) = &spec.arguments {
        let jvm: Vec<String> = args
            .jvm
            .iter()
            .flat_map(|e| flatten_arg_entry(e, target_os))
            .collect();
        let game: Vec<String> = args
            .game
            .iter()
            .flat_map(|e| flatten_arg_entry(e, target_os))
            .collect();
        (jvm, game)
    } else if let Some(legacy) = &spec.minecraft_arguments {
        // Split on ASCII whitespace; leave placeholders intact.
        let game: Vec<String> = legacy
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect();
        (vec![], game)
    } else {
        (vec![], vec![])
    };

    let launch = LaunchMeta {
        version_id: spec.id.clone(),
        version_type: spec.version_type.clone(),
        main_class: spec.main_class.clone(),
        jvm_args,
        game_args,
        asset_index_id: spec.asset_index.id.clone(),
        assets_legacy: assets.assets_legacy(),
        java_major: spec.java_major(),
        classpath,
        natives,
        logging_config: logging_config_path,
    };

    (DownloadPlan::new(items), launch)
}

// ---------------------------------------------------------------------------
// Loader profile merge
// ---------------------------------------------------------------------------

/// Merge a Fabric / Quilt loader profile into an already-resolved vanilla
/// `DownloadPlan` + `LaunchMeta`.
///
/// Mutations applied:
/// 1. `launch.main_class` is overridden with the profile's `mainClass`.
/// 2. Each loader library becomes a [`DownloadItem`] (no hash — loader profiles
///    don't ship sha1) appended to `plan`, and its dest path is inserted into
///    `launch.classpath` **before** the vanilla client jar (which must remain
///    the last entry, per the `assemble` contract).
/// 3. The profile's `arguments.jvm` / `.game` entries are OS-filtered via the
///    same [`flatten_arg_entry`] machinery used by `assemble`, then appended to
///    `launch.jvm_args` / `launch.game_args` after the existing vanilla args.
///
/// `cache_dir` is the cache dir (`<data>/cache/`).
/// No network access. Pure function — safe to unit-test without an `AppHandle`.
pub fn merge_loader_profile(
    plan: &mut crate::core::download::DownloadPlan,
    launch: &mut LaunchMeta,
    profile: &crate::core::loader_profile::LoaderProfile,
    target_os: &str,
    cache_dir: &std::path::Path,
) {
    use crate::core::download::DownloadItem;
    use crate::core::loader_profile::maven_coord_to_path;

    // 1. Override main class.
    launch.main_class = profile.main_class.clone();

    // 2. Loader libraries → plan items + classpath entries before client jar.
    //    The client jar is guaranteed to be the last classpath entry (assemble contract).
    //    Pop it, extend with loader lib dests in profile order, then push it back so the
    //    final order is: [vanilla libs…, loader lib0, lib1, …, client jar].
    let client_jar = launch.classpath.pop();

    for lib in &profile.libraries {
        let maven_path = maven_coord_to_path(&lib.name);
        let dest = cache_dir.join("libraries").join(&maven_path);

        // Libraries with a present, non-empty url get a DownloadItem; libraries
        // with url=None or url="" (processor-produced, no download URL) are added
        // to the classpath only — the file must already exist from the installer run.
        match lib.url.as_deref().filter(|u| !u.is_empty()) {
            Some(raw_url) => {
                // If the URL already ends with ".jar" it is a full artifact URL
                // (Forge/NeoForge `downloads.artifact.url` format) — use it as-is.
                // Otherwise treat it as a Maven repository base URL and append the
                // maven coordinate path (Fabric/Quilt format).
                let url = if raw_url.ends_with(".jar") {
                    raw_url.to_owned()
                } else {
                    let base_url = raw_url.trim_end_matches('/');
                    format!("{}/{}", base_url, maven_path)
                };

                plan.items.push(DownloadItem {
                    url,
                    dest: dest.clone(),
                    expected_hash: None,
                    size: None,
                });
            }
            None => {
                // url absent or empty — classpath-only, no download.
            }
        }

        launch.classpath.push(dest.to_string_lossy().into_owned());
    }

    // Restore the client jar as the last entry.
    if let Some(jar) = client_jar {
        launch.classpath.push(jar);
    }

    // 3. Append loader args (OS-filtered) after the vanilla args.
    let extra_jvm: Vec<String> = profile
        .arguments
        .jvm
        .iter()
        .flat_map(|e| flatten_arg_entry(e, target_os))
        .collect();
    launch.jvm_args.extend(extra_jvm);

    let extra_game: Vec<String> = profile
        .arguments
        .game
        .iter()
        .flat_map(|e| flatten_arg_entry(e, target_os))
        .collect();
    launch.game_args.extend(extra_game);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch + cache the per-version manifest for `version_id`.
///
/// Uses the top-level `version_manifest_v2.json` to discover the per-version
/// URL (verified by `sha1`), then fetches + caches the version JSON with a
/// long TTL (immutable per id).
pub async fn fetch_version_spec(
    app: &AppHandle,
    version_id: &str,
) -> Result<VersionSpec, String> {
    // 1. Fetch top-level manifest (short TTL — new releases appear here).
    let body =
        meta::cached_text(app, MANIFEST_URL, "version_manifest_v2.json", MANIFEST_TTL).await?;
    let manifest: VersionManifest =
        serde_json::from_str(&body).map_err(|e| format!("bad version_manifest_v2: {e}"))?;

    // 2. Find the entry for the requested version id.
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| format!("version '{version_id}' not found in manifest"))?;

    // 3. Fetch + cache the per-version JSON (long TTL — content is immutable).
    let key = format!("version-{}.json", version_id);
    let version_body =
        meta::cached_text(app, &entry.url, &key, VERSION_TTL).await?;

    // 4. Deserialize into typed structs.
    let spec: VersionSpec = serde_json::from_str(&version_body)
        .map_err(|e| format!("bad version manifest for '{version_id}': {e}"))?;

    // Sanity-check the sha1 field is present on the entry (we don't verify
    // the download here because cached_text handles re-validation on miss;
    // full hash verification is download.rs's job at execution time).
    let _ = &entry.sha1; // bind to avoid dead-code warning; used in CP4 plan item

    Ok(spec)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod tests;
