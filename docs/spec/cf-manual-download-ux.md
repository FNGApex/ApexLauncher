# Spec: CurseForge manual-download UX

> Branch: `feat/cf-manual-download-ux`
> Design: `docs/design/cf-manual-download-ux.md`
> Build/test ONLY via `scripts/build.sh` (`check`, `test [filter]`, `dev`). Tests live in
> sibling `<stem>_tests.rs` files (CLAUDE.md convention). DTO/command/event changes require
> regenerating `src/lib/bindings.ts` via `scripts/build.sh dev` (wait for `[bindings] exported`,
> stop) — never hand-edit `ipc.ts`.

Each checkpoint ends **runnable** (`scripts/build.sh check` green, named tests pass, app builds).
Sequence: link fix → manifest schema+migration → panel+badge → pre-launch warning →
watcher/rescan/launch-scan auto-recovery → drag-drop fallback.

---

## Checkpoint table

| CP | Goal | Files touched | Tests to add | bindings regen? | Runnable gate |
|----|------|---------------|--------------|-----------------|---------------|
| **CP-1** | Precise slug-based manual links | `src-tauri/src/core/curseforge.rs` (add `slug`+`links` to `CfModData`; `get_mod_slug`); `src-tauri/src/core/modpack.rs` (`cf_file_page_url` helper; slug enrichment in `resolve_and_build_cf_plan`, cached per project) | `curseforge_tests.rs`: `get_mod_slug` parses `data.slug` (mock client), returns `None` on missing slug, surfaces `KeyMissing`. `modpack_tests.rs`: `cf_file_page_url` → `…/mc-mods/{slug}/files/{fileId}` with slug, numeric `…/projects/{id}` fallback without; `resolve_and_build_cf_plan` rewrites `page_url` on manual entries (mock client returns a slug) and fetches each project's slug at most once | **No** (no new DTO/command/event — `CfManualFile` shape unchanged) | `build.sh check` + `build.sh test core::curseforge` + `build.sh test core::modpack` green |
| **CP-2** | Persistent pending state + migration | `src-tauri/src/core/instances.rs` (`PendingManual` struct, `Instance.pending_manual`, `Instance.suppress_pending_launch_warning`, both `#[serde(default)]`); write `pending_manual` in import/update jobs `src-tauri/src/lib.rs` (`ImportCfZipJob.run` ~2820, `update_modpack`/`UpdateModpackJob` ~3184) | `instances_tests.rs`: old `instance.json` with no `pendingManual`/`suppress…` deserializes to `[]`/`false` (back-compat); round-trip write→read preserves a populated `pending_manual`; `PendingManual::from(&CfManualFile)` maps fields | **Yes** — `Instance`/`PendingManual` are generated DTOs (`specta::Type`). Regen `bindings.ts`; confirm `pendingManual?: PendingManual[]` + `suppressPendingLaunchWarning?: boolean` appear | `build.sh check` (Rust + tsc) + `build.sh test core::instances` green; `bindings.ts` regenerated |
| **CP-3** | Pending panel + incomplete badge (read-only) | `src/routes/instance-tabs/InfoTab.tsx` (pending section: list + Open-page button); `src/routes/InstanceDetail.tsx` (amber `N missing` badge near title); `src/lib/ipc.ts` (re-export `PendingManual`); `src-tauri/src/lib.rs` + `instances.rs` (`set_pending_launch_warning_suppressed(slug,bool)` command — sync) | Rust: `instances_tests.rs` — setting/clearing `suppress_pending_launch_warning` persists. (No frontend test harness yet — visual only.) | **Yes** — new command `set_pending_launch_warning_suppressed`. Regen `bindings.ts` | `build.sh check` green; panel renders when `pendingManual.length>0`, hidden otherwise; `bindings.ts` regenerated |
| **CP-4** | Pre-launch warning + "don't ask again" | `src/routes/InstanceDetail.tsx` (`PendingLaunchModal` reusing VersionUpdateModal pattern ~520-535; gate in `handleLaunch` ~157-192); `src/lib/ipc.ts` (wrapper for `set_pending_launch_warning_suppressed`) | None new (frontend-only behavior; backed by CP-3 Rust persistence test) | **No** (commands already generated in CP-3) | `build.sh check` green; launch with pending+unsuppressed → modal; "Launch anyway" + checkbox → suppress persists then launches; "Cancel" aborts; suppressed or zero-pending → launches directly |
| **CP-5** | Auto-recovery: reconcile fn + rescan command + launch scan + lazy watcher + event | `src-tauri/Cargo.toml` (`notify="=8.2.0"`, `notify-debouncer-full="=0.7.0"` — exact pins); `src-tauri/src/core/instances.rs` (`ResolvedManual`, `reconcile_pending_manual()`); `src-tauri/src/lib.rs` (`rescan_pending_manual`, `start_pending_watch`, `stop_pending_watch` commands — sync; `PendingWatcher` managed state holding the single open-instance watch; `manual://resolved` sink; launch-time reconcile in `launch_instance` after load ~745-749); `src/components/AppShell.tsx` (`manual://resolved` listener → invalidate `["instance",slug]`); `src/lib/store.ts` (optional `pendingResolvedTick`); `src/routes/instance-tabs/InfoTab.tsx` (Re-scan button; `useEffect` start/stop watch on mount/unmount when `pendingManual.length>0`) | `instances_tests.rs`: `reconcile_pending_manual` — exact-name match with matching sha1 → entry removed + `ModEntry{enabled:true}` appended; **`.disabled`-form match → resolved + `ModEntry{enabled:false}`**; name match, wrong sha1 → not resolved; name-only (no `expected_sha1`) → accepted; missing file → unchanged; idempotent (second call no-op); empties list when all resolved | **Yes** — new commands `rescan_pending_manual`/`start_pending_watch`/`stop_pending_watch` + new event `manual://resolved`. Regen `bindings.ts`; confirm `events.manualResolved` exists | `build.sh check` + `build.sh test core::instances` green; with the detail page open, dropping the named jar into `mods/` clears its row live (lazy watcher); a drop made with the page closed clears at launch (scan) or on Re-scan; `bindings.ts` regenerated |
| **CP-6** | Manual fallback: pick-file + drag-drop | `src-tauri/src/lib.rs` (`import_manual_file(slug, src_path)` command — copies jar into `mods/` then reconciles, sync); `src/routes/instance-tabs/InfoTab.tsx` (Pick file… via `@tauri-apps/plugin-dialog` `open()`; drag-drop zone); `src/lib/ipc.ts` (wrapper) | `lib_tests.rs` (or `instances_tests.rs` if logic extracted): copying a jar named per a pending entry into `mods/` then reconciling resolves it; copying an unrelated jar leaves pending unchanged | **Yes** — new command `import_manual_file`. Regen `bindings.ts` | `build.sh check` green; Pick file… / drag-drop a jar → row clears; `bindings.ts` regenerated |

