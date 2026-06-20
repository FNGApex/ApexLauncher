# Modding lock + Apple toggle + grandiose bar (spec)

Status: in progress (2026-06-20). Branch: `ui-overhaul`. Human batch.

## Goals
1. **Remove the pack-source panel** at the bottom of the Info tab (the persistent bar + version/update modal
   now own version/update/source).
2. **Move the Pack Lock toggle** into the Mods (modding) tab.
3. **Auto-lock modpacks**: instances installed AS a modpack (import/Browse-install) start `pack_locked = true`.
4. **Rename "Add mod" → "Add Mods"**; when locked, **prevent adding** (gate the add flow).
5. **Enable/Disable**: gray out when locked; switch from text buttons to an **Apple-style toggle switch**
   (reuse the `Toggle` component).
6. **Persistent Bar grandiose**: much bigger pack picture, big text, bold/impressive layout.

## LB — backend (auto-lock modpacks)
| CP | Deliverable | Files |
|----|-------------|-------|
| LB-1 | Every path that creates an instance from a MODPACK (ImportMrpackJob, ImportCfZipJob, and the Browse `install_modpack` flow) sets the new instance's `pack_locked = true` (set it where the `Source` is assigned / right before the final manifest save). Blank/manual instance creation (`create_instance`) stays `false`. Add a test asserting a modpack-import path yields `pack_locked = true`. NO bindings change (pack_locked exists). | `src-tauri/src/lib.rs` (the 3 modpack jobs) + test |

## LF — frontend
| CP | Deliverable | Files |
|----|-------------|-------|
| LF-1 | **InfoTab:** remove `<PackSourcePanel>` entirely (and its empty-state). Info tab now shows only the pack description (+ the "not installed from a provider" empty state). Delete the `PackSourcePanel` component if unused after this (grep). | `src/routes/instance-tabs/InfoTab.tsx`, `src/routes/InstanceDetail.tsx` (PackSourcePanel) |
| LF-2 | **Mods tab Pack Lock:** add a Pack Lock toggle to the Modlist/ManageInstallsPanel header — an Apple-style `<Toggle>` (reuse `src/components/Toggle.tsx`) labeled "Pack locked", calling `setPackLock(slug, !locked)` + invalidate. Keep the existing pack-locked notice. | `src/routes/InstanceDetail.tsx` (ManageInstallsPanel / ModlistTab) |
| LF-3 | **Rename + add-gate:** the add sub-tab/button label "Add mod" → **"Add Mods"**. When `packLocked`, the Add Mods flow must prevent adds (disable the Add buttons + show the locked notice — already partly gated; ensure the whole add UI is blocked/greyed when locked). | `src/routes/InstanceDetail.tsx` (AddModTab / ModSearchCard) |
| LF-4 | **Enable/Disable → Apple toggle:** in `ModRow`, replace the "Enable"/"Disable" TEXT button with an Apple-style toggle switch (reuse/extend `Toggle`, or a small inline switch) bound to `!mod.disabled`; on change → `setModEnabled(slug, fileName, …)`. **Grey it out (disabled) when `packLocked`.** Keep update/remove actions (also greyed when locked). | `src/routes/InstanceDetail.tsx` (ModRow) |
| LF-5 | **Grandiose Persistent Bar:** make the header big and impressive — large pack image (e.g. size-24/28), large bold name (text-3xl+), prominent author + version chip, generous padding, strong visual hierarchy. Keep all current functionality (icon/name/author/version-chip/pack-source/updatable-banner/launch-stop) — just scale it up and polish. | `src/routes/InstanceDetail.tsx` (header) |

## Notes
- Locked modpack UX: a locked pack shows greyed enable/disable toggles + blocked Add Mods + the lock notice;
  the Mods-tab Pack Lock toggle lets the user unlock to modify. Existing instances aren't retroactively
  locked (install-time only); user can lock via the toggle.
- `Toggle` (`src/components/Toggle.tsx`) is the reusable Apple-style switch from WS-C/WS-E.
- Frontend gate: `scripts\apex-build.bat check` + dev-window smoke.
