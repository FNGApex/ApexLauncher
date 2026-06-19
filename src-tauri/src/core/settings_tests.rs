use super::*;

/// Deserializing an old settings.json that does NOT have `sidebarStartCollapsed`
/// must yield `false` (the serde default).
#[test]
fn sidebar_start_collapsed_defaults_to_false_on_missing_field() {
    let json = r#"{
        "schema": 1,
        "defaultMemoryMb": 4096,
        "defaultJavaArgs": "-XX:+UseG1GC",
        "curseforgeApiKey": null,
        "offlineMode": false
    }"#;

    let settings: Settings = serde_json::from_str(json).expect("valid json");
    assert!(!settings.sidebar_start_collapsed, "should default to false");
}

/// Round-trip: serialize then deserialize preserves the value.
#[test]
fn sidebar_start_collapsed_round_trips() {
    let mut s = Settings::default();
    s.sidebar_start_collapsed = true;

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert!(back.sidebar_start_collapsed);
}

/// `impl Default` gives `false`.
#[test]
fn settings_default_sidebar_start_collapsed_is_false() {
    let s = Settings::default();
    assert!(!s.sidebar_start_collapsed);
}