---

## Per-checkpoint detail

### CP-1 — Precise links (no bindings regen)
- `CfModData` (`curseforge.rs:249-262`): add `#[serde(default)] slug: Option<String>` and a
  `links: Option<CfLinks>` with `#[serde(rename = "websiteUrl")] website_url: Option<String>`
  (or reuse the `CfLinks` already in `providers.rs:514-517` if accessible; otherwise a local
  mirror). Only `slug` is needed for the URL; `links` optional.
- `get_mod_slug(&self, client, project_id: &str) -> Result<Option<String>, ProviderError>`:
  `require_key()`, GET `build_mod_url`, parse `CfModResponse`, return `data.slug`.
- `cf_file_page_url(slug: Option<&str>, project_id: u64, file_id: u64) -> String` in `modpack.rs`.
- In `resolve_and_build_cf_plan` (`modpack.rs:547-582`): keep a
  `HashMap<u64, Option<String>>` slug cache; in the manual branch only, look up/fetch the
  slug and set `page_url = cf_file_page_url(...)`. `build_cf_pack_plan` stays pure (numeric
  fallback); the async wrapper rewrites the manual entries' `page_url`.
- Closes follow-up `.claude/project/followups/modpack-import-cf-manual-slug-link.md` (mark
  `status: closed` on merge).

### CP-2 — Schema + migration (bindings regen)
- `PendingManual` struct + two `Instance` fields per design (`instances.rs`). `#[serde(default)]`,
  no `SCHEMA_VERSION` bump (precedent: `from_pack` `instances.rs:113-114`, `pack_locked`
  `instances.rs:144-145`).
- `PendingManual::from(&CfManualFile)` helper (carries `expected_sha1`/`size` when the resolved
  `VersionFile` had them — thread the hash/size through from `build_cf_pack_plan`'s manual
  branch; today it discards them).
- Import/update jobs set `instance.pending_manual` before `write_manifest`.

### CP-3 — Panel + badge (bindings regen)
- Panel = conditional section in `InfoTab.tsx` modeled on `PackSourcePanel`/update bar
  (`InstanceDetail.tsx:337-350`). Reads `instance.pendingManual` from the existing
  `["instance",slug]` query (no dedicated getter — see design open Q4).
