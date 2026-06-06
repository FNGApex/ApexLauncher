import type { QueryClient } from "@tanstack/react-query";
import { getLoaders, listInstances, listMinecraftVersions } from "@/lib/ipc";
import { META_STALE_TIME } from "@/lib/query";

/**
 * Warm the cache at app start so the first paint and the New Instance modal are
 * instant:
 *  - the instance list (local disk read),
 *  - the Minecraft release list, and
 *  - the loaders + build numbers for the *latest* version.
 *
 * Fire-and-forget — call once on startup; failures (e.g. offline) are swallowed
 * and the relevant view surfaces its own error/loading state on demand.
 */
export async function prefetchStartupData(qc: QueryClient): Promise<void> {
  void qc.prefetchQuery({ queryKey: ["instances"], queryFn: listInstances });

  try {
    const versions = await qc.fetchQuery({
      queryKey: ["mc-versions"],
      queryFn: listMinecraftVersions,
      staleTime: META_STALE_TIME,
    });
    const latest = versions[0]?.id;
    if (latest) {
      void qc.prefetchQuery({
        queryKey: ["loaders", latest],
        queryFn: () => getLoaders(latest),
        staleTime: META_STALE_TIME,
      });
    }
  } catch {
    // Offline / fetch error — the modal will retry and show its own error.
  }
}
