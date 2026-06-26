# Design: CurseForge manual-download UX

> Branch: `feat/cf-manual-download-ux`
> Status: design — approved-ready, not implemented
> Companion spec: `docs/spec/cf-manual-download-ux.md`

## Problem

Some CurseForge mods set `allowModDistribution: false` (or return `downloadUrl: null`).
We cannot fetch these programmatically (CF API terms — `docs/PROVIDERS.md:35-48`). When a
modpack import or a Browse one-click install hits one, the file is routed to
`CfPackPlan.manual` (`src-tauri/src/core/modpack.rs:504-512`) instead of the download
plan. Today the only surfacing of that fact is a **one-shot toast** (`src/components/Toasts.tsx:148-159`)
that opens the project page(s). Once the toast is dismissed the information is gone:
the instance has missing mods, the user has no durable record of which ones, where to get
them, or any feedback once they drop the jar in. They can launch a known-broken pack with
no warning.

This design makes the manual-download state **durable, instance-level, and self-healing**.

## Goals

1. **Precise links** — link the user to the exact file page, not a numeric redirect.
2. **Persistent warnings** — the pending list survives on the manifest; the instance
   shows an incomplete badge + panel; launching with pending files warns first.
3. **Auto-recovery** — when the user drops the jar into `mods/`, detect it and self-heal
   (panel + badge clear, a real `ModEntry` is recorded).

## Non-goals

- No proxying or scraping of disabled files (forbidden by CF terms).
- No murmur2 fingerprint matching (`/v1/fingerprints`) — auto-recovery matches by
  `file_name` first, hash second; fingerprinting is a possible later refinement.
- No global "downloads inbox" — pending state is strictly per-instance.

---

## Evidence trail (file:line)

| Fact | Source |
|------|--------|
| `CfManualFile { project_id, file_id, file_name, page_url }` | `src-tauri/src/core/modpack.rs:395-406` |
| `page_url` built as `…/projects/{projectId}` numeric redirect | `src-tauri/src/core/modpack.rs:508-511` |
| Manual routing (url=None or no sha1) | `src-tauri/src/core/modpack.rs:492-513` |
| `resolve_and_build_cf_plan` (the per-file resolve seam) | `src-tauri/src/core/modpack.rs:547-582` |
| `get_file` → `CfFileResponse` (no parent-mod data) | `src-tauri/src/core/curseforge.rs:435-457` |
| `build_mod_url` (`/v1/mods/{id}`) already exists | `src-tauri/src/core/curseforge.rs:404-407` |
| `CfModData` parses name/logo/authors/summary/categories — **NOT** slug/links | `src-tauri/src/core/curseforge.rs:249-262` |
| `CfMod`/`CfLinks` (search path) already parse `links.websiteUrl` | `src-tauri/src/core/providers.rs:489-541` |
| `CfImportResult.manual` carried to frontend | `src-tauri/src/lib.rs:2659-2671` |
| `update_modpack` → `PackUpdateResult.manual` | `src-tauri/src/lib.rs:3184` |
| One-shot toast surfaces `manual[].pageUrl` | `src/components/Toasts.tsx:42-48, 148-159` |
| `Instance` struct + `SCHEMA_VERSION=1` | `src-tauri/src/core/instances.rs:22, 131-150` |
| `Source` back-compat optionals (`#[serde(default)] Option<…>`) | `src-tauri/src/core/instances.rs:63-98` |
| `ModEntry { provider, project_id, version_id, file_name, hashes, enabled, side, … }` | `src-tauri/src/core/instances.rs:102-127` |
| `read_manifest` / `write_manifest` | `src-tauri/src/core/instances.rs:324-339` |
| `scan_mods` already walks the `mods/` dir | `src-tauri/src/core/instances.rs:341+` |
| `launch_instance` steps (load → resolve → download → spawn) | `src-tauri/src/lib.rs:734-984` |
| `TaskKind` / `TaskJob` / sync-vs-enqueued split | `src-tauri/src/core/task_manager.rs:43-54, 335-345`; `src-tauri/src/lib.rs:1615-1673` |
| Sinks + event channels | `src-tauri/src/lib.rs:342-359, 526-559, 642-664, 700-719` |
| `AppShell` listeners → store actions | `src/components/AppShell.tsx:57-112` |
| `useAppStore` / `useUiStore` | `src/lib/store.ts:37-133` |
| Tabs / router child routes | `src/routes/InstanceDetail.tsx:378-419`; `src/router.tsx:27-35` |
| `handleLaunch` / launch button | `src/routes/InstanceDetail.tsx:157-192, 324-332` |
| Reusable modal pattern (VersionUpdateModal) | `src/routes/InstanceDetail.tsx:520-535` |
| File picker `open()` already used | `src/components/NewInstanceModal.tsx:3`; `src/routes/instance-tabs/JavaTab.tsx:62-78` |
| No drag-drop anywhere yet | (NOT FOUND) |
| `notify` not yet a dependency | `src-tauri/Cargo.toml:20-44` |

