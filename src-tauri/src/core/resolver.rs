//! Vanilla Minecraft resolver — Phase 2, slice B.
//!
//! Fetches + caches the per-version manifest from Mojang's piston-meta, parses
//! it into typed structs, and (later checkpoints) produces a [`DownloadPlan`]
//! plus a `LaunchMeta` struct for slice D.
//!
//! CP1: manifest fetch + parse only. Rule eval (CP2), asset resolution (CP3),
//!      and command wiring (CP4) are added in subsequent iterations.

use std::time::Duration;

use serde::Deserialize;
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
mod tests {
    use super::*;

    const MODERN_FIXTURE: &str =
        include_str!("fixtures/version_manifest_modern.json");

    #[test]
    fn parse_modern_manifest_client_download() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(
            spec.downloads.client.url,
            "https://piston-data.mojang.com/v1/objects/aabbccddeeff00112233445566778899aabbccdd/client.jar"
        );
        assert_eq!(
            spec.downloads.client.sha1,
            "aabbccddeeff00112233445566778899aabbccdd"
        );
        assert_eq!(spec.downloads.client.size, 26234786);
    }

    #[test]
    fn parse_modern_manifest_main_class() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.main_class, "net.minecraft.client.main.Main");
    }

    #[test]
    fn parse_modern_manifest_java_major() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.java_major(), 21);
    }

    #[test]
    fn parse_modern_manifest_asset_index() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.asset_index.id, "17");
        assert_eq!(
            spec.asset_index.sha1,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
        assert_eq!(spec.asset_index.size, 447030);
        assert_eq!(spec.asset_index.total_size, 799786602);
    }

    #[test]
    fn parse_modern_manifest_libraries() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.libraries.len(), 3);
        let authlib = &spec.libraries[0];
        assert_eq!(authlib.name, "com.mojang:authlib:6.0.54");
        let artifact = authlib.downloads.artifact.as_ref().unwrap();
        assert_eq!(artifact.size, 112233);
    }

    #[test]
    fn parse_modern_manifest_structured_arguments() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        // Modern manifest: `arguments` present, `minecraftArguments` absent.
        assert!(spec.arguments.is_some());
        assert!(spec.minecraft_arguments.is_none());

        let args = spec.arguments.unwrap();
        // Game args include plain string entries.
        assert!(!args.game.is_empty());
        // JVM args include both plain strings and conditional (rules-based) entries.
        assert!(!args.jvm.is_empty());
    }

    #[test]
    fn java_major_defaults_to_8_when_absent() {
        // Minimal manifest with no javaVersion field.
        let json = r#"{
            "id": "1.8.9",
            "mainClass": "net.minecraft.client.main.Main",
            "downloads": {
                "client": {
                    "url": "https://example.com/client.jar",
                    "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "size": 1000000
                }
            },
            "assetIndex": {
                "id": "1.8",
                "sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "size": 100000,
                "totalSize": 200000000,
                "url": "https://example.com/1.8.json"
            },
            "libraries": []
        }"#;

        let spec: VersionSpec = serde_json::from_str(json)
            .expect("minimal manifest must deserialize");
        assert_eq!(spec.java_major(), 8);
    }

    // -----------------------------------------------------------------------
    // CP2: artifact path field
    // -----------------------------------------------------------------------

    #[test]
    fn artifact_path_field_present_in_modern_fixture() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize");
        let artifact = spec.libraries[0].downloads.artifact.as_ref().unwrap();
        assert_eq!(
            artifact.path.as_deref(),
            Some("com/mojang/authlib/6.0.54/authlib-6.0.54.jar")
        );
    }

    // -----------------------------------------------------------------------
    // CP2: rule evaluation
    // -----------------------------------------------------------------------

    fn make_rule(action: &str, os_name: Option<&str>) -> serde_json::Value {
        match os_name {
            None => serde_json::json!({ "action": action }),
            Some(name) => serde_json::json!({ "action": action, "os": { "name": name } }),
        }
    }

    #[test]
    fn eval_rules_no_rules_allows_all_os() {
        // Empty rules → allowed on every OS.
        assert!(eval_rules(&[], "linux"));
        assert!(eval_rules(&[], "windows"));
        assert!(eval_rules(&[], "osx"));
    }

    #[test]
    fn eval_rules_allow_all_no_os() {
        // Single allow rule with no OS constraint → matches everything.
        let rules = vec![make_rule("allow", None)];
        assert!(eval_rules(&rules, "linux"));
        assert!(eval_rules(&rules, "windows"));
        assert!(eval_rules(&rules, "osx"));
    }

    #[test]
    fn eval_rules_allow_specific_os() {
        // allow osx only.
        let rules = vec![make_rule("allow", Some("osx"))];
        assert!(!eval_rules(&rules, "linux"));
        assert!(!eval_rules(&rules, "windows"));
        assert!(eval_rules(&rules, "osx"));
    }

    #[test]
    fn eval_rules_disallow_specific_os() {
        // allow all, then disallow windows.
        let rules = vec![
            make_rule("allow", None),
            make_rule("disallow", Some("windows")),
        ];
        assert!(eval_rules(&rules, "linux"));
        assert!(!eval_rules(&rules, "windows"));
        assert!(eval_rules(&rules, "osx"));
    }

    #[test]
    fn eval_rules_last_matching_rule_wins() {
        // disallow windows, then allow windows → allowed (later rule wins).
        let rules = vec![
            make_rule("disallow", Some("windows")),
            make_rule("allow", Some("windows")),
        ];
        assert!(eval_rules(&rules, "windows"));
        // linux: only the first rule has os=windows (no match), second has os=windows
        // (no match) → default disallow (rules present, none matched linux).
        assert!(!eval_rules(&rules, "linux"));
    }

    // -----------------------------------------------------------------------
    // CP2: classpath selection (legacy fixture)
    // -----------------------------------------------------------------------

    const LEGACY_FIXTURE: &str =
        include_str!("fixtures/version_manifest_legacy.json");

    #[test]
    fn parse_legacy_manifest_uses_minecraft_arguments() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        assert!(spec.minecraft_arguments.is_some());
        assert!(spec.arguments.is_none());
        assert!(spec
            .minecraft_arguments
            .unwrap()
            .contains("${auth_player_name}"));
    }

    #[test]
    fn classpath_linux_excludes_osx_and_windows_only_libs() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let cp = select_classpath(&spec.libraries, "linux");

        // authlib (no rules), lwjgl (allow all), lwjgl_util (allow all) → 3 entries.
        // java-objc-bridge (allow osx only) → excluded.
        // icu4j-core-mojang (allow windows only) → excluded.
        assert_eq!(cp.len(), 3);
        let paths: Vec<&str> = cp.iter().map(|e| e.maven_path.as_str()).collect();
        assert!(paths.contains(&"com/mojang/authlib/1.5.25/authlib-1.5.25.jar"));
        assert!(paths.contains(&"org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar"));
        assert!(paths.contains(&"org/lwjgl/lwjgl/lwjgl_util/2.9.4-nightly-20150209/lwjgl_util-2.9.4-nightly-20150209.jar"));
    }

    #[test]
    fn classpath_windows_excludes_osx_only_lib_includes_windows_only() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let cp = select_classpath(&spec.libraries, "windows");

        // authlib + lwjgl + lwjgl_util + icu4j (windows-only) = 4.
        // java-objc-bridge (osx-only) excluded.
        assert_eq!(cp.len(), 4);
        let paths: Vec<&str> = cp.iter().map(|e| e.maven_path.as_str()).collect();
        assert!(paths.contains(&"com/ibm/icu/icu4j-core-mojang/51.2/icu4j-core-mojang-51.2.jar"));
        assert!(!paths.contains(&"ca/weblite/java-objc-bridge/1.1/java-objc-bridge-1.1.jar"));
    }

    #[test]
    fn classpath_osx_excludes_windows_only_lib_includes_osx_only() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let cp = select_classpath(&spec.libraries, "osx");

        // authlib + lwjgl + lwjgl_util + java-objc-bridge (osx-only) = 4.
        // icu4j (windows-only) excluded.
        assert_eq!(cp.len(), 4);
        let paths: Vec<&str> = cp.iter().map(|e| e.maven_path.as_str()).collect();
        assert!(paths.contains(&"ca/weblite/java-objc-bridge/1.1/java-objc-bridge-1.1.jar"));
        assert!(!paths.contains(&"com/ibm/icu/icu4j-core-mojang/51.2/icu4j-core-mojang-51.2.jar"));
    }

    // -----------------------------------------------------------------------
    // CP2: natives selection
    // -----------------------------------------------------------------------

    #[test]
    fn natives_linux_resolves_correct_classifier() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let natives = select_natives(&spec.libraries, "linux");

        assert_eq!(natives.len(), 1);
        assert_eq!(natives[0].classifier, "natives-linux");
        assert_eq!(
            natives[0].maven_path,
            "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209-natives-linux.jar"
        );
        assert_eq!(natives[0].sha1, "cccc0003cccc0003cccc0003cccc0003cccc0003");
    }

    #[test]
    fn natives_windows_resolves_correct_classifier() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let natives = select_natives(&spec.libraries, "windows");

        assert_eq!(natives.len(), 1);
        assert_eq!(natives[0].classifier, "natives-windows");
        assert_eq!(
            natives[0].maven_path,
            "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209-natives-windows.jar"
        );
    }

    #[test]
    fn natives_osx_resolves_correct_classifier() {
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let natives = select_natives(&spec.libraries, "osx");

        assert_eq!(natives.len(), 1);
        assert_eq!(natives[0].classifier, "natives-osx");
        assert_eq!(
            natives[0].maven_path,
            "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209-natives-osx.jar"
        );
    }

    #[test]
    fn natives_arch_token_substituted() {
        // A library whose classifier template contains ${arch}.
        let json = serde_json::json!({
            "name": "org.lwjgl:lwjgl:3.x",
            "downloads": {
                "classifiers": {
                    "natives-linux-64": {
                        "path": "org/lwjgl/lwjgl/3.x/lwjgl-3.x-natives-linux-64.jar",
                        "sha1": "aaaa1234aaaa1234aaaa1234aaaa1234aaaa1234",
                        "size": 100000,
                        "url": "https://example.com/lwjgl-3.x-natives-linux-64.jar"
                    }
                }
            },
            "natives": {
                "linux": "natives-linux-${arch}"
            }
        });
        let lib: Library = serde_json::from_value(json).unwrap();
        let natives = select_natives(&[lib], "linux");
        assert_eq!(natives.len(), 1);
        assert_eq!(natives[0].classifier, "natives-linux-64");
    }

    #[test]
    fn maven_dest_path_prefix() {
        // Verify classpath entries report the correct maven_path for dest computation.
        let spec: VersionSpec = serde_json::from_str(LEGACY_FIXTURE)
            .expect("legacy fixture must deserialize");
        let cp = select_classpath(&spec.libraries, "linux");
        // Every entry's dest would be: libraries/<maven_path>
        for entry in &cp {
            assert!(
                !entry.maven_path.starts_with('/'),
                "maven_path must be relative: {}",
                entry.maven_path
            );
            assert!(
                entry.maven_path.ends_with(".jar"),
                "maven_path must end with .jar: {}",
                entry.maven_path
            );
        }
    }
}
