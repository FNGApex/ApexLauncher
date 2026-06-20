import { useEffect, useRef, useState } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { AlertCircle, Download, ExternalLink, Loader2, Package, Search, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Link } from "react-router-dom";
import {
  getLoaders,
  listMinecraftVersions,
  searchMods,
  type ProjectSummary,
  type ProviderCommandError,
} from "@/lib/ipc";
import { META_STALE_TIME } from "@/lib/query";
import { ProviderBadge } from "@/components/ProviderBadge";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PAGE_LIMIT = 20;
const DEBOUNCE_MS = 400;

// ---------------------------------------------------------------------------
// Root component
// ---------------------------------------------------------------------------

export function Browse() {
  const [rawQuery, setRawQuery] = useState("");
  const [query, setQuery] = useState("");
  const [mcVersion, setMcVersion] = useState<string | null>(null);
  const [loader, setLoader] = useState<string | null>(null);
  const [cfNoticeDismissed, setCfNoticeDismissed] = useState(false);

  // Debounce: apply the input value after DEBOUNCE_MS of inactivity.
  useEffect(() => {
    const id = setTimeout(() => setQuery(rawQuery.trim()), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [rawQuery]);

  // Reset loader when MC version changes (selected loader may not exist for new version).
  function handleMcVersionChange(v: string | null) {
    setMcVersion(v);
    setLoader(null);
  }

  // Dismiss notice resets when CF error state might have changed (new query/filters).
  useEffect(() => {
    setCfNoticeDismissed(false);
  }, [query, mcVersion, loader]);

  return (
    <div className="flex h-full flex-col px-8 py-7">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold">Browse</h1>
        <p className="text-sm text-muted">Search modpacks across providers</p>
      </header>

      {/* Search bar */}
      <div className="mb-4 flex min-w-0 items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2">
        <Search className="size-4 shrink-0 text-muted" />
        <input
          className="w-full bg-transparent text-sm outline-none placeholder:text-muted"
          placeholder="Search modpacks…"
          value={rawQuery}
          onChange={(e) => setRawQuery(e.target.value)}
        />
      </div>

      {/* Facet row */}
      <FacetRow
        mcVersion={mcVersion}
        loader={loader}
        onMcVersionChange={handleMcVersionChange}
        onLoaderChange={setLoader}
      />

      {/* Results area */}
      <div className="mt-5 min-h-0 flex-1 overflow-y-auto">
        <MergedFeed
          query={query}
          mcVersion={mcVersion}
          loader={loader}
          cfNoticeDismissed={cfNoticeDismissed}
          onDismissCfNotice={() => setCfNoticeDismissed(true)}
        />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Facet selectors
// ---------------------------------------------------------------------------

interface FacetRowProps {
  mcVersion: string | null;
  loader: string | null;
  onMcVersionChange: (v: string | null) => void;
  onLoaderChange: (v: string | null) => void;
}

function FacetRow({ mcVersion, loader, onMcVersionChange, onLoaderChange }: FacetRowProps) {
  const { data: versions } = useQuery({
    // Canonical key shared with prefetch.ts + NewInstanceModal so Browse reuses
    // the startup-warmed MC-versions cache instead of refetching (was "mcVersions").
    queryKey: ["mc-versions"],
    queryFn: listMinecraftVersions,
    staleTime: META_STALE_TIME,
  });

  const { data: loaderOptions } = useQuery({
    queryKey: ["loaders", mcVersion ?? ""],
    queryFn: () => getLoaders(mcVersion ?? ""),
    enabled: mcVersion != null && mcVersion !== "",
    staleTime: META_STALE_TIME,
  });

  const loaderNames = loaderOptions?.flatMap((opt) =>
    opt.kind !== "vanilla" ? [opt.kind] : [],
  ) ?? [];

  return (
    <div className="flex flex-wrap gap-2">
      <select
        value={mcVersion ?? ""}
        onChange={(e) => onMcVersionChange(e.target.value || null)}
        className="input w-auto min-w-[140px]"
      >
        <option value="">Any MC version</option>
        {versions?.map((v) => (
          <option key={v.id} value={v.id}>
            {v.id}
          </option>
        ))}
      </select>

      <select
        value={loader ?? ""}
        onChange={(e) => onLoaderChange(e.target.value || null)}
        disabled={loaderNames.length === 0}
        className="input w-auto min-w-[120px] disabled:opacity-50"
      >
        <option value="">Any loader</option>
        {loaderNames.map((name) => (
          <option key={name} value={name}>
            {name[0].toUpperCase() + name.slice(1)}
          </option>
        ))}
      </select>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Merged feed: two independent infinite queries, merged + sorted client-side
// ---------------------------------------------------------------------------

interface MergedFeedProps {
  query: string;
  mcVersion: string | null;
  loader: string | null;
  cfNoticeDismissed: boolean;
  onDismissCfNotice: () => void;
}

function MergedFeed({
  query,
  mcVersion,
  loader,
  cfNoticeDismissed,
  onDismissCfNotice,
}: MergedFeedProps) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  const modrinth = useInfiniteQuery({
    queryKey: ["modpacks", "modrinth", query, mcVersion, loader],
    queryFn: ({ pageParam }) =>
      searchMods("modrinth", query, mcVersion, loader, pageParam, PAGE_LIMIT, "modpack"),
    initialPageParam: 0,
    getNextPageParam: (lastPage) => {
      if (lastPage.hits.length === 0) return undefined;
      const fetched = lastPage.offset + lastPage.hits.length;
      return fetched < lastPage.total ? fetched : undefined;
    },
    staleTime: 30_000,
  });

  const curseforge = useInfiniteQuery({
    queryKey: ["modpacks", "curseforge", query, mcVersion, loader],
    queryFn: ({ pageParam }) =>
      searchMods("curseforge", query, mcVersion, loader, pageParam, PAGE_LIMIT, "modpack"),
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

  const cfOtherError =
    curseforge.isError && !cfKeyMissing;

  // Infinite scroll: when sentinel enters view, advance both providers.
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

  // Collect and merge hits from both loaded buffers, dedupe by `provider:id`, sort downloads desc.
  const modrinthHits: ProjectSummary[] = modrinth.data?.pages.flatMap((p) => p.hits) ?? [];
  const curseforgeHits: ProjectSummary[] = curseforge.isError
    ? []
    : curseforge.data?.pages.flatMap((p) => p.hits) ?? [];

  const seen = new Set<string>();
  const merged: ProjectSummary[] = [];

  for (const hit of [...modrinthHits, ...curseforgeHits]) {
    const key = `${hit.provider}:${hit.id}`;
    if (!seen.has(key)) {
      seen.add(key);
      merged.push(hit);
    }
  }

  merged.sort((a, b) => b.downloads - a.downloads);

  const isInitialLoading = modrinth.isLoading && (cfKeyMissing || curseforge.isLoading);
  const isFetchingMore = modrinth.isFetchingNextPage || curseforge.isFetchingNextPage;

  if (isInitialLoading) {
    return (
      <div className="flex items-center gap-2 py-8 text-sm text-muted">
        <Loader2 className="size-4 animate-spin" />
        Searching…
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
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

      {/* CF non-key error inline notice */}
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

      {/* Modrinth error inline notice */}
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

      {/* Result cards */}
      {merged.map((pack) => (
        <ModpackCard key={`${pack.provider}:${pack.id}`} pack={pack} />
      ))}

      {/* Empty state — only when both providers yielded nothing (and not still loading).
          CF key-missing counts as settled; exclude its loading flag in that case. */}
      {merged.length === 0 && !modrinth.isLoading && (cfKeyMissing || !curseforge.isLoading) && (
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
// Modpack result card — navigation-only, links to the pack info page
// ---------------------------------------------------------------------------

interface ModpackCardProps {
  pack: ProjectSummary;
}

function ModpackCard({ pack }: ModpackCardProps) {
  // Resolve the provider routing string (wire value → routing param).
  const providerRoute: "modrinth" | "curseforge" =
    pack.provider === "modrinth"
      ? "modrinth"
      : pack.provider === "curseForge"
        ? "curseforge"
        : ((_: never) => "curseforge" as const)(pack.provider);

  function handleOpenPage(e: React.MouseEvent) {
    e.preventDefault();
    if (pack.pageUrl == null) return;
    openUrl(pack.pageUrl).catch(console.error);
  }

  return (
    <Link
      to={`/browse/${providerRoute}/${pack.id}`}
      state={{ pack }}
      className="flex w-full items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3 text-sm transition-colors hover:border-primary"
    >
      {/* Icon */}
      {pack.iconUrl ? (
        <img
          src={pack.iconUrl}
          alt=""
          referrerPolicy="no-referrer"
          className="size-10 shrink-0 rounded-lg object-cover"
          loading="lazy"
        />
      ) : (
        <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted">
          <Package className="size-5" />
        </div>
      )}

      {/* Text */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <p className="truncate font-medium text-foreground">{pack.name}</p>
          <ProviderBadge provider={pack.provider} />
        </div>
        <p className="mt-0.5 line-clamp-2 text-xs text-muted">{pack.summary}</p>
      </div>

      {/* Downloads */}
      <div className="flex shrink-0 items-center gap-1 text-muted">
        <Download className="size-3.5" />
        <span>{formatDownloads(pack.downloads)}</span>
      </div>

      {/* Open provider page (secondary action) */}
      {pack.pageUrl != null && (
        <button
          onClick={handleOpenPage}
          title="Open project page"
          className="shrink-0 rounded-lg p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
        >
          <ExternalLink className="size-4" />
        </button>
      )}
    </Link>
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

function formatDownloads(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}
