/**
 * BR-C: Anchored Filters popover.
 *
 * Opened by a "Filters" button in Browse.tsx. Closes on outside-click or Esc.
 * Apply-on-change (Q4): state changes propagate upward immediately via callbacks;
 * no explicit Apply button required.
 *
 * Groups:
 *   1. Loaders — fabric / quilt / forge / neoforge checkboxes.
 *   2. Game version — single <select> from the ["mc-versions"] cache (no new fetch).
 *   3. Categories — checkbox chips from categoryMap.ts, filtered to the active provider.
 *      Only rows where the provider's value is non-null are shown (no provider tags
 *      needed — it's a single-provider view).
 */

import { useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { listMinecraftVersions } from "@/lib/ipc";
import { META_STALE_TIME } from "@/lib/query";
import { CATEGORY_MAP } from "@/lib/categoryMap";
import { cn } from "@/lib/utils";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface FiltersState {
  loaders: Set<string>;
  mcVersion: string | null;
  categories: Set<string>;
}

export interface FiltersPopoverProps {
  /** Anchor element — popover is positioned below this. */
  anchorRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  filters: FiltersState;
  onFiltersChange: (next: FiltersState) => void;
  /** Active provider — determines which categories are shown. */
  provider: "curseforge" | "modrinth";
}

const LOADER_OPTIONS = [
  { id: "fabric",   label: "Fabric"   },
  { id: "quilt",    label: "Quilt"    },
  { id: "forge",    label: "Forge"    },
  { id: "neoforge", label: "NeoForge" },
];

// ---------------------------------------------------------------------------
// FiltersPopover
// ---------------------------------------------------------------------------

export function FiltersPopover({
  anchorRef,
  open,
  onClose,
  filters,
  onFiltersChange,
  provider,
}: FiltersPopoverProps) {
  const popoverRef = useRef<HTMLDivElement | null>(null);

  const { data: versions } = useQuery({
    queryKey: ["mc-versions"],
    queryFn: listMinecraftVersions,
    staleTime: META_STALE_TIME,
  });

  // Close on outside-click
  useEffect(() => {
    if (!open) return;
    function handlePointerDown(e: PointerEvent) {
      const target = e.target as Node;
      if (
        popoverRef.current &&
        !popoverRef.current.contains(target) &&
        anchorRef.current &&
        !anchorRef.current.contains(target)
      ) {
        onClose();
      }
    }
    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [open, onClose, anchorRef]);

  // Close on Esc
  useEffect(() => {
    if (!open) return;
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  // -------------------------------------------------------------------------
  // Helpers
  // -------------------------------------------------------------------------

  function setLoader(id: string, checked: boolean) {
    const next = new Set(filters.loaders);
    if (checked) next.add(id);
    else next.delete(id);
    onFiltersChange({ ...filters, loaders: next });
  }

  function setMcVersion(v: string | null) {
    onFiltersChange({ ...filters, mcVersion: v });
  }

  function setCategory(label: string, checked: boolean) {
    const next = new Set(filters.categories);
    if (checked) next.add(label);
    else next.delete(label);
    onFiltersChange({ ...filters, categories: next });
  }

  function clearAll() {
    onFiltersChange({ loaders: new Set(), mcVersion: null, categories: new Set() });
  }

  const hasAny =
    filters.loaders.size > 0 ||
    filters.mcVersion != null ||
    filters.categories.size > 0;

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  return (
    <div
      ref={popoverRef}
      // Right-aligned under the Filters button so the panel opens leftward and
      // never overflows the right edge of the page.
      className="absolute right-0 z-40 mt-1 w-80 rounded-xl border border-border bg-surface shadow-xl"
      style={{ top: "100%" }}
    >
      <div className="flex items-center justify-between border-b border-border px-4 py-3">
        <span className="text-sm font-medium">Filters</span>
        {hasAny && (
          <button
            onClick={clearAll}
            className="text-xs text-muted hover:text-foreground"
          >
            Clear all
          </button>
        )}
      </div>

      <div className="divide-y divide-border">
        {/* Game version */}
        <section className="px-4 py-3">
          <p className="mb-2 text-xs font-medium text-muted uppercase tracking-wider">
            Game Version
          </p>
          <select
            value={filters.mcVersion ?? ""}
            onChange={(e) => setMcVersion(e.target.value || null)}
            className="input w-full"
          >
            <option value="">Any version</option>
            {versions?.map((v) => (
              <option key={v.id} value={v.id}>
                {v.id}
              </option>
            ))}
          </select>
        </section>

        {/* Loaders */}
        <section className="px-4 py-3">
          <p className="mb-2 text-xs font-medium text-muted uppercase tracking-wider">
            Loader
          </p>
          <div className="flex flex-wrap gap-2">
            {LOADER_OPTIONS.map(({ id, label }) => {
              const checked = filters.loaders.has(id);
              return (
                <label
                  key={id}
                  className={cn(
                    "flex cursor-pointer select-none items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs font-medium transition-colors",
                    checked
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border text-muted hover:border-primary/50 hover:text-foreground",
                  )}
                >
                  <input
                    type="checkbox"
                    className="sr-only"
                    checked={checked}
                    onChange={(e) => setLoader(id, e.target.checked)}
                  />
                  {label}
                </label>
              );
            })}
          </div>
        </section>

        {/* Categories — filtered to the active provider */}
        <section className="px-4 py-3">
          <p className="mb-2 text-xs font-medium text-muted uppercase tracking-wider">
            Category
          </p>
          <div className="flex flex-wrap gap-1.5">
            {CATEGORY_MAP.filter((row) =>
              provider === "modrinth" ? row.modrinth != null : row.cfId != null,
            ).map((row) => {
              const checked = filters.categories.has(row.label);
              return (
                <label
                  key={row.label}
                  className={cn(
                    "flex cursor-pointer select-none items-center gap-1 rounded-md border px-2 py-1 text-xs font-medium transition-colors",
                    checked
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border text-muted hover:border-primary/50 hover:text-foreground",
                  )}
                >
                  <input
                    type="checkbox"
                    className="sr-only"
                    checked={checked}
                    onChange={(e) => setCategory(row.label, e.target.checked)}
                  />
                  {row.label}
                </label>
              );
            })}
          </div>
        </section>
      </div>
    </div>
  );
}
