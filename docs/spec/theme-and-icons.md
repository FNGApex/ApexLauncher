# Spec: dark/light themes + instance icons

> Workstream 2 of the Phase-7 polish slices — execute **after** `docs/spec/rustls-tls-switch.md`
> has shipped. Design: `docs/design/theme-and-icons.md`.
> Build/test ONLY via `scripts/build.sh` (`check`, `test [filter]`, `dev`). Tests live in
> sibling `<stem>_tests.rs` files (CLAUDE.md convention). DTO/command/event changes require
> regenerating `src/lib/bindings.ts` via `scripts/build.sh dev` (wait for `[bindings] exported`,
> stop) — never hand-edit `ipc.ts`.

Each checkpoint ends **runnable** (`scripts/build.sh check` green, named tests pass, app builds).
Sequence: **themes first** (CP-1 no-flash plumbing → CP-2 Settings control), **then icons**
(CP-3 backend copy/clear + tests → CP-4 asset protocol + render precedence → CP-5 picker UI).
Themes touch **no** Rust surface (no bindings regen); the single regen is at CP-3 (two new
commands).

---

## Checkpoint table

| CP | Goal | Files touched | Tests to add | bindings regen? | Runnable gate |
|----|------|---------------|--------------|-----------------|---------------|
| **CP-1** | Theme engine + no-flash boot (no UI control yet) | `index.html` (inline `<head>` script reading `apex-theme`, toggling `.light` on `documentElement`); `src/lib/theme.ts` (new: `ThemePref="system"\|"light"\|"dark"`, `getThemePref`/`setThemePref`/`applyTheme`/`subscribeSystem`, `apex-theme` localStorage key); `src/main.tsx` (call `applyTheme()` + register the `matchMedia` `change` listener at startup) | None automated (no frontend test harness yet). Manual: see gate. | **No** (pure frontend, no Rust) | `scripts/build.sh check` (tsc) green; with `localStorage.apex-theme` unset the app boots dark with **no** flash; setting it to `"light"` in devtools + reload boots light with no flash; `"system"` follows the OS toggle live (no reload) |
| **CP-2** | Settings → Appearance control | `src/routes/Settings.tsx` (new "Appearance" section above "Behavior": a System/Light/Dark segmented control bound to `theme.ts`) | None automated. | **No** | `scripts/build.sh check` green; the control reflects the current pref, switching it re-themes the whole app instantly and persists across reload; choosing "System" re-enables OS-follow |
| **CP-3** | Backend: set/clear instance icon (copy into tree) | `src-tauri/src/core/instances.rs` (pure helper `write_instance_icon(inst_dir, &mut Instance, src_path) -> Result<(), String>`: ext allowlist + size cap, copy to `icon-<unixMillis>.<ext>`, delete prior `icon-*`, set `inst.icon`; and `clear_instance_icon_file(inst_dir, &mut Instance)`); `src-tauri/src/lib.rs` (sync commands `set_instance_icon(slug, src_path)`, `clear_instance_icon(slug)` — load → helper → `write_manifest`) | `instances_tests.rs`: copying a `.png` sets `icon` to `icon-<ts>.png` + file exists in dir; a second set deletes the prior `icon-*` (only one remains); rejected extension (`.txt`/`.exe`) → `Err`, `icon` unchanged; oversize → `Err`; `clear` removes file + sets `icon=None`; round-trip write→`read_manifest` preserves `icon` | **Yes** — 2 new commands (`Instance.icon` DTO field already exists; no shape change). Regen `bindings.ts`; confirm `commands.setInstanceIcon`/`commands.clearInstanceIcon` appear | `scripts/build.sh check` + `scripts/build.sh test core::instances` green; `bindings.ts` regenerated |
| **CP-4** | Display: asset protocol + render precedence | `src-tauri/tauri.conf.json` (`app.security.assetProtocol = { enable: true, scope: ["$DATA/ApexLauncher/instances/**"] }`); `src/components/InstanceCard.tsx` + `src/routes/InstanceDetail.tsx` (custom-icon precedence via `convertFileSrc(`${dataDir}/instances/${slug}/${icon}`)`, `dataDir` from the existing `["app-paths"]` query; fall back to `source.iconUrl` then placeholder) | None automated (visual). | **No** (config + frontend only) | `scripts/build.sh check` green; an instance whose `instance.icon` is set (drop a file in via CP-5 or seed manually) renders its custom image on the Home card **and** the InstanceDetail header; unset instances still show pack icon / placeholder; `scripts/build.sh build` produces a bundle that loads the local icon (asset protocol works in a real build, not just dev) |
| **CP-5** | Icon picker UI (Set / Remove) | `src/routes/InstanceDetail.tsx` (hover "Set icon…" overlay on the header icon → `open()` image filter → `setInstanceIcon`; "Remove" when `instance.icon` set → `clearInstanceIcon`; invalidate `["instance",slug]` + `["instances"]` after); `src/lib/ipc.ts` (thin wrappers for the two commands) | None automated. | **No** (commands generated at CP-3) | `scripts/build.sh check` green; picking an image sets the icon and it appears immediately (timestamped filename ⇒ no stale-cache); replacing it swaps cleanly; "Remove" reverts to pack-icon/placeholder; all reflected on Home after navigation |

