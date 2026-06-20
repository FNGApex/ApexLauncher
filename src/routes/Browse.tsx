import { useEffect, useRef, useState } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
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
// Root component
// ---------------------------------------------------------------------------

export function Browse() {
  const [rawQuery, setRawQuery] = useState("");
  const [query, setQuery] = useState("");
  const [filters, setFilters] = useState<FiltersState>(EMPTY_FILTERS);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [cfNoticeDismissed, setCfNoticeDismissed] = useState(false);
  const filtersButtonRef = useRef<HTMLButtonElement | null>(null);

  // Debounce: apply the input value after DEBOUNCE_MS of inactivity.
  useEffect(() => {
    const id = setTimeout(() => setQuery(rawQuery.trim()), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [rawQuery]);

  // Dismiss CF notice whenever filter state or query changes.
  useEffect(() => {
    setCfNoticeDismissed(false);
  }, [query, filters]);

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

  return (
    <div className="flex h-full flex-col px-8 py-7">
      {/* Search + Filters bar — BR-D: prominent, centered search field */}
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

          {/* Filters button — BR-C: with active-filter count badge */}
          <div className="relative">
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
            />
          </div>
        </div>

        {/* Applied-filter chips — BR-C */}
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
        <MergedFeed
          query={query}
          filters={filters}
          installedIndex={installedIndex}
          cfNoticeDismissed={cfNoticeDismissed}
          onDismissCfNotice={() => setCfNoticeDismissed(true)}
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
// Merged feed: two independent infinite queries, merged + sorted client-side
// ---------------------------------------------------------------------------

interface MergedFeedProps {
  query: string;
  filters: FiltersState;
  installedIndex: Map<string, string>;
  cfNoticeDismissed: boolean;
  onDismissCfNotice: () => void;
}

function MergedFeed({
  query,
  filters,
  installedIndex,
  cfNoticeDismissed,
  onDismissCfNotice,
}: MergedFeedProps) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  // Derive per-provider values from the unified filter state (BR-C-4).
  const loadersArr = Array.from(filters.loaders);
  const mrCategories = resolveCategoriesFor("modrinth", filters.categories);
  // CF ANDs categoryIds → send at most one (design §5.3 / Q7).
  const cfCategoriesAll = resolveCategoriesFor("curseforge", filters.categories);
  const cfCategories = cfCategoriesAll.length > 0 ? [cfCategoriesAll[0]] : [];

  // Provider compatibility: a provider is shown only if it supports ALL selected
  // categories. So an exclusive single-provider category (CF-only Skyblock,
  // Modrinth-only Optimization) hides the other provider's feed entirely.
  const catCount = filters.categories.size;
  const mrCompatible = catCount === 0 || mrCategories.length === catCount;
  const cfCompatible = catCount === 0 || cfCategoriesAll.length === catCount;

  // Stable query-key arrays (filters wired into keys so changes refetch).
  const modrinthKey = [
    "modpacks", "modrinth", query,
    filters.mcVersion,
    loadersArr.join(","),
    mrCategories.join(","),
  ] as const;

  const curseforgeKey = [
    "modpacks", "curseforge", query,
    filters.mcVersion,
    loadersArr.join(","),
    cfCategories.join(","),
  ] as const;

  const modrinth = useInfiniteQuery({
    queryKey: modrinthKey,
    enabled: mrCompatible,
    queryFn: ({ pageParam }) =>
      searchMods(
        "modrinth",
        query,
        filters.mcVersion,
        null,           // single-loader arg unused — pass multi via loaders[]
        pageParam,
        PAGE_LIMIT,
        "modpack",
        loadersArr,
        mrCategories,
      ),
    initialPageParam: 0,
    getNextPageParam: (lastPage) => {
      if (lastPage.hits.length === 0) return undefined;
      const fetched = lastPage.offset + lastPage.hits.length;
      return fetched < lastPage.total ? fetched : undefined;
    },
    staleTime: 30_000,
  });

  const curseforge = useInfiniteQuery({
    queryKey: curseforgeKey,
    enabled: cfCompatible,
    queryFn: ({ pageParam }) =>
      searchMods(
        "curseforge",
        query,
        filters.mcVersion,
        null,
        pageParam,
        PAGE_LIMIT,
        "modpack",
        // Q1: singular-or-Any — if >1 loader selected CF gets Any (empty → omit)
        loadersArr.length === 1 ? loadersArr : [],
        cfCategories,
      ),
    initialPageParam: 0,
    getNextPageParam: (lastPage) => {
      if (lastPage.hits.length === 0) return undefined;
      const fetched = lastPage.offset + lastPage.hits.length;
      return fetched < lastPage.total ? fetched : undefined;
    },
    staleTime: 30_000,
  });

  // CF key-missing is a non-fatal condition — surface an inline notice only.
  const cfKeyMissing =
    curseforge.isError &&
    isProviderCommandError(curseforge.error) &&
    curseforge.error.kind === "key_missing";

  const cfOtherError = curseforge.isError && !cfKeyMissing;

  // Infinite scroll sentinel
  const fetchBoth = () => {
    if (modrinth.hasNextPage && !modrinth.isFetchingNextPage) {
      modrinth.fetchNextPage();
    }
    if (curseforge.hasNextPage && !curseforge.isFetchingNextPage) {
      curseforge.fetchNextPage();
    }
  };

  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) fetchBoth();
      },
      { threshold: 0.1 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [
    modrinth.hasNextPage,
    modrinth.isFetchingNextPage,
    curseforge.hasNextPage,
    curseforge.isFetchingNextPage,
    modrinth.fetchNextPage,
    curseforge.fetchNextPage,
  ]);

  // Merge, dedupe, sort by downloads desc. Incompatible providers contribute nothing.
  const modrinthHits = mrCompatible
    ? modrinth.data?.pages.flatMap((p) => p.hits) ?? []
    : [];
  const curseforgeHits = !cfCompatible || curseforge.isError
    ? []
    : curseforge.data?.pages.flatMap((p) => p.hits) ?? [];

  const seen = new Set<string>();
  const merged = [] as typeof modrinthHits;

  for (const hit of [...modrinthHits, ...curseforgeHits]) {
    const key = `${hit.provider}:${hit.id}`;
    if (!seen.has(key)) {
      seen.add(key);
      merged.push(hit);
    }
  }
  merged.sort((a, b) => b.downloads - a.downloads);

  // Only the SHOWN providers gate the loading / empty states.
  const mrShown = mrCompatible;
  const cfShown = cfCompatible && !cfKeyMissing;
  const isInitialLoading =
    (mrShown || cfShown) &&
    (!mrShown || modrinth.isLoading) &&
    (!cfShown || curseforge.isLoading);
  const isFetchingMore =
    modrinth.isFetchingNextPage || curseforge.isFetchingNextPage;

  if (isInitialLoading) {
    return (
      <div className="flex items-center gap-2 py-8 text-sm text-muted">
        <Loader2 className="size-4 animate-spin" />
        Searching…
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {/* CF key-missing inline notice */}
      {cfKeyMissing && !cfNoticeDismissed && (
        <div className="flex items-center justify-between gap-3 rounded-xl border border-border bg-surface/60 px-4 py-3 text-sm text-muted">
          <span>
            CurseForge results hidden — add an API key in{" "}
            <Link to="/settings" className="text-primary hover:underline">
              Settings
            </Link>
            .
          </span>
          <button
            onClick={onDismissCfNotice}
            className="shrink-0 rounded-md p-0.5 hover:bg-surface-2 hover:text-foreground"
            title="Dismiss"
          >
            <X className="size-4" />
          </button>
        </div>
      )}

      {/* CF non-key error */}
      {cfOtherError && (
        <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          <AlertCircle className="size-4 shrink-0" />
          <span>
            CurseForge error:{" "}
            {isProviderCommandError(curseforge.error)
              ? curseforge.error.message
              : String(curseforge.error)}
          </span>
        </div>
      )}

      {/* Modrinth error */}
      {modrinth.isError && (
        <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          <AlertCircle className="size-4 shrink-0" />
          <span>
            Modrinth error:{" "}
            {isProviderCommandError(modrinth.error)
              ? modrinth.error.message
              : String(modrinth.error)}
          </span>
        </div>
      )}

      {/* BR-D: responsive grid of larger cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {merged.map((pack) => (
          <BrowseCard
            key={`${pack.provider}:${pack.id}`}
            pack={pack}
            installedSlug={isInstalled(installedIndex, pack.provider, pack.id)}
          />
        ))}
      </div>

      {/* Empty state */}
      {merged.length === 0 &&
        (!mrShown || !modrinth.isLoading) &&
        (!cfShown || !curseforge.isLoading) && (
          <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-12 text-center text-sm text-muted">
            <Package className="mb-3 size-8" />
            No modpacks found
          </div>
        )}

      {/* Scroll sentinel */}
      <div ref={sentinelRef} className="h-4" />

      {isFetchingMore && (
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
