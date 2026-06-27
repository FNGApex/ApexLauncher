import { useEffect, useRef, useState } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { useParams } from "react-router-dom";
import {
  AlertCircle,
  Filter,
  Loader2,
  Package,
  Search,
  X,
} from "lucide-react";
import { Link } from "react-router-dom";
import {
  listInstances,
  searchMods,
  type ProviderCommandError,
} from "@/lib/ipc";
import { META_STALE_TIME } from "@/lib/query";
import { buildInstalledIndex, isInstalled } from "@/lib/installedIndex";
import { resolveCategoriesFor } from "@/lib/categoryMap";
import { FiltersPopover, type FiltersState } from "@/components/FiltersPopover";
import { BrowseCard } from "@/components/BrowseCard";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/lib/store";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PAGE_LIMIT = 20;
const DEBOUNCE_MS = 400;

const EMPTY_FILTERS: FiltersState = {
  loaders: new Set<string>(),
  mcVersion: null,
  categories: new Set<string>(),
};

// ---------------------------------------------------------------------------
// BrowseProvider — per-provider browse page, driven by :provider route param
// ---------------------------------------------------------------------------

export function BrowseProvider() {
  const { provider } = useParams<{ provider: string }>();
  const setBrowseProvider = useUiStore((s) => s.setBrowseProvider);

  const [rawQuery, setRawQuery] = useState("");
  const [query, setQuery] = useState("");
  const [filters, setFilters] = useState<FiltersState>(EMPTY_FILTERS);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const filtersButtonRef = useRef<HTMLButtonElement | null>(null);

  // Remember last-used provider for the real providers.
  useEffect(() => {
    if (provider === "curseforge" || provider === "modrinth" || provider === "ftb" || provider === "atlauncher") {
      setBrowseProvider(provider);
    }
  }, [provider, setBrowseProvider]);

  // FTB and ATLauncher have no server-side loader/category facets — hide the filters UI for them.
  const supportsFilters = provider !== "ftb" && provider !== "atlauncher";

  // Reset filters when provider changes so stale cross-provider categories are cleared.
  useEffect(() => {
    setFilters(EMPTY_FILTERS);
    setRawQuery("");
    setQuery("");
  }, [provider]);

  // Debounce: apply the input value after DEBOUNCE_MS of inactivity.
  useEffect(() => {
    const id = setTimeout(() => setQuery(rawQuery.trim()), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [rawQuery]);

  // BR-B: build installed index from the cached ["instances"] query (no new IPC call).
  const { data: instances } = useQuery({
    queryKey: ["instances"],
    queryFn: listInstances,
    staleTime: META_STALE_TIME,
  });
  const installedIndex = instances ? buildInstalledIndex(instances) : new Map<string, string>();

  // Active-filter count for the badge.
  const activeFilterCount =
    filters.loaders.size +
    filters.categories.size +
    (filters.mcVersion != null ? 1 : 0);

  function removeLoader(l: string) {
    const next = new Set(filters.loaders);
    next.delete(l);
    setFilters((f) => ({ ...f, loaders: next }));
  }

  function removeCategory(c: string) {
    const next = new Set(filters.categories);
    next.delete(c);
    setFilters((f) => ({ ...f, categories: next }));
  }

  function removeMcVersion() {
    setFilters((f) => ({ ...f, mcVersion: null }));
  }

  // Coming-soon placeholder for unimplemented providers.
  if (provider !== "curseforge" && provider !== "modrinth" && provider !== "ftb" && provider !== "atlauncher") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 px-8 py-7">
        <Package className="size-12 text-muted" />
        <div className="text-center">
          <p className="text-lg font-semibold text-foreground">Coming soon</p>
          <p className="mt-1 text-sm text-muted">
            This provider support is not yet available.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col px-8 py-7">
      {/* Search + Filters bar */}
      <div className="mb-4">
        <div className="flex items-center gap-2">
          {/* Search field */}
          <div className="flex min-w-0 flex-1 items-center gap-3 rounded-xl border border-border bg-surface px-4 py-2.5 focus-within:border-primary transition-colors">
            <Search className="size-5 shrink-0 text-muted" />
            <input
              className="w-full bg-transparent text-base outline-none placeholder:text-muted"
              placeholder="Search modpacks…"
              value={rawQuery}
              onChange={(e) => setRawQuery(e.target.value)}
            />
            {rawQuery.length > 0 && (
              <button
                onClick={() => setRawQuery("")}
                className="shrink-0 rounded-md p-0.5 text-muted hover:text-foreground"
                title="Clear search"
              >
                <X className="size-4" />
              </button>
            )}
          </div>

          {/* Filters button — with active-filter count badge (hidden for FTB) */}
          <div className={cn("relative", !supportsFilters && "hidden")}>
            <button
              ref={filtersButtonRef}
              onClick={() => setFiltersOpen((o) => !o)}
              className={cn(
                "flex items-center gap-2 rounded-xl border px-4 py-2.5 text-sm font-medium transition-colors",
                filtersOpen || activeFilterCount > 0
                  ? "border-primary bg-primary/10 text-primary"
                  : "border-border bg-surface text-foreground hover:border-primary/50",
              )}
            >
              <Filter className="size-4" />
              Filters
              {activeFilterCount > 0 && (
                <span className="flex size-5 items-center justify-center rounded-full bg-primary text-xs font-semibold text-primary-foreground">
                  {activeFilterCount}
                </span>
              )}
            </button>

            {/* Anchored popover */}
            <FiltersPopover
              anchorRef={filtersButtonRef}
              open={filtersOpen}
              onClose={() => setFiltersOpen(false)}
              filters={filters}
              onFiltersChange={setFilters}
              provider={provider}
            />
          </div>
        </div>

        {/* Applied-filter chips */}
        {activeFilterCount > 0 && (
          <div className="mt-2 flex flex-wrap gap-1.5">
            {filters.mcVersion != null && (
              <FilterChip
                label={`MC ${filters.mcVersion}`}
                onRemove={removeMcVersion}
              />
            )}
            {Array.from(filters.loaders).map((l) => (
              <FilterChip
                key={l}
                label={l[0].toUpperCase() + l.slice(1)}
                onRemove={() => removeLoader(l)}
              />
            ))}
            {Array.from(filters.categories).map((c) => (
              <FilterChip key={c} label={c} onRemove={() => removeCategory(c)} />
            ))}
          </div>
        )}
      </div>

      {/* Results area */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <SingleProviderFeed
          provider={provider}
          query={query}
          filters={filters}
          installedIndex={installedIndex}
        />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Applied-filter chip
// ---------------------------------------------------------------------------

function FilterChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <span className="inline-flex items-center gap-1 rounded-lg border border-primary/30 bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary">
      {label}
      <button
        onClick={onRemove}
        className="ml-0.5 rounded-sm hover:text-foreground"
        title="Remove filter"
      >
        <X className="size-3" />
      </button>
    </span>
  );
}

// ---------------------------------------------------------------------------
// Single-provider infinite feed
// ---------------------------------------------------------------------------

interface SingleProviderFeedProps {
  provider: "curseforge" | "modrinth" | "ftb" | "atlauncher";
  query: string;
  filters: FiltersState;
  installedIndex: Map<string, string>;
}

function SingleProviderFeed({
  provider,
  query,
  filters,
  installedIndex,
}: SingleProviderFeedProps) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  const loadersArr = Array.from(filters.loaders);

  // Resolve categories for the active provider.
  const resolvedCategories = resolveCategoriesFor(provider, filters.categories);
  // CurseForge ANDs categoryIds → send at most one.
  const queryCategories =
    provider === "curseforge"
      ? resolvedCategories.length > 0
        ? [resolvedCategories[0]]
        : []
      : resolvedCategories;

  // Loader arg: CurseForge singular-or-Any rule (>1 selected → omit); Modrinth ORs all.
  const queryLoaders =
    provider === "curseforge"
      ? loadersArr.length === 1
        ? loadersArr
        : []
      : loadersArr;

  const queryKey = [
    "modpacks",
    provider,
    query,
    filters.mcVersion,
    loadersArr.join(","),
    queryCategories.join(","),
  ] as const;

  const feed = useInfiniteQuery({
    queryKey,
    queryFn: ({ pageParam }) =>
      searchMods(
        provider,
        query,
        filters.mcVersion,
        null,
        pageParam,
        PAGE_LIMIT,
        "modpack",
        queryLoaders,
        queryCategories,
      ),
    initialPageParam: 0,
    getNextPageParam: (lastPage) => {
      if (lastPage.hits.length === 0) return undefined;
      const fetched = lastPage.offset + lastPage.hits.length;
      return fetched < lastPage.total ? fetched : undefined;
    },
    staleTime: 30_000,
  });

  // Infinite scroll sentinel
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (
          entries[0].isIntersecting &&
          feed.hasNextPage &&
          !feed.isFetchingNextPage
        ) {
          feed.fetchNextPage();
        }
      },
      { threshold: 0.1 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [feed.hasNextPage, feed.isFetchingNextPage, feed.fetchNextPage]);

  const cfKeyMissing =
    feed.isError &&
    isProviderCommandError(feed.error) &&
    feed.error.kind === "key_missing";

  const otherError = feed.isError && !cfKeyMissing;

  const hits = feed.data?.pages.flatMap((p) => p.hits) ?? [];

  if (feed.isLoading) {
    return (
      <div className="flex items-center gap-2 py-8 text-sm text-muted">
        <Loader2 className="size-4 animate-spin" />
        Searching…
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {/* CF key-missing notice */}
      {cfKeyMissing && (
        <div className="flex items-center gap-3 rounded-xl border border-border bg-surface/60 px-4 py-3 text-sm text-muted">
          <AlertCircle className="size-4 shrink-0" />
          <span>
            CurseForge results hidden — add an API key in{" "}
            <Link to="/settings" className="text-primary hover:underline">
              Settings
            </Link>
            .
          </span>
        </div>
      )}

      {/* Other error */}
      {otherError && (
        <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          <AlertCircle className="size-4 shrink-0" />
          <span>
            Error:{" "}
            {isProviderCommandError(feed.error)
              ? feed.error.message
              : String(feed.error)}
          </span>
        </div>
      )}

      {/* Responsive grid of cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {hits.map((pack) => (
          <BrowseCard
            key={`${pack.provider}:${pack.id}`}
            pack={pack}
            installedSlug={isInstalled(installedIndex, pack.provider, pack.id)}
          />
        ))}
      </div>

      {/* Empty state */}
      {hits.length === 0 && !feed.isLoading && !feed.isFetching && (
        <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-12 text-center text-sm text-muted">
          <Package className="mb-3 size-8" />
          No modpacks found
        </div>
      )}

      {/* Scroll sentinel */}
      <div ref={sentinelRef} className="h-4" />

      {feed.isFetchingNextPage && (
        <div className="flex items-center justify-center gap-2 py-2 text-sm text-muted">
          <Loader2 className="size-4 animate-spin" />
          Loading more…
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function isProviderCommandError(v: unknown): v is ProviderCommandError {
  return (
    typeof v === "object" &&
    v !== null &&
    "kind" in v &&
    "message" in v &&
    typeof v.kind === "string" &&
    typeof v.message === "string"
  );
}