---

## Per-checkpoint detail

### CP-1 — theme engine + no-flash boot
- **`index.html` inline script** (in `<head>`, before the module `<script>`), kept tiny so it
  parses and runs during HTML load — this is the FOUC guard:
  ```html
  <script>
    (function () {
      try {
        var p = localStorage.getItem("apex-theme") || "system";
        var light = p === "light" ||
          (p === "system" && matchMedia("(prefers-color-scheme: light)").matches);
        document.documentElement.classList.toggle("light", light);
      } catch (e) {}
    })();
  </script>
  ```
- **`src/lib/theme.ts`** centralizes the same resolution for runtime use: `applyTheme()` (read
  pref → toggle `.light`), `setThemePref(p)` (write `apex-theme` + `applyTheme()`),
  `getThemePref()`, and `subscribeSystem()` (a `matchMedia("(prefers-color-scheme: light)")`
  `change` listener that re-applies only while the pref is `system`).
- **`main.tsx`** calls `applyTheme()` once (idempotent with the inline script) and
  `subscribeSystem()` at startup.
- Contract reminder (design A1): **dark = no class, light = `.light` on `documentElement`** — do
  not add a `.dark` class or any `dark:` utilities; the existing `styles.css:4-45` token flip
  does all the work.

### CP-2 — Settings control
- New "Appearance" section in `Settings.tsx` (above "Behavior", `Settings.tsx:116`), styled with
  the existing `SettingRow`. A 3-segment control (System / Light / Dark); selected segment uses
  `bg-primary text-primary-foreground`. `onChange` → `setThemePref(value)` (from `theme.ts`),
  which re-themes instantly. The control's value reads `getThemePref()` (local `useState` seeded
  once). This control is **not** wired to the backend `Settings` model (design A3).

### CP-3 — backend icon copy/clear (bindings regen)
- Pure helper in `instances.rs` so it's unit-testable without Tauri:
  `write_instance_icon(inst_dir: &Path, inst: &mut Instance, src_path: &Path) -> Result<(),String>`
  — lowercase-ext allowlist `{png,jpg,jpeg,webp,gif}`, size cap (e.g. 4 MiB), copy to
  `inst_dir/icon-<unix_millis>.<ext>`, remove any preexisting `icon-*.*` (glob the dir), set
  `inst.icon = Some("icon-<ts>.<ext>")`. Sibling `clear_instance_icon_file` deletes + sets `None`.
- `lib.rs` commands `set_instance_icon(slug, src_path)` / `clear_instance_icon(slug)`: resolve
  instance dir via `store`, `read_manifest` → helper → `write_manifest`. **Sync**, off the task
  queue (instant FS op — mirrors `set_pack_lock`/`set_mod_enabled`, `lib.rs:1615-1673`).
