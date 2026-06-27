/**
 * BR-C: Unified category mapping table (design §4.2).
 *
 * Each row maps a human-readable label to per-provider values:
 *   - `modrinth`: the Modrinth facet string (passed as `categories:<value>` in the
 *     inner OR array). `null` = this category has no Modrinth equivalent.
 *   - `cfId`: the CurseForge numeric category id (as a string). `null` = no CF equivalent.
 *
 * Single-provider rows (exactly one side non-null) are tagged so the UI can show a
 * provider glyph (Q6: show all, tag single-provider ones).
 *
 * `resolveCategoriesFor(provider, selectedLabels)` returns the values for that provider,
 * dropping labels with no value for the provider. The caller (Browse.tsx) sends at most
 * ONE CF value (CF ANDs categoryIds → >1 yields near-empty feed, per Q7/design §5.3).
 */

export type CategoryRow = {
  label: string;
  modrinth: string | null;
  cfId: string | null;
};

/** Complete unified category table — 9 shared + 1 Modrinth-only + 10 CF-only = 20 rows. */
export const CATEGORY_MAP: CategoryRow[] = [
  // --- 9 shared rows (strong or approximate overlap) ---
  { label: "Adventure & RPG",    modrinth: "adventure",    cfId: "4475" },
  { label: "Combat / PvP",       modrinth: "combat",       cfId: "4483" },
  { label: "Magic",              modrinth: "magic",        cfId: "4473" },
  { label: "Tech",               modrinth: "technology",   cfId: "4472" },
  { label: "Quests",             modrinth: "quests",       cfId: "4478" },
  { label: "Multiplayer",        modrinth: "multiplayer",  cfId: "4484" },
  { label: "Kitchen Sink / Large", modrinth: "kitchen-sink", cfId: "4482" },
  { label: "Lightweight",        modrinth: "lightweight",  cfId: "4481" },
  { label: "Challenging",        modrinth: "challenging",  cfId: "4479" },
  // --- 1 Modrinth-only ---
  { label: "Optimization",       modrinth: "optimization", cfId: null  },
  // --- 10 CF-only ---
  { label: "Exploration",        modrinth: null,           cfId: "4476" },
  { label: "Skyblock",           modrinth: null,           cfId: "4736" },
  { label: "Sci-Fi",             modrinth: null,           cfId: "4474" },
  { label: "Map Based",          modrinth: null,           cfId: "4480" },
  { label: "Mini Game",          modrinth: null,           cfId: "4477" },
  { label: "Horror",             modrinth: null,           cfId: "7418" },
  { label: "Vanilla+",           modrinth: null,           cfId: "5128" },
  { label: "FTB Official",       modrinth: null,           cfId: "4487" },
  { label: "RLCraft",            modrinth: null,           cfId: "10683" },
  { label: "Expert",             modrinth: null,           cfId: "9243" },
];

/**
 * Return whether a row is single-provider (only one side has a value).
 * The UI uses this to tag the chip with a provider glyph (Q6).
 */
export function isSingleProvider(row: CategoryRow): boolean {
  return (row.modrinth == null) !== (row.cfId == null);
}

/**
 * Resolve a set of selected unified labels into per-provider values.
 *
 * For `"modrinth"`: returns Modrinth facet strings for all selected labels that have one.
 * For `"curseforge"`: returns CF id strings for all selected labels that have one.
 *   NOTE: the caller (Browse.tsx) must send at most ONE CF value to avoid the CF AND
 *   semantics yielding a near-empty feed (design §5.3 / Q7). This function returns all
 *   matching CF values; Browse takes `[0]` for the CF query.
 *
 * `provider` is the lowercase routing string ("modrinth" / "curseforge").
 */
export function resolveCategoriesFor(
  provider: "modrinth" | "curseforge" | "ftb" | "atlauncher",
  selectedLabels: Iterable<string>,
): string[] {
  const selected = new Set(selectedLabels);
  const result: string[] = [];
  for (const row of CATEGORY_MAP) {
    if (!selected.has(row.label)) continue;
    if (provider === "modrinth" && row.modrinth != null) {
      result.push(row.modrinth);
    } else if (provider === "curseforge" && row.cfId != null) {
      result.push(row.cfId);
    }
  }
  return result;
}
