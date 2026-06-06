import { QueryClient } from "@tanstack/react-query";

/**
 * Version & loader metadata changes rarely and is already disk-cached in Rust
 * (`core/meta.rs`, 6h TTL). Mirror that on the client so dropdowns open instantly
 * and we don't re-invoke the backend on every modal open / remount.
 */
export const META_STALE_TIME = 6 * 60 * 60 * 1000; // 6h

/**
 * Shared TanStack Query client. Defaults favor a warm local cache:
 * - `staleTime` avoids refetch churn when navigating between pages,
 * - long `gcTime` keeps results resident across route changes,
 * - mutations still call `invalidateQueries` to force a refresh when data changes.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
      staleTime: 30_000,
      gcTime: 24 * 60 * 60 * 1000, // 24h
    },
  },
});
