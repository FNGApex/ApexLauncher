import { useEffect, useRef, useState } from "react";
import { useInfiniteQuery, useMutation, useQuery } from "@tanstack/react-query";
import { AlertCircle, Download, Key, Loader2, Package, Plus, Search, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  addMod,
  getLoaders,
  getModVersions,
  listInstances,
  listMinecraftVersions,
  searchMods,
  type AddModResult,
  type ManualMod,
  type ProjectSummary,
  type ProviderCommandError,
} from "@/lib/ipc";
import { META_STALE_TIME } from "@/lib/query";
import { useNavigate } from "react-router-dom";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PAGE_LIMIT = 20;
const DEBOUNCE_MS = 400;

type ProviderTab = "all" | "modrinth" | "curseforge";

const TABS: { id: ProviderTab; label: string }[] = [
  { id: "all", label: "All" },
  { id: "modrinth", label: "Modrinth" },
  { id: "curseforge", label: "CurseForge" },
];

// ---------------------------------------------------------------------------
// Root component
// ---------------------------------------------------------------------------

export function Browse() {
  const [tab, setTab] = useState<ProviderTab>("all");
  const [rawQuery, setRawQuery] = useState("");
  const [query, setQuery] = useState("");
  const [mcVersion, setMcVersion] = useState<string | null>(null);
  const [loader, setLoader] = useState<string | null>(null);

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

  return (
    <div className="flex h-full flex-col px-8 py-7">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold">Browse</h1>
        <p className="text-sm text-muted">Search mods across providers</p>
      </header>

      {/* Search + tabs bar */}
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2">
          <Search className="size-4 shrink-0 text-muted" />
          <input
            className="w-full bg-transparent text-sm outline-none placeholder:text-muted"
            placeholder="Search mods…"
            value={rawQuery}
            onChange={(e) => setRawQuery(e.target.value)}
          />
        </div>

        <div className="flex gap-1 rounded-lg border border-border bg-surface p-1">
          {TABS.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={
                "rounded-md px-3 py-1.5 text-sm font-medium transition-colors " +
                (tab === t.id
                  ? "bg-surface-2 text-foreground"
                  : "text-muted hover:text-foreground")
              }
            >
              {t.label}
            </button>
          ))}
        </div>
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
        {tab === "all" ? (
          <AllColumns query={query} mcVersion={mcVersion} loader={loader} />
        ) : (
          <ProviderColumn
            provider={tab}
            query={query}
            mcVersion={mcVersion}
            loader={loader}
          />
        )}
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
    queryKey: ["mcVersions"],
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
// "All" tab: two independent columns side by side
// ---------------------------------------------------------------------------

interface AllColumnsProps {
  query: string;
  mcVersion: string | null;
  loader: string | null;
}