- Tests per the checkpoint table land in `instances_tests.rs` (use `tempfile` for the dir).
- **Regen `bindings.ts`** (the only regen in this spec): `scripts/build.sh dev` → wait for
  `[bindings] exported` → stop → commit the regenerated file. Confirm `commands.setInstanceIcon`
  and `commands.clearInstanceIcon` exist; `Instance.icon` is already present (no DTO diff).

### CP-4 — asset protocol + render (config + frontend)
- `tauri.conf.json` `app.security` gains
  `"assetProtocol": { "enable": true, "scope": ["$DATA/ApexLauncher/instances/**"] }`. Keep
  `"csp": null` (no `img-src` rule blocks `asset:`). `$DATA` = `app.path().data_dir()` base, so
  the scope matches the real instance paths on all OSes (design B3 — do **not** use `$APPDATA`,
  which is bundle-id'd).
- Render sites compute:
  ```tsx
  import { convertFileSrc } from "@tauri-apps/api/core";
  const custom = inst.icon ? convertFileSrc(`${dataDir}/instances/${inst.slug}/${inst.icon}`) : null;
  const iconSrc = custom ?? source?.iconUrl ?? null;   // precedence; null → placeholder
  ```
  `dataDir` comes from the existing `getAppPaths()` query (`["app-paths"]`). Apply at
  `InstanceCard.tsx:113-125` and `InstanceDetail.tsx:255-267`. Browse cards/BrowsePackInfo are
  **untouched** (they show remote packs, not instances).
- Gate includes a real `scripts/build.sh build` because the asset protocol must also work in a
  bundled (non-dev) build, not just under the dev server.

### CP-5 — picker UI
- InstanceDetail header icon becomes hover-actionable: a small "Set icon…" overlay button →
  `open({ multiple:false, directory:false, filters:[{name:"Image",extensions:["png","jpg","jpeg","webp","gif"]}] })`
  (pattern at `JavaTab.tsx:62-78`) → `setInstanceIcon(slug, path)`. When `instance.icon` is set,
  also offer "Remove" → `clearInstanceIcon(slug)`. After either, invalidate `["instance",slug]`
  and `["instances"]` so the header + Home re-render. `dialog:default` already covers `open()`.
- `ipc.ts` thin wrappers `setInstanceIcon`/`clearInstanceIcon` over the generated `commands.*`
  (via `unwrap`).

---

## Test inventory delta (expected)
- `instances_tests.rs`: +~6 (`write_instance_icon` set / re-set-deletes-prior / bad-ext / oversize;
  `clear`; round-trip persistence).
- No frontend tests (none exist yet; planned Phase 7). Themes + icon UI are visual-gate only.

## Regeneration checklist (bindings.ts)
Regen required at **CP-3 only** (commands `set_instance_icon` / `clear_instance_icon`). CP-1, CP-2,
CP-4, CP-5 touch no generated DTO/command/event (CP-4 is config + a `convertFileSrc` call; the
theme CPs are pure frontend). The CP-3 regen: `scripts/build.sh dev` → wait for
`[bindings] exported` → stop → commit `src/lib/bindings.ts` with the Rust change.

## Change log
- 2026-06-26 — Initial spec authored (design `docs/design/theme-and-icons.md`). Not implemented.
  Decisions: themes use the existing `.light` token flip (dark = no class), tri-state
  `system|light|dark` persisted to a dedicated `apex-theme` localStorage key with an `index.html`
  inline no-flash boot script (no backend field, no bindings regen); instance icons copy into
  `<instances>/<slug>/icon-<ts>.<ext>`, persist a relative filename, display via the Tauri asset
  protocol (`assetProtocol.enable` + static `$DATA/ApexLauncher/instances/**` scope) +
  `convertFileSrc` with custom > pack > placeholder precedence; two new sync commands
  (`set_instance_icon`/`clear_instance_icon`) — single bindings regen at CP-3. Three open
  questions carried to approval (theme persistence location, light-palette polish scope, icon
  path assembly JS-join vs Rust-returned).