External (primary sources):
- CF `/v1/mods/{id}.data` carries `slug` + `links.websiteUrl` — CurseForge REST docs (`docs.curseforge.com/rest-api/#get-mod`).
- Exact file-page URL format: `https://www.curseforge.com/minecraft/mc-mods/{slug}/files/{fileId}` (verified; numeric `…/projects/{id}` is a redirect).
- `notify` 8.2.0 + `notify-debouncer-full` 0.7.0, MSRV 1.85 (project is on Rust 1.96).

---

## Pillar 1 — Precise links

### Decision
Upgrade `CfManualFile.page_url` from the numeric `…/projects/{projectId}` redirect to the
slug-based **exact file page** `https://www.curseforge.com/minecraft/mc-mods/{slug}/files/{fileId}`.

### How
The slug lives on the parent mod (`GET /v1/mods/{projectId}` → `data.slug`), which
`get_file` does **not** fetch. The planner already iterates per-file in
`resolve_and_build_cf_plan` (`modpack.rs:547-582`) and only routes a file to `manual` when
`url == None`. So the extra mod lookup is needed **only for files that actually go manual** —
exactly the api-frugality constraint.

Plan:
- Add `slug: Option<String>` and `links` to `CfModData` (`curseforge.rs:249-262`) — the
  `/v1/mods/{id}` endpoint already returns them; we just stop discarding them.
- Add `CurseForgeProvider::get_mod_slug(client, project_id) -> Result<Option<String>, ProviderError>`
  (one GET to `build_mod_url`, returns `data.slug`). Sibling tests with a mock client.
- In `resolve_and_build_cf_plan`, **only in the manual branch**, call `get_mod_slug` and
  build the precise URL. Cache the slug per `project_id` in a `HashMap<u64, Option<String>>`
  local to the call so a pack with several manual files from the same mod fetches once.
- Build helper `cf_file_page_url(slug: Option<&str>, project_id, file_id) -> String`:
  - `Some(slug)` → `…/minecraft/mc-mods/{slug}/files/{fileId}`
  - `None` (fetch failed / no slug) → fall back to today's `…/projects/{projectId}` redirect.
- `page_url` is **persisted** on the manual entry and on the manifest (Pillar 2), so it is
  never re-fetched merely to display — honors the api-frugality standing rule.

`build_cf_pack_plan` is pure/synchronous and has no client; the slug enrichment therefore
lives in `resolve_and_build_cf_plan` (the async seam that already has the provider+client).
`build_cf_pack_plan` keeps emitting the numeric fallback; `resolve_and_build_cf_plan`
rewrites `page_url` on the returned `manual` entries. This keeps the pure planner unit-testable
without a network mock.

### Tradeoff
+1 GET per distinct manual mod. Acceptable: only on the manual path (rare), cached per
project, and the result is persisted. Closes follow-up
`.claude/project/followups/modpack-import-cf-manual-slug-link.md`.

---

## Pillar 2 — Persistent, instance-level warnings

### Persistent state shape

New struct on the manifest, mirroring the established `#[serde(default)]` + `Option`
back-compat pattern (`Source.page_url` at `instances.rs:74-75` is the precedent):

```rust
// src-tauri/src/core/instances.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingManual {
    pub project_id: String,      // CF mod id (stringified, matches ModEntry convention)
    pub file_id: String,         // CF file id
    pub file_name: String,       // exact filename the user must drop into mods/
    pub page_url: String,        // slug-based exact-file URL (Pillar 1)
    #[serde(default)]
    pub expected_sha1: Option<String>, // hash if CF gave one; None → name/size match only
    #[serde(default)]
    pub size: Option<u64>,       // declared size for the name-only acceptance fallback
}

// added to Instance:
#[serde(default)]
pub pending_manual: Vec<PendingManual>,
#[serde(default)]
pub suppress_pending_launch_warning: bool,   // per-instance "don't ask again"
```

