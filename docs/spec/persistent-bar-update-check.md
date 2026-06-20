# Persistent Bar + throttled update-check (spec)

Status: **in progress (2026-06-20).** Branch: `ui-overhaul`. Rule: [[api-frugality]] — one throttled
pack-meta refresh per managed instance per 24h; cached on the manifest; zero network under 24h.

## Goal (human, 2026-06-20)
Enhance the always-visible Instance Detail header ("Persistent Bar"): pack icon + name + author
(Team for Modrinth, Author for CF) + version number (clickable → version/update modal, replacing the
Update button) + a pack-source option. Add a throttled update check: store when the pack was last checked;
if >24h (or never), poll the provider for the latest version; show an "updatable" banner when a newer
version exists. Icon/author/latest populate on first open via the refresh, then cached.

## Data model — `Source` (instances.rs:63) += (all `#[serde(default)]`)
- `icon_url: Option<String>` — pack icon.
- `author: Option<String>` — CF author(s) / Modrinth team or owner.
- `last_update_check: Option<String>` — RFC3339 timestamp of the last refresh.
- `latest_version: Option<String>` — newest available version NUMBER (display).
- `latest_version_id: Option<String>` — newest available version id (for the update action).
(`page_url` already added. `update_available` is derived: `latest_version_id != file_id` when both set.)

## PB-B — backend
| CP | Deliverable | Files |
|----|-------------|-------|
| PB-B1 | `Source` new fields (above). Pure helper `needs_update_check(last: Option<&str>, now: DateTime<Utc>) -> bool` (None or parse-fail or `>24h` → true). Unit tests (none → true; 1h ago → false; 25h ago → true; bad string → true). | `instances.rs` (+`Source`), a helper (lib.rs or instances.rs) + tests |
| PB-B2 | Provider `get_pack_summary(client, id) -> PackSummary { name, icon_url: Option, author: Option }` (NO description — avoid CF's extra /description call). **CurseForge:** GET `/v1/mods/{id}` → name, logo.url, `authors[].name` (join, or first) → author. **Modrinth:** GET `/v2/project/{id}` → title, icon_url; GET `/v2/project/{id}/members` → the owner's `user.username` (role "Owner", else first) as the team/author. Fixture-tested. | `providers.rs`, `modrinth.rs`, `curseforge.rs` + tests/fixtures |
| PB-B3 | `refresh_pack_meta(app, slug) -> PackMetaRefresh { update_available: bool, latest_version: Option<String>, checked: bool }`: load instance; if no `source` → return `{false, None, false}`. If `!needs_update_check(source.last_update_check, now)` → return cached (no network; `update_available` derived from stored `latest_version_id` vs `file_id`, `checked:false`). Else: `get_pack_summary` (name/icon/author) + `get_versions(provider, project_id, None, None)` → newest `[0]` (id+number); store icon_url/author/latest_version/latest_version_id/last_update_check=now on `source`; `save_manifest`; return `{ update_available = latest_version_id != file_id, latest_version, checked:true }`. CF key resolution as other commands. Register in `collect_commands!`. Idempotency/throttle test (mock). | `lib.rs` + tests |
| PB-B4 | Regenerate `bindings.ts` (Source fields + refresh_pack_meta + PackMetaRefresh DTO). | (orchestrator) |

## PB-F — frontend (Persistent Bar + modal)
| CP | Deliverable | Files |
|----|-------------|-------|
| PB-F1 | Redesign the InstanceDetail header into the Persistent Bar: pack icon (`source.iconUrl`, fallback box) + name + author (`source.author`) + a clickable **version** chip (`source.packVersion`) that opens the version/update modal + a "Pack source" / Open-page affordance + the running badge + Launch/Stop. Keep it outside the `<Outlet>` (already persistent). `referrerPolicy="no-referrer"` on the icon. | `src/routes/InstanceDetail.tsx` |
| PB-F2 | On opening a managed instance, call `refreshPackMeta(slug)` once (the backend throttles to 24h); on result with `update_available`, show an **"Updatable"** banner in the bar (with the latest version); invalidate the instance query if `checked` so stored icon/author/latest refresh. Guard to call once per instance per session. | `src/routes/InstanceDetail.tsx`, `ipc.ts` |
| PB-F3 | Version/update modal (reuse the Browse download-prompt pattern): lazy `getModVersions(provider, projectId, null, null)`, current version marked, newest at top, **Update** button → `updateModpack(slug, versionId)`; close on success. | `src/routes/InstanceDetail.tsx` |

## Notes
- Frugality: the refresh is the ONLY new periodic call, throttled to once/24h (backend-authoritative via
  `last_update_check`); under 24h it returns cached with zero network. Modrinth adds one `/members` call
  per refresh for the team name (human-approved).
- Backend changes follow sibling `<stem>_tests.rs`; DTO/command changes → `bindings.ts` regen.
- The existing PackSourcePanel (Info tab) can stay or be folded into the modal later — out of scope here.
