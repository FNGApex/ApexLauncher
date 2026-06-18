//! Export test for `src/lib/bindings.ts`.
//!
//! This module is gated `#[cfg(all(test, not(target_os = "windows")))]` in
//! `lib.rs` because constructing the command surface creates function pointers
//! to Tauri commands. On Windows those commands transitively link WebView2 GUI
//! DLLs, which crashes the test binary before any tests run. On Linux (with
//! GTK/WebKit installed, e.g. future CI) the test works natively via
//! `cargo test --manifest-path src-tauri/Cargo.toml export_bindings`.
//!
//! It exports through the SAME `crate::make_builder()` the real app uses, so the
//! test-generated and app-generated `bindings.ts` are byte-identical — there is
//! no second command list or second export path to drift.
//!
//! On Windows: regenerate via `scripts/build.sh dev` (the `#[cfg(debug_assertions)]`
//! block in `run()` writes `src/lib/bindings.ts` at app startup, via this builder).
//!
//! Wired from `lib.rs`:
//!   `#[cfg(all(test, not(target_os = "windows")))] #[path = "bindings_export_tests.rs"] mod bindings_export_tests;`

#[test]
fn export_bindings() {
    crate::make_builder()
        .export(
            specta_typescript::Typescript::default(),
            "../src/lib/bindings.ts",
        )
        .expect("failed to export bindings.ts");
}