### Migration / back-compat
- **No `SCHEMA_VERSION` bump.** Both new fields are `#[serde(default)]`: old `instance.json`
  with no `pendingManual` / `suppressPendingLaunchWarning` deserialize to `[]` / `false`.
  This is exactly the precedent set by `from_pack` (`instances.rs:113-114`) and `pack_locked`
  (`instances.rs:144-145`) — both added without a schema bump.
- The single load path is `read_manifest` (`instances.rs:324-327`); serde defaults make the
  migration transparent there. The single save path `write_manifest` (`instances.rs:336-339`)
  starts emitting the fields once they are non-default.
- **Backfill for in-flight imports:** `pending_manual` is populated when the importer/installer
  writes the manifest (from the same `CfPackPlan.manual` it returns to the toast). Existing
  instances that imported manual files *before* this feature have `pending_manual == []` —
  acceptable; the Re-scan button (Pillar 3) + the next import refresh them, and they were
  already in the "silent missing mods" state this feature is fixing.

### Where the pending list is written
`ImportCfZipJob.run` (`lib.rs:2693-2862`) and `update_modpack`/`UpdateModpackJob`
(`lib.rs:3184`) already hold `plan.manual` and build the `Instance` before promote. They
set `instance.pending_manual = plan.manual.iter().map(PendingManual::from).collect()`
just before `write_manifest`. `install_modpack` (Browse one-click) flows through
`enqueue_import_cf_zip` (`lib.rs:2866-2897`) → same job, so it is covered.

### New commands (Pillar 2)
| Command | Sync? | Returns | bindings regen? |
|---------|-------|---------|-----------------|
| `set_pending_launch_warning_suppressed(slug, bool) -> ()` | sync | — | yes (new cmd) |

`get_instance` already returns the full `Instance` (`lib.rs:276-278`), so the panel and the
pre-launch path both read `pending_manual` straight off the existing `["instance", slug]`
query — **no dedicated `get_pending_manual` getter** (decided). Only
`set_pending_launch_warning_suppressed` is strictly new here. See spec CP-3.

### UI surfaces

**Pending-downloads panel** — a section in `InfoTab.tsx` (model: the existing
`PackSourcePanel` / update-check bar at `InstanceDetail.tsx:337-350`), rendered only when
`instance.pendingManual.length > 0`. Not a new tab — keeps the tab bar stable and co-locates
with pack source info. Each row: mod file name, an **Open page** button (`openUrl(page_url)`,
reusing the Toasts pattern), and a **Pick file…** affordance (Pillar 3).

**Incomplete-pack badge** — a small amber chip near the instance title in `InstanceDetail.tsx`
(beside the existing update-available banner region), shown when `pendingManual.length > 0`,
labelled e.g. `N missing`. Mirrors the update-available chip styling.

**Pre-launch warning flow** (decided: per-instance "don't ask again"):

```
handleLaunch() (InstanceDetail.tsx:157-192)
  └─ if instance.pendingManual.length > 0
       && !instance.suppressPendingLaunchWarning
     └─ open PendingLaunchModal (reuse VersionUpdateModal pattern, InstanceDetail.tsx:520-535)
          ├─ lists missing files + Open-page links
          ├─ [ ] Don't warn me again for this instance
          ├─ "Launch anyway"  → if checkbox set: set_pending_launch_warning_suppressed(slug,true)
          │                      then proceed to launchInstance(slug)
          └─ "Cancel"         → abort
  └─ else proceed directly to launchInstance(slug)
```

The remembered choice persists on the manifest (`suppress_pending_launch_warning`), so it
survives restarts and is scoped to that one instance. Auto-recovery clearing all pending
files removes the warning regardless.

### Toast change
The one-shot toast stays but becomes a *pointer* rather than the only record: on a partial
import it still appears, but its copy changes to "N need manual download — see the instance's
Pending panel" and the Open-pages button remains. The durable truth is now the panel. (Minimal
change to `Toasts.tsx` — no behavior regression.)

