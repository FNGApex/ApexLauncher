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
//! - `resolver`  — vanilla manifest fetch + parse, DownloadPlan + LaunchMeta (Phase 2 slice B).
//! - `java`      — system JRE detection + Adoptium provisioning (Phase 2 slice C).
//! - `java_resolve` — pure helper: resolve effective Java/RAM config for a launch (WS-A A-2).
//! - `auth`      — Microsoft OAuth 2.0 device-code authentication + token chain (Phase 3).
//! - `loader_profile` — Fabric/Quilt loader profile fetch + parse (Phase 4 slice A).
//! - `forge_installer` — NeoForge/Forge headless installer runner (Phase 4 slice B).
//! - `providers` — normalized mod-provider types + trait + HTTP seam (Phase 5 slice A).
//! - `modrinth`  — Modrinth `ModProvider` implementation (Phase 5 slice A, CP2).
//! - `curseforge` — CurseForge `ModProvider` implementation (Phase 5 slice A, CP3).
//! - `materialize` — hardlink-with-copy-fallback instance file-tree builder (storage-auth-reorg C1).
//! - `modpack`     — Modrinth `.mrpack` parse + plan + overrides extraction (Phase 6 slice A).

pub mod auth;
pub mod curseforge;
pub mod download;
pub mod forge_installer;
pub mod instances;
pub mod java;
pub mod java_resolve;
pub mod launch;
pub mod loader_profile;
pub mod loaders;
pub mod materialize;
pub mod meta;
pub mod modpack;
pub mod mod_install;
pub mod modrinth;
pub mod providers;
pub mod resolver;
pub mod settings;
pub mod store;
pub mod task_manager;
pub mod versions;