- Badge in `InstanceDetail.tsx` header region.
- `set_pending_launch_warning_suppressed` command (sync, mirrors `set_pack_lock`
  `lib.rs:1615-1673` pattern).
- Toast stays as a **pointer** (decided): minimal `Toasts.tsx` copy change so the partial-import
  toast points at the instance's Pending panel; the panel is now the durable record. No
  behavior regression.

### CP-4 — Pre-launch warning (no bindings regen)
- `PendingLaunchModal` reuses VersionUpdateModal markup (`InstanceDetail.tsx:520-535`).
- Gate inside `handleLaunch` before `launchInstance(slug)` (`InstanceDetail.tsx:175`).
- Checkbox → `set_pending_launch_warning_suppressed(slug,true)` then proceed.

### CP-5 — Auto-recovery (bindings regen)
- `reconcile_pending_manual(inst, mods_dir) -> Vec<ResolvedManual>` pure fn (`instances.rs`),
  reusing the `sha1` crate (`Cargo.toml:31`) and the existing `mods/` walk style from
  `scan_mods` (`instances.rs:341+`). Idempotent. Matches `<file_name>` (→ `ModEntry{enabled:true}`)
  **or** `<file_name>.disabled` (→ `ModEntry{enabled:false}`); exact name wins if both present.
- `rescan_pending_manual(slug)` command: load → reconcile → write → emit `manual://resolved`
  per resolved file → return remaining list. Sync (no TaskJob — instant local op).
- Launch scan (primary safety net for closed-page drops): in `launch_instance` after instance
  load (`lib.rs:745-749`), reconcile + persist before the warning is evaluated.
- **Lazy watcher** (decided: detail-open scoped, NOT app-wide): `start_pending_watch(slug)` /
  `stop_pending_watch(slug)` commands wired to the InfoTab/InstanceDetail route lifecycle via a
  `useEffect` (start on mount when `pendingManual.length>0`, stop on unmount/empty).
  `PendingWatcher` managed state holds the single currently-open watch: `notify-debouncer-full`
  recursive-off watch on that instance's `mods/` dir; ~750ms–1s debounce; handler ignores
  filenames matching no pending entry (exact or `.disabled`) + temp suffixes
  (`.part`/`.tmp`/`.crdownload`/`~`), calls `reconcile_pending_manual` on a batch, emits
  `manual://resolved`, drops the watch when the list empties. No app-setup watcher registration.
- `manual://resolved { slug, fileName, remaining }` sink (model: `TauriTaskObserver`
  `lib.rs:642-664`). `AppShell` listener invalidates `["instance",slug]` (and bumps optional
  `pendingResolvedTick`).

### CP-6 — Drag-drop / pick-file (bindings regen)
- `import_manual_file(slug, src_path)` command: validate `src_path` is a `.jar`, copy into the
  instance `mods/` under the resolved filename, reconcile, return remaining. Sync.
- `InfoTab.tsx`: Pick file… (`open()` from `@tauri-apps/plugin-dialog`, pattern at
  `JavaTab.tsx:62-78`) + a DOM drag-drop zone handing the path to `import_manual_file`.

---

## Test inventory delta (expected)
- `curseforge_tests.rs`: +~3 (`get_mod_slug` cases).
- `modpack_tests.rs`: +~3 (`cf_file_page_url`, slug rewrite, single-fetch-per-project).
- `instances_tests.rs`: +~9 (migration round-trip, suppress persist, `reconcile_pending_manual`
  matrix including the `.disabled`-form → `enabled:false` case).
- `lib_tests.rs`: +~2 (`import_manual_file` copy+reconcile).
- No frontend tests (none exist yet; planned Phase 7).

## Regeneration checklist (bindings.ts)
Regen required at **CP-2, CP-3, CP-5, CP-6**. CP-1 and CP-4 do **not** touch generated DTOs/
commands/events. Each regen: `scripts/build.sh dev` → wait for `[bindings] exported` →
stop → commit the regenerated `src/lib/bindings.ts` alongside the Rust change.

## Change log
- 2026-06-21 — Initial spec authored (design `docs/design/cf-manual-download-ux.md`). Not implemented.
- 2026-06-21 — Resolved 5 open questions: watcher → lazy/detail-open (`start_pending_watch`/
  `stop_pending_watch`, not app-wide); `.disabled`-form drop accepted (→ `enabled:false`);
  `notify`/`notify-debouncer-full` exact-pinned; toast kept as pointer; no `get_pending_manual`
  command. Regen flags unchanged (CP-2/3/5/6).