---

## Pillar 3 — Auto-recovery

### Detection — three layers (decided)
1. **Live FS watcher** (`notify` + `notify-debouncer-full`) on the instance `mods/` dir.
2. **On-demand "Re-scan" button** in the pending panel.
3. **Launch-time scan** as a fallback (cheap, always runs before spawn).

All three converge on one pure function so behavior is identical regardless of trigger:

```rust
// src-tauri/src/core/instances.rs (pure, unit-tested)
/// Inspect `mods/` against the instance's pending_manual list. For each pending
/// entry whose file now exists in mods/ and validates, produce a resolution:
/// remove it from pending_manual and append a real ModEntry.
pub fn reconcile_pending_manual(inst: &mut Instance, mods_dir: &Path) -> Vec<ResolvedManual>;
```

Matching rule (per pending entry):
- Find the dropped file by either the **exact** `mods/<file_name>` or the **disabled** form
  `mods/<file_name>.disabled` (decided: accept both). Exact name wins when both exist.
- **Validate:** if `expected_sha1` is `Some`, hash the file (reuse `sha1` crate already in
  deps, `Cargo.toml:31`) and require a match; else accept on name (+ optional size check
  against `size`).
- On match: remove from `pending_manual`, append `ModEntry { provider:"curseforge",
  project_id, version_id:file_id, file_name, hashes:{sha1 if computed}, enabled, side:"both",
  from_pack:true, name:None, icon_url:None, summary:None }` — where `enabled` is `false` when
  matched via the `.disabled` form, `true` for the exact name. No re-fetch to display
  (api-frugality) — name/icon stay `None` and `enrich_instance_mods` backfills later.
- When `pending_manual` becomes empty, the panel + badge auto-clear (they are derived from
  the list) and the launch warning no longer triggers.

This is **synchronous** — no network, just a dir read + optional hash. Per the task-queue
contract (`task_manager.rs` + CLAUDE.md), instant ops stay off the queue, like
`set_mod_enabled`/`remove_mod` (`lib.rs:1615-1673`). No `TaskJob`.

### New command + event
| Surface | Kind | Returns | bindings regen? |
|---------|------|---------|-----------------|
| `rescan_pending_manual(slug) -> Vec<PendingManual>` (command — runs `reconcile_pending_manual`, writes manifest, returns the *remaining* list) | sync command | remaining pending | yes |
| `manual://resolved` event payload `{ slug, fileName, remaining: u32 }` | event | — | yes (new event) |

The `manual://resolved` event is emitted by the watcher (and optionally by the command) so
the open UI updates live without a manual refetch. It follows the existing channel naming
(`task://`, `run://`, `launch://`, `install://`) and rides the standard
`AppShell` listener → Zustand store path:
- New `useAppStore` slice action `notePendingResolved(slug, remaining)` (or simpler: bump a
  per-slug counter the InfoTab query subscribes to so it re-reads the instance). Minimal
  approach in the spec: the event handler in `AppShell.tsx` invalidates the
  `["instance", slug]` query so the panel/badge re-derive from fresh manifest data. No new
  store slice strictly required — but a tiny `pendingResolvedTick` in `useAppStore` is the
  clean option and is what the spec adopts.

### Watcher lifecycle (decided: lazy, detail-open scoped)

**Scope:** the `notify` watcher is **lazy** — it runs only while an instance detail page with
pending files is mounted, and stops on unmount. There is **no** app-wide background watcher.
Rationale: zero idle cost, and the watcher's only job is live feedback while the user is
actively looking at the instance ("I dropped the jar, watch the row clear"). Drops made while
the detail page is closed (browser/Explorer, another instance) are not the watcher's
responsibility — they are caught by the **launch-time scan** and the **Re-scan button**, which
are the safety net.

**Frontend-driven lifecycle (commands, not an app-wide task):** the watch is wired to the
React route lifecycle rather than app setup:
- `start_pending_watch(slug) -> ()` — called from a `useEffect` in `InfoTab.tsx` (or
  `InstanceDetail.tsx`) when the instance mounts *and* `pendingManual.length > 0`. Idempotent;
  registers a debounced recursive-off watch on that instance's `mods/` dir.