function AllColumns({ query, mcVersion, loader }: AllColumnsProps) {
  return (
    <div className="grid grid-cols-2 gap-4 h-full">
      <div className="flex flex-col gap-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted">
          Modrinth
        </span>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ProviderColumn
            provider="modrinth"
            query={query}
            mcVersion={mcVersion}
            loader={loader}
            compact
          />
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted">
          CurseForge
        </span>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ProviderColumn
            provider="curseforge"
            query={query}
            mcVersion={mcVersion}
            loader={loader}
            compact
          />
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Single provider column with infinite scroll
// ---------------------------------------------------------------------------

interface ProviderColumnProps {
  provider: "modrinth" | "curseforge";
  query: string;
  mcVersion: string | null;
  loader: string | null;
  compact?: boolean;
}

function ProviderColumn({ provider, query, mcVersion, loader, compact = false }: ProviderColumnProps) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
    isError,
    error,
  } = useInfiniteQuery({
    queryKey: ["mods", provider, query, mcVersion, loader],
    queryFn: ({ pageParam }) =>
      searchMods(provider, query, mcVersion, loader, pageParam, PAGE_LIMIT),
    initialPageParam: 0,
    getNextPageParam: (lastPage) => {
      if (lastPage.hits.length === 0) return undefined;
      const fetched = lastPage.offset + lastPage.hits.length;
      return fetched < lastPage.total ? fetched : undefined;
    },
    staleTime: 30_000,
  });

  // Infinite scroll: observe the sentinel div; fetch next page when it enters view.
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasNextPage && !isFetchingNextPage) {
          fetchNextPage();
        }
      },
      { threshold: 0.1 },
    );

    observer.observe(el);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  // CF key-missing state — not a crash, not a bare empty list.
  if (isError) {
    const raw: unknown = error;
    if (isProviderCommandError(raw) && raw.kind === "key_missing") {
      return <KeyMissingState />;
    }
    return (
      <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
        <AlertCircle className="size-4 shrink-0" />
        <span>
          {isProviderCommandError(raw) ? raw.message : String(error)}
        </span>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 py-8 text-sm text-muted">
        <Loader2 className="size-4 animate-spin" />
        Searching…
      </div>
    );
  }

  const hits: ProjectSummary[] = data?.pages.flatMap((p) => p.hits) ?? [];

  if (hits.length === 0) {
    return (
      <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-12 text-center text-sm text-muted">
        <Package className="mb-3 size-8" />
        No results
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {hits.map((mod) => (
        <ModCard key={`${mod.provider}:${mod.id}`} mod={mod} compact={compact} />
      ))}

      {/* Scroll sentinel */}
      <div ref={sentinelRef} className="h-4" />

      {isFetchingNextPage && (
        <div className="flex items-center justify-center gap-2 py-2 text-sm text-muted">
          <Loader2 className="size-4 animate-spin" />
          Loading more…
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Mod result card
// ---------------------------------------------------------------------------

interface ModCardProps {
  mod: ProjectSummary;
  compact: boolean;
}

function ModCard({ mod, compact }: ModCardProps) {
  const [showAddModal, setShowAddModal] = useState(false);

  return (
    <>
      <div
        className={
          "flex items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3 transition-colors hover:border-primary " +
          (compact ? "text-xs" : "text-sm")
        }
      >
        {/* Icon */}
        {mod.iconUrl ? (
          <img
            src={mod.iconUrl}
            alt=""
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
          <p className="truncate font-medium text-foreground">{mod.name}</p>
          {!compact && (
            <p className="mt-0.5 line-clamp-2 text-xs text-muted">{mod.summary}</p>
          )}
          {compact && (
            <p className="truncate text-muted">{mod.summary}</p>
          )}
        </div>

        {/* Downloads */}
        <div className="flex shrink-0 items-center gap-1 text-muted">
          <Download className="size-3.5" />
          <span className={compact ? "text-xs" : "text-sm"}>
            {formatDownloads(mod.downloads)}
          </span>
        </div>

        {/* Add to instance button */}
        <button
          onClick={() => setShowAddModal(true)}
          title="Add to instance"
          className="ml-1 shrink-0 rounded-md p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
        >
          <Plus className="size-4" />
        </button>
      </div>

      {showAddModal && (
        <AddToInstanceModal mod={mod} onClose={() => setShowAddModal(false)} />
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Add-to-instance modal
// ---------------------------------------------------------------------------

interface AddToInstanceModalProps {
  mod: ProjectSummary;
  onClose: () => void;
}

function AddToInstanceModal({ mod, onClose }: AddToInstanceModalProps) {
  const [selectedSlug, setSelectedSlug] = useState("");
  const [result, setResult] = useState<AddModResult | null>(null);
  const [addError, setAddError] = useState<string | null>(null);

  const { data: instances } = useQuery({
    queryKey: ["instances"],
    queryFn: listInstances,
  });

  const selectedInstance = instances?.find((i) => i.slug === selectedSlug);

  const versionsQuery = useQuery({
    queryKey: ["modVersions", mod.provider, mod.id, selectedInstance?.minecraft, selectedInstance?.loader.kind],
    queryFn: () =>
      getModVersions(
        mod.provider === "modrinth" ? "modrinth" : "curseforge",
        mod.id,
        selectedInstance!.minecraft,
        selectedInstance!.loader.kind,
      ),
    enabled: selectedInstance != null,
  });

  const primaryVersion = versionsQuery.data?.[0];

  const mutation = useMutation({
    mutationFn: () => {
      if (!selectedInstance || !primaryVersion) {
        throw new Error("Select an instance and ensure a compatible version is available.");
      }
      const provider = mod.provider === "modrinth" ? "modrinth" as const : "curseforge" as const;
      return addMod(
        provider,
        mod.id,
        primaryVersion.id,
        selectedInstance.slug,
        selectedInstance.minecraft,
        selectedInstance.loader.kind,
      );
    },
    onSuccess: (data) => {
      setResult(data);
      setAddError(null);
    },
    onError: (err) => {
      setAddError(String(err));
    },
  });

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-xl border border-border bg-surface p-6 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="mb-5 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Add to instance</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted hover:bg-background hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>

        <p className="mb-4 text-sm font-medium truncate">{mod.name}</p>

        {result ? (
          <AddResultSummary result={result} onClose={onClose} />
        ) : (
          <div className="space-y-4">
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-muted">Target instance</span>
              <select
                value={selectedSlug}
                onChange={(e) => setSelectedSlug(e.target.value)}
                className="input w-full"
              >
                <option value="">Select an instance…</option>
                {instances?.map((i) => (
                  <option key={i.slug} value={i.slug}>
                    {i.name} ({i.minecraft})
                  </option>
                ))}
              </select>
            </label>

            {selectedInstance && (
              <div className="rounded-lg border border-border bg-surface/50 px-3 py-2 text-xs text-muted">
                {versionsQuery.isLoading ? (
                  <span className="flex items-center gap-1.5">
                    <Loader2 className="size-3 animate-spin" />
                    Fetching compatible versions…
                  </span>
                ) : versionsQuery.isError ? (
                  <span className="text-danger">
                    Failed to fetch versions{versionsQuery.error instanceof Error ? `: ${versionsQuery.error.message}` : "."}
                  </span>
                ) : primaryVersion ? (
                  <span>Version: <span className="text-foreground">{primaryVersion.versionNumber}</span></span>
                ) : (
                  <span className="text-danger">No compatible version found for this instance.</span>
                )}
              </div>
            )}

            {addError && (
              <p className="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">
                {addError}
              </p>
            )}

            <div className="flex justify-end gap-2 pt-1">
              <button
                type="button"
                onClick={onClose}
                className="rounded-lg px-4 py-2 text-sm font-medium text-muted hover:bg-background hover:text-foreground"
              >
                Cancel
              </button>
              <button
                onClick={() => mutation.mutate()}
                disabled={!selectedSlug || !primaryVersion || mutation.isPending}
                className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
              >
                {mutation.isPending && <Loader2 className="size-4 animate-spin" />}
                Install
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Install result summary (shown after addMod resolves)
// ---------------------------------------------------------------------------

interface AddResultSummaryProps {
  result: AddModResult;
  onClose: () => void;
}

function AddResultSummary({ result, onClose }: AddResultSummaryProps) {
  return (
    <div className="space-y-3 text-sm">
      {result.added.length > 0 && (
        <p className="text-green-400">
          Added {result.added.length} mod{result.added.length !== 1 ? "s" : ""} successfully.
        </p>
      )}

      {result.manual.length > 0 && (
        <div>
          <p className="mb-1.5 font-medium text-muted">Requires manual download:</p>
          <ul className="space-y-1">
            {result.manual.map((m) => (
              <ManualEntry key={`${m.projectId}:${m.versionId}`} entry={m} />
            ))}
          </ul>
        </div>
      )}

      {result.unresolved.length > 0 && (
        <div>
          <p className="mb-1 font-medium text-muted">Unresolved dependencies:</p>
          <ul className="space-y-0.5 text-xs text-muted">
            {result.unresolved.map((u) => (
              <li key={u.projectId}>{u.projectId} — {u.reason}</li>
            ))}
          </ul>
        </div>
      )}

      {result.failed.length > 0 && (
        <div>
          <p className="mb-1 font-medium text-danger">Failed downloads:</p>
          <ul className="space-y-0.5 text-xs text-danger">
            {result.failed.map((f) => (
              <li key={f.fileName}>{f.fileName}: {f.error}</li>
            ))}
          </ul>
        </div>
      )}

      <div className="flex justify-end pt-1">
        <button
          onClick={onClose}
          className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
        >
          Done
        </button>
      </div>
    </div>
  );
}

function ManualEntry({ entry }: { entry: ManualMod }) {
  return (
    <li className="flex items-center justify-between gap-2 rounded-lg border border-border bg-surface/50 px-3 py-1.5 text-xs">
      <span className="truncate text-muted">{entry.fileName}</span>
      <button
        onClick={() => openUrl(entry.pageUrl).catch(console.error)}
        className="shrink-0 rounded px-2 py-0.5 text-xs font-medium text-primary hover:underline"
      >
        Open page
      </button>
    </li>
  );
}

// ---------------------------------------------------------------------------
// CF key-missing empty state
// ---------------------------------------------------------------------------

function KeyMissingState() {
  const navigate = useNavigate();

  return (
    <div className="flex flex-col items-center gap-3 rounded-xl border border-border bg-surface/40 py-10 text-center">
      <Key className="size-8 text-muted" />
      <p className="text-sm font-medium">API key required</p>
      <p className="max-w-xs text-xs text-muted">
        A CurseForge API key is required to search CurseForge mods. Add your key in
        Settings.
      </p>
      <button
        onClick={() => navigate("/settings")}
        className="mt-1 rounded-lg bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
      >
        Open Settings
      </button>
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

function formatDownloads(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}
