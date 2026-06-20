/**
 * BR-B: Pure installed-index helpers.
 *
 * Builds a `Map<"<normProvider>:<projectId>", slug>` from a list of instances.
 * CRITICAL: provider strings are normalized to lowercase before keying because
 * on-disk `Source.provider` is inconsistently cased:
 *   - Browse one-click install  → "curseForge" (camelCase)
 *   - CF .zip import            → "curseforge" (lowercase)
 *   - Modrinth (all paths)      → "modrinth"
 * Card `pack.provider` is the ProviderKind wire value ("modrinth" / "curseForge").
 * Normalizing both sides via `.toLowerCase()` makes all three cases match (design §2.1).
 *
 * No React or IPC imports — pure functions, safe to call outside component trees.
 */

import type { Instance } from "@/lib/bindings";

type InstalledIndex = Map<string, string>; // key → instance slug

/** Build an index from a list of instances. */
export function buildInstalledIndex(instances: Instance[]): InstalledIndex {
  const map: InstalledIndex = new Map();
  for (const inst of instances) {
    if (inst.source == null) continue;
    const key = `${inst.source.provider.toLowerCase()}:${inst.source.projectId}`;
    map.set(key, inst.slug);
  }
  return map;
}

/**
 * Return the slug of the installed instance if `provider:id` is in the index, else null.
 * Both `provider` and the stored provider are normalized to lowercase before comparison.
 */
export function isInstalled(
  index: InstalledIndex,
  provider: string,
  id: string,
): string | null {
  return index.get(`${provider.toLowerCase()}:${id}`) ?? null;
}