- `stop_pending_watch(slug) -> ()` — called from the same effect's cleanup on unmount, and
  whenever `pendingManual` empties.
- Backed by a `PendingWatcher` Tauri-managed state (`Arc<Mutex<HashMap<slug, Debouncer>>>`)
  holding at most the currently-open instance's watch. `start` is a no-op if already watching;
  `stop` drops the debouncer.

**Debounce / noise:** `notify-debouncer-full` with a ~750ms–1s debounce window collapses the
editor/AV/temp-file churn (`.part`, `~`-suffixed, `.tmp`, `.crdownload`) — the handler
**ignores** files whose name matches no pending entry (exact or `.disabled` form), so AV
scratch files never trigger work. On a debounced batch the handler runs
`reconcile_pending_manual` once.

**Not double-healing:** `reconcile_pending_manual` is idempotent — it only acts on entries
still in `pending_manual`; once an entry is removed it can't be re-resolved. The manifest write
is the single source of truth. The watcher, the Re-scan button, and the launch scan all call
the same function under the instance's manifest, so concurrent triggers converge (the
`PendingWatcher` mutex + the fact that resolution is removal-from-list makes a second pass a
no-op). Manifest writes for a given instance are serialized through the command layer.

**Launch-time scan (primary safety net):** in `launch_instance`, after loading the instance
(`lib.rs:745-749`) and before the pre-launch warning is evaluated, run
`reconcile_pending_manual` once and persist — so a user who dropped the jar while the detail
page was closed, then hit Launch, gets healed without ever opening the panel, and the warning
reflects reality. Combined with the on-demand Re-scan button, this fully covers out-of-app
drops that the lazy watcher does not see.

### Manual fallback — drag-drop / pick-file
The panel gets a **Pick file…** button (reusing `@tauri-apps/plugin-dialog` `open()`, already
used at `JavaTab.tsx:62-78`) and a **drag-drop zone**. Both resolve to: copy the chosen jar
into the instance `mods/` dir under the pending `file_name` (or validate if already named
correctly), then call `rescan_pending_manual`. This covers the case where the watcher missed
an event or the user prefers an explicit action. Drag-drop is net-new to the frontend (no
existing `onDrop` usage) — implemented with Tauri's webview file-drop or a DOM drop zone that
hands the path to a small `import_manual_file(slug, src_path)` command (copies into `mods/`,
then reconciles). Spec sequences this last (CP-6) so the watcher path ships first.

---

## New surface summary

### Rust
- `instances.rs`: `PendingManual` struct, `ResolvedManual` struct, `Instance.pending_manual`,
  `Instance.suppress_pending_launch_warning`, `reconcile_pending_manual()`.
- `curseforge.rs`: `CfModData.slug`/`links`, `get_mod_slug()`.
- `modpack.rs`: slug enrichment in `resolve_and_build_cf_plan`, `cf_file_page_url()` helper.
- `lib.rs`: commands `set_pending_launch_warning_suppressed`, `rescan_pending_manual`,
  `import_manual_file`, `start_pending_watch`, `stop_pending_watch`; `PendingWatcher` managed
  state (single open-instance watch) + `manual://resolved` sink; launch-time reconcile in
  `launch_instance`; pending-list write in the import/update jobs.
- `Cargo.toml`: `notify = "=8.2.0"`, `notify-debouncer-full = "=0.7.0"` (exact pins, matching
  the specta deps convention — decided).

### Events / IPC
- New event channel `manual://resolved` `{ slug, fileName, remaining }`.
- All new DTOs/commands/events require **regenerating `src/lib/bindings.ts`** via
  `scripts/build.sh dev` (wait for `[bindings] exported`, stop) — never a hand-edit of
  `ipc.ts`. Called out per-checkpoint in the spec.

### Frontend
- `InfoTab.tsx`: pending-downloads panel (Open page / Pick file / drag-drop / Re-scan).
- `InstanceDetail.tsx`: incomplete badge; `PendingLaunchModal`; `handleLaunch` gate.
- `AppShell.tsx`: `manual://resolved` listener → invalidate `["instance", slug]` (+ optional
  `pendingResolvedTick`).
- `store.ts`: optional `pendingResolvedTick` action.
- `ipc.ts`: thin wrappers for the new commands (generated DTOs re-exported).

---

## UX wireframe (ASCII)

