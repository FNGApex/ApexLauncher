use std::fs;
use std::path::Path;

fn main() {
    // Parse the gitignored `.env` file for MODLOADER_CF_API_KEY and bake it
    // into the binary via cargo:rustc-env so `option_env!` can read it at
    // compile time.  Missing or unparseable `.env` → bake nothing; build
    // still succeeds and `option_env!` returns `None`.
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");

    // Retrigger this build script when .env changes (or appears/disappears).
    println!("cargo:rerun-if-changed={}", env_path.display());
    // Also retrigger when the env var is set externally (CI / developer shell).
    println!("cargo:rerun-if-env-changed=MODLOADER_CF_API_KEY");

    if let Ok(contents) = fs::read_to_string(&env_path) {
        for line in contents.lines() {
            let line = line.trim();
            // Skip blanks and comments.
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("MODLOADER_CF_API_KEY=") {
                // Strip at most one leading and one trailing quote independently
                // (single or double).  Independent passes let mismatched/partial
                // quotes like `"key'` shed only the surviving quote character.
                let value = rest.trim();
                let value = if value.starts_with('"') || value.starts_with('\'') {
                    &value[1..]
                } else {
                    value
                };
                let value = if value.ends_with('"') || value.ends_with('\'') {
                    &value[..value.len() - 1]
                } else {
                    value
                };
                let value = value.trim();
                if !value.is_empty() {
                    println!("cargo:rustc-env=MODLOADER_CF_API_KEY={value}");
                }
                // Found the key line — no need to read further.
                break;
            }
        }
    }
    // If .env is absent or the key line is missing/blank, no
    // cargo:rustc-env is emitted; option_env!("MODLOADER_CF_API_KEY") → None.

    tauri_build::build()
}
