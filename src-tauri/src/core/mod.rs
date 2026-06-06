//! Core domain logic, kept out of the thin Tauri command layer in `lib.rs`.
//!
//! Modules are added per roadmap phase. So far:
//! - `store`     — on-disk layout / path resolution under the app data dir.
//! - `instances` — the `instance.json` model and create/list/get/delete (Phase 1).
//! - `settings`  — global `settings.json` (defaults + CF API key) (Phase 1).
//! - `meta`      — TTL'd disk-cached HTTP helper for the metadata fetchers.
//! - `versions`  — Minecraft release list (Mojang piston-meta).
//! - `loaders`   — per-MC mod-loader version lists (Forge/Fabric/Quilt/NeoForge).
//! - `download`  — concurrent, hash-verified download engine (Phase 2).

pub mod download;
pub mod instances;
pub mod loaders;
pub mod meta;
pub mod settings;
pub mod store;
pub mod versions;
