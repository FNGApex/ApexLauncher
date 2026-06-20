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

// ---------------------------------------------------------------------------
// T3 auto_download_java — default true
// ---------------------------------------------------------------------------

/// Old settings.json without `autoDownloadJava` must yield `true`.
#[test]
fn auto_download_java_defaults_to_true_on_missing_field() {
    let json = r#"{
        "schema": 1,
        "defaultMemoryMb": 4096,
        "defaultJavaArgs": "-XX:+UseG1GC",
        "curseforgeApiKey": null,
        "offlineMode": false,
        "sidebarStartCollapsed": false
    }"#;

    let settings: Settings = serde_json::from_str(json).expect("valid json");
    assert!(settings.auto_download_java, "should default to true");
}

/// Round-trip: serialize then deserialize preserves the value.
#[test]
fn auto_download_java_round_trips() {
    let mut s = Settings::default();
    s.auto_download_java = false;

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert!(!back.auto_download_java);
}

/// `impl Default` gives `true`.
#[test]
fn settings_default_auto_download_java_is_true() {
    let s = Settings::default();
    assert!(s.auto_download_java);
}

// ---------------------------------------------------------------------------
// T5 show_console_default — default false
// ---------------------------------------------------------------------------

/// Old settings.json without `showConsoleDefault` must yield `false`.
#[test]
fn show_console_default_defaults_to_false_on_missing_field() {
    let json = r#"{
        "schema": 1,
        "defaultMemoryMb": 4096,
        "defaultJavaArgs": "-XX:+UseG1GC",
        "curseforgeApiKey": null,
        "offlineMode": false,
        "sidebarStartCollapsed": false
    }"#;

    let settings: Settings = serde_json::from_str(json).expect("valid json");
    assert!(!settings.show_console_default, "should default to false");
}

/// Round-trip: serialize then deserialize preserves the value.
#[test]
fn show_console_default_round_trips() {
    let mut s = Settings::default();
    s.show_console_default = true;

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert!(back.show_console_default);
}

/// `impl Default` gives `false`.
#[test]
fn settings_default_show_console_default_is_false() {
    let s = Settings::default();
    assert!(!s.show_console_default);
}

// ---------------------------------------------------------------------------
// T6 keep_launcher_open — default true
// ---------------------------------------------------------------------------

/// Old settings.json without `keepLauncherOpen` must yield `true`.
#[test]
fn keep_launcher_open_defaults_to_true_on_missing_field() {
    let json = r#"{
        "schema": 1,
        "defaultMemoryMb": 4096,
        "defaultJavaArgs": "-XX:+UseG1GC",
        "curseforgeApiKey": null,
        "offlineMode": false,
        "sidebarStartCollapsed": false
    }"#;

    let settings: Settings = serde_json::from_str(json).expect("valid json");
    assert!(settings.keep_launcher_open, "should default to true");
}

/// Round-trip: serialize then deserialize preserves the value.
#[test]
fn keep_launcher_open_round_trips() {
    let mut s = Settings::default();
    s.keep_launcher_open = false;

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert!(!back.keep_launcher_open);
}

/// `impl Default` gives `true`.
#[test]
fn settings_default_keep_launcher_open_is_true() {
    let s = Settings::default();
    assert!(s.keep_launcher_open);
}

// ---------------------------------------------------------------------------
// T7 maximize_on_start — default true
// ---------------------------------------------------------------------------

/// Old settings.json without `maximizeOnStart` must yield `true`.
#[test]
fn maximize_on_start_defaults_to_true_on_missing_field() {
    let json = r#"{
        "schema": 1,
        "defaultMemoryMb": 4096,
        "defaultJavaArgs": "-XX:+UseG1GC",
        "curseforgeApiKey": null,
        "offlineMode": false,
        "sidebarStartCollapsed": false
    }"#;

    let settings: Settings = serde_json::from_str(json).expect("valid json");
    assert!(settings.maximize_on_start, "should default to true");
}

/// Round-trip: serialize then deserialize preserves the value.
#[test]
fn maximize_on_start_round_trips() {
    let mut s = Settings::default();
    s.maximize_on_start = false;

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert!(!back.maximize_on_start);
}

/// `impl Default` gives `true`.
#[test]
fn settings_default_maximize_on_start_is_true() {
    let s = Settings::default();
    assert!(s.maximize_on_start);
}
