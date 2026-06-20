//! Pure helper: resolve the effective Java/RAM configuration for a launch.
//!
//! No I/O or `AppHandle` — safe to unit-test directly.
//!
//! Precedence (highest first, §4.1 + locked §8 Q4):
//!   1. `inst.source.recommended` — honored per-field when `Some`.
//!   2. Per-instance override — used when `inst.java.use_pack_settings == true`.
//!   3. Global default from `Settings` — fallback.

use std::path::PathBuf;

use super::instances::Instance;
use super::settings::Settings;

/// The resolved Java/RAM parameters to pass to the JVM at launch.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveJava {
    /// Max heap size (`-Xmx<n>m`). Always `Some` — resolved from one of the three tiers.
    pub xmx_mb: u32,
    /// Min heap size (`-Xms<n>m`). `None` = omit `-Xms` entirely (§8 Q5).
    pub xms_mb: Option<u32>,
    /// Extra JVM args (whitespace-split, empties dropped). May be empty.
    pub extra_args: Vec<String>,
    /// Explicit java binary path. `None` = auto-detect via `ensure_java(major)`.
    pub java_path: Option<PathBuf>,
}

/// Resolve the effective Java/RAM configuration for `inst` given global `settings`.
///
/// Pure function — no I/O, no filesystem access.
pub fn resolve_effective_java(inst: &Instance, settings: &Settings) -> EffectiveJava {
    // Extract the optional recommended tier (tier 1).
    let recommended = inst
        .source
        .as_ref()
        .and_then(|s| s.recommended.as_ref());

    // --- xmx_mb ---
    // Tier 1 wins if recommended.memory_mb is Some.
    // Tier 2 wins if use_pack_settings == true.
    // Tier 3 (global) is the fallback.
    let xmx_mb = if let Some(rec_mem) = recommended.and_then(|r| r.memory_mb) {
        rec_mem
    } else if inst.java.use_pack_settings {
        inst.java.memory_mb
    } else {
        settings.default_memory_mb
    };

    // --- xms_mb ---
    // Only set from the per-instance min (use_pack_settings = true). The global
    // tier never sets xms (§8 Q5), and `RecommendedJava` has no min field today —
    // if one is ever added, add a recommended arm here too.
    let xms_mb = if inst.java.use_pack_settings {
        inst.java.min_memory_mb
    } else {
        None
    };

    // --- extra_args ---
    // Tier 1 wins if recommended.java_args is Some.
    // Tier 2 (use_pack_settings) is PER-FIELD for args: `args_override == None`
    // means "not configured" → inherit the global default_java_args (so enabling
    // per-instance RAM doesn't silently drop the global GC tuning, design §4.1
    // "fields unset → global"). An explicit `Some(s)` overrides — including
    // `Some("")` = deliberately no extra args.
    // Tier 3 is the global default_java_args.
    let raw_args: &str = if let Some(rec_args) = recommended.and_then(|r| r.java_args.as_deref()) {
        rec_args
    } else if inst.java.use_pack_settings {
        inst.java
            .args_override
            .as_deref()
            .unwrap_or(settings.default_java_args.as_str())
    } else {
        settings.default_java_args.as_str()
    };
    let extra_args: Vec<String> = raw_args
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    // --- java_path ---
    // Only set from per-instance path_override (use_pack_settings = true).
    // Neither recommended nor global tier supplies a binary path.
    let java_path = if inst.java.use_pack_settings {
        inst.java.path_override.as_deref().map(PathBuf::from)
    } else {
        None
    };

    EffectiveJava {
        xmx_mb,
        xms_mb,
        extra_args,
        java_path,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "java_resolve_tests.rs"]
mod tests;
