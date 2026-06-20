//! Unit tests for `java_resolve`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "java_resolve_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use crate::core::instances::{JavaCfg, Loader, RecommendedJava, Source};
use crate::core::settings::Settings;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn default_settings() -> Settings {
    Settings::default()
    // default_memory_mb = 4096, default_java_args = "-XX:+UseG1GC"
}

fn base_instance() -> crate::core::instances::Instance {
    crate::core::instances::Instance {
        schema: 1,
        id: "test-id".into(),
        name: "Test".into(),
        slug: "test".into(),
        icon: None,
        minecraft: "1.20.1".into(),
        loader: Loader {
            kind: "vanilla".into(),
            version: None,
        },
        java: JavaCfg {
            major: None,
            args_override: None,
            memory_mb: 8192,
            min_memory_mb: None,
            path_override: None,
            use_pack_settings: false,
        },
        source: None,
        pack_locked: false,
        mods: vec![],
        created: "2024-01-01T00:00:00Z".into(),
        last_played: None,
        total_playtime_sec: 0,
    }
}

// -----------------------------------------------------------------------
// Tier 3: global default (use_pack_settings == false, no recommended)
// -----------------------------------------------------------------------

/// When `use_pack_settings` is false and source has no recommendation,
/// we fall through to global defaults from Settings.
#[test]
fn global_default_when_use_pack_settings_false() {
    let inst = base_instance(); // use_pack_settings = false
    let settings = default_settings();

    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(result.xmx_mb, 4096, "xmx_mb must come from settings.default_memory_mb");
    assert_eq!(result.xms_mb, None, "xms_mb must be None for global default");
    assert_eq!(
        result.extra_args,
        vec!["-XX:+UseG1GC".to_string()],
        "extra_args must come from settings.default_java_args"
    );
    assert_eq!(result.java_path, None, "java_path must be None for global default");
}

/// When `use_pack_settings` is false AND default_java_args is empty,
/// extra_args must be an empty Vec (no spurious empties).
#[test]
fn global_default_empty_args_produces_empty_vec() {
    let inst = base_instance();
    let mut settings = default_settings();
    settings.default_java_args = "".into();

    let result = resolve_effective_java(&inst, &settings);

    assert!(
        result.extra_args.is_empty(),
        "extra_args must be empty when default_java_args is blank"
    );
}

// -----------------------------------------------------------------------
// Tier 2: per-instance override (use_pack_settings == true)
// -----------------------------------------------------------------------

/// When `use_pack_settings` is true, all instance fields are used.
#[test]
fn instance_override_when_use_pack_settings_true() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.memory_mb = 8192;
    inst.java.min_memory_mb = Some(2048);
    inst.java.args_override = Some("-XX:+UseZGC".into());
    inst.java.path_override = Some("/usr/lib/jvm/java-21/bin/java".into());

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(result.xmx_mb, 8192, "xmx_mb must come from inst.java.memory_mb");
    assert_eq!(result.xms_mb, Some(2048), "xms_mb must come from inst.java.min_memory_mb");
    assert_eq!(
        result.extra_args,
        vec!["-XX:+UseZGC".to_string()],
        "extra_args must come from inst.java.args_override"
    );
    assert_eq!(
        result.java_path,
        Some(std::path::PathBuf::from("/usr/lib/jvm/java-21/bin/java")),
        "java_path must come from inst.java.path_override"
    );
}

/// With use_pack_settings = true but min_memory_mb = None, xms_mb stays None.
#[test]
fn instance_override_min_unset_xms_is_none() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.memory_mb = 4096;
    inst.java.min_memory_mb = None; // not set

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(result.xms_mb, None, "xms_mb must be None when min_memory_mb is not set");
}

/// With use_pack_settings = true but args_override = None ("not configured"),
/// extra_args inherits the global default_java_args (per-field fallback, §4.1).
#[test]
fn instance_override_unset_args_inherits_global() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.args_override = None;

    let settings = default_settings(); // default_java_args = "-XX:+UseG1GC"
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(
        result.extra_args,
        vec!["-XX:+UseG1GC".to_string()],
        "unset args_override must inherit the global default_java_args"
    );
}

