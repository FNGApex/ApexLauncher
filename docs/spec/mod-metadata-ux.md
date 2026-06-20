# Mod metadata + Add-mod UX (spec)

Status: **in progress (2026-06-19).** Branch: `ui-overhaul`. Human batch + the `api-frugality` rule
(poll providers as little as possible; store metadata at add-time, never re-fetch to display).

## Goals (human, 2026-06-19)
1. **Add-mod search stops fetching versions per mod** — zero version/`.jar` polling on the search list;
   resolve "latest" only on the Add click.
2. **Rename "Install" → "Add"** in the instance Add-mod section.
3. **"Added" state** — search results already in the instance show **Added**; on hover → **Remove** (✕),
   click removes.
4. **Installed tab rows larger.**
5. **Richer installed mods** — Name + Icon + short description, jar filename as subtitle. Stored at add-time
   from the search `ProjectSummary` (no re-fetch). Dependency-added mods show jar name only (no extra calls).
6. **Open project page** for managed instances — a link on the Info tab / pack-source panel.

## Checkpoints

### AM-B — Backend: metadata fields + capture (+ bindings)
| CP | Deliverable | Files |
|----|-------------|-------|
| AM-B1 | `ModEntry` += `name: Option<String>`, `icon_url: Option<String>`, `summary: Option<String>` (all `#[serde(default)]`). `planned_to_mod_entry` sets them `None` (deps + default). | `instances.rs`, `mod_install.rs:539`, tests |
| AM-B2 | `add_mod` command += optional `name`/`icon_url`/`summary` params; after the job builds `added` entries, set the ROOT entry's (project_id == the added project_id) metadata from the params. Frontend passes the search `ProjectSummary`'s name/icon/summary. | `lib.rs:1265` (add_mod), `lib.rs` ModAddJob, `ipc.ts` |
| AM-B3 | `Source` += `page_url: Option<String>` (`#[serde(default)]`); set it where `Source` is built (`lib.rs:2735`) from the install flow's available page URL (installModpack's pageUrl param / resolved). Used by AM-F3. | `instances.rs`, `lib.rs:2735`, tests |
| AM-B4 | Regenerate `bindings.ts` (ModEntry + Source fields + add_mod signature). | `src/lib/bindings.ts` (orchestrator) |

### AM-F1 — Add-mod search (ModSearchCard) — frugal + Add/Added/Remove
| CP | Deliverable | Files |
|----|-------------|-------|
| AM-F1 | Remove the per-card `getModVersions` query entirely (no display fetch). Button renamed **Add**. On click: fetch newest version once (`getModVersions` → `[0]`), then `addMod(..., name, iconUrl, summary)` (pass the ProjectSummary metadata); keep the F2-2 task-tracking spinner. Detect already-installed (instance.mods has `project_id === mod.id`) → show **Added**; on hover → **Remove** ✕ → `removeMod(slug, <that entry's file_name>)`. | `src/routes/InstanceDetail.tsx` (ModSearchCard + AddModTab needs `modEntries`) |

### AM-F2 — Installed list (ModRow) — richer + larger
| CP | Deliverable | Files |
|----|-------------|-------|
| AM-F2 | Larger rows; show icon (ModEntry.iconUrl) + Name (ModEntry.name ?? fileName) + jar filename as subtitle + short description (ModEntry.summary). Falls back gracefully when metadata is absent (deps / old entries). | `src/routes/InstanceDetail.tsx` (ModRow / InstalledModsTab) |

### AM-F3 — Info tab "Open project page"
| CP | Deliverable | Files |
|----|-------------|-------|
| AM-F3 | In the Info tab / `PackSourcePanel`, for managed instances (instance.source set), an **"Open project page"** button → `openUrl(source.pageUrl)`; fall back to a built modrinth URL when pageUrl absent (old instances). | `src/routes/InstanceDetail.tsx` (PackSourcePanel) |

## Notes
- Frugality: NO new display-time provider fetches. The only add-time fetch is the single newest-version
  lookup on the Add click (intent), unavoidable to resolve a concrete version_id.
- Backend changes follow sibling `<stem>_tests.rs`; DTO/command changes need a `bindings.ts` regen.
- Dependency-added mods intentionally lack name/icon/summary (jar name only) — no extra calls.