InfoTab — pending panel (only when pending > 0):

```
┌─ Pack source ─────────────────────────────────────────────┐
│  All the Mods 10   ·  v4.7   ·  [Check for updates]        │
└───────────────────────────────────────────────────────────┘
┌─ ⚠ 2 files need a manual download ─────────────[ Re-scan ]┐
│  These mods disable automatic downloads. Get them from     │
│  CurseForge and drop the .jar into this instance's mods/   │
│  folder — they'll be detected automatically.               │
│                                                            │
│  • optifine-1.20.1.jar     [ Open page ↗ ] [ Pick file… ]  │
│  • somemod-2.3.4.jar       [ Open page ↗ ] [ Pick file… ]  │
│                                                            │
│  ┌───────────────────────────────────────────────────┐    │
│  │   ⬇  Drag .jar files here to add them               │    │
│  └───────────────────────────────────────────────────┘    │
└───────────────────────────────────────────────────────────┘
```

Instance header badge:

```
All the Mods 10   [ 2 missing ]   [ ▶ Launch ]   [ ⏹ ]
```

Pre-launch warning modal:

```
┌─ Missing mods ────────────────────────────────[ X ]┐
│  This pack has 2 mods that haven't been downloaded   │
│  yet. It may crash or behave incorrectly.            │
│                                                      │
│  • optifine-1.20.1.jar          [ Open page ↗ ]      │
│  • somemod-2.3.4.jar            [ Open page ↗ ]      │
│                                                      │
│  [ ] Don't warn me again for this instance           │
│                                                      │
│                       [ Cancel ]  [ Launch anyway ]  │
└──────────────────────────────────────────────────────┘
```

---

## Tradeoffs / rejected alternatives

- **Panel vs new tab.** Rejected a dedicated "Downloads" tab — it would be empty for the
  common case (no pending files) and split attention from the pack-source info it relates to.
  A conditional InfoTab section is lighter and self-clearing.
- **Watcher scope: detail-open-only vs app-wide.** Chose **detail-open-only** (lazy). App-wide
  watchers would catch out-of-app drops live, but at a standing idle cost and added complexity
  (a watcher set maintained across the whole instance list). Out-of-app drops are instead
  covered by the launch-time scan + the on-demand Re-scan button, which are cheap and
  deterministic. The live watcher's value — instant row-clear feedback — only matters while the
  user is looking at the instance, which is exactly when the lazy watcher is running.
- **Fingerprint (murmur2) matching.** Rejected for v1 — `file_name` + optional sha1 covers the
  realistic "user downloaded the exact file we told them to" case. Fingerprinting adds a
  hash-strip implementation and a `/v1/fingerprints` call (network — against api-frugality)
  for a marginal gain. Possible later.
- **Schema bump + explicit migration code.** Rejected — `#[serde(default)]` is the
  established, proven pattern here (three precedents). A bump would force touching every old
  manifest needlessly.
- **Re-fetch mod metadata on heal.** Rejected — the auto-created `ModEntry` leaves
  `name/icon_url/summary == None` and lets the existing `enrich_instance_mods` backfill them,
  honoring the api-frugality standing rule (never poll to merely display).
- **Making rescan/heal a TaskJob.** Rejected — it's a local, instant op (dir read + hash); the
  contract reserves the queue for long/network ops. Stays synchronous like
  `set_mod_enabled`/`remove_mod`.

## Resolved decisions (was: open questions)

1. **Watcher lifecycle → LAZY, detail-open scoped.** No app-wide watcher; `start_pending_watch`
   / `stop_pending_watch` are wired to the detail-route lifecycle. Out-of-app drops covered by
   the launch scan + Re-scan button.
2. **Disabled-file drop → ACCEPT.** `reconcile_pending_manual` resolves both `<file_name>` and
   `<file_name>.disabled`; the latter records the `ModEntry` with `enabled: false`.
3. **Toast → KEEP as pointer.** The partial-import toast stays and points at the instance's
   Pending panel; the panel is the durable record.
4. **No `get_pending_manual` command.** The panel and pre-launch path read `instance.pendingManual`
   from the existing `["instance", slug]` query.
5. **Dependency pins → EXACT.** `notify = "=8.2.0"`, `notify-debouncer-full = "=0.7.0"`.