/// With use_pack_settings = true and an EXPLICIT empty args_override (Some("")),
/// the user deliberately wants no extra args — does NOT inherit global.
#[test]
fn instance_override_explicit_empty_args_gives_empty_vec() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.args_override = Some("".into());

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert!(
        result.extra_args.is_empty(),
        "explicit Some(\"\") args_override must yield empty extra_args, not inherit global"
    );
}

/// With use_pack_settings = true and multi-token args_override, extra_args has the tokens.
#[test]
fn instance_override_args_split_on_whitespace() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.args_override = Some("  -XX:+UseZGC  -Xss1m  ".into());

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(
        result.extra_args,
        vec!["-XX:+UseZGC".to_string(), "-Xss1m".to_string()],
        "extra_args must be split on whitespace with empties dropped"
    );
}

// -----------------------------------------------------------------------
// Tier 1: provider-recommended overrides everything
// -----------------------------------------------------------------------

/// When source has a recommended memory_mb, that wins even with use_pack_settings=false.
#[test]
fn recommended_memory_overrides_global_default() {
    let mut inst = base_instance();
    inst.source = Some(Source {
        provider: "modrinth".into(),
        project_id: "proj".into(),
        file_id: "file".into(),
        pack_version: "1.0".into(),
        recommended: Some(RecommendedJava {
            memory_mb: Some(6144),
            java_args: None,
        }),
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
    });
    // use_pack_settings = false → would normally give 4096 from settings
    inst.java.use_pack_settings = false;

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(
        result.xmx_mb, 6144,
        "xmx_mb must come from recommended.memory_mb when Some"
    );
}

/// When source has a recommended memory_mb, that wins even with use_pack_settings=true.
#[test]
fn recommended_memory_overrides_instance_override() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.memory_mb = 8192;
    inst.source = Some(Source {
        provider: "modrinth".into(),
        project_id: "proj".into(),
        file_id: "file".into(),
        pack_version: "1.0".into(),
        recommended: Some(RecommendedJava {
            memory_mb: Some(6144),
            java_args: None,
        }),
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
    });

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(
        result.xmx_mb, 6144,
        "xmx_mb must come from recommended.memory_mb when Some"
    );
}

/// When recommended has java_args set, extra_args comes from there.
#[test]
fn recommended_java_args_overrides_instance_args() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.args_override = Some("-XX:+UseZGC".into());
    inst.source = Some(Source {
        provider: "modrinth".into(),
        project_id: "proj".into(),
        file_id: "file".into(),
        pack_version: "1.0".into(),
        recommended: Some(RecommendedJava {
            memory_mb: None,
            java_args: Some("-XX:+UseG1GC -Xss512k".into()),
        }),
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
    });

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(
        result.extra_args,
        vec!["-XX:+UseG1GC".to_string(), "-Xss512k".to_string()],
        "extra_args must come from recommended.java_args when Some"
    );
    // xmx_mb falls through to instance override (use_pack_settings=true) since recommended.memory_mb=None
    assert_eq!(result.xmx_mb, 8192, "xmx_mb falls to instance tier when recommended.memory_mb is None");
}

/// When source is Some but recommended is None, it's treated as no recommendation.
#[test]
fn source_with_no_recommended_falls_through_to_instance_tier() {
    let mut inst = base_instance();
    inst.java.use_pack_settings = true;
    inst.java.memory_mb = 8192;
    inst.source = Some(Source {
        provider: "modrinth".into(),
        project_id: "proj".into(),
        file_id: "file".into(),
        pack_version: "1.0".into(),
        recommended: None,
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
    });

    let settings = default_settings();
    let result = resolve_effective_java(&inst, &settings);

    assert_eq!(
        result.xmx_mb, 8192,
        "xmx_mb must come from instance tier when source.recommended is None"
    );
}
