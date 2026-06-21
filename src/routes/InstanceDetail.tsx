import { useEffect, useRef, useState } from "react";
import { useParams, Link, NavLink, Outlet } from "react-router-dom";
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  AlertCircle,
  ArrowLeft,
  Check,
  ChevronDown,
  ChevronUp,
  Download,
  ExternalLink,
  FileBox,
  Key,
  Loader2,
  Package,
  Play,
  RefreshCw,
  Search,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  addMod,
  enrichInstanceMods,
  getInstance,
  getModVersions,
  getSettings,
  launchInstance,
  killInstance,
  refreshPackMeta,
  removeMod,
  searchMods,
  setModEnabled,
  updateMod,
  updateModpack,
  type FolderMod,
  type Instance,
  type ModEntry,
  type ProjectSummary,
  type ProviderCommandError,
} from "@/lib/ipc";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAppStore } from "@/lib/store";
import { ProviderBadge } from "@/components/ProviderBadge";
import { Toggle } from "@/components/Toggle";

const PAGE_LIMIT = 20;
const DEBOUNCE_MS = 400;

// Tracks instances whose mod metadata backfill has been triggered this session, so
// the lazy enrich (for modpack-imported mods without metadata) runs at most once per
// instance. The backend enrich is also idempotent (0 network calls when nothing is
// missing); this just avoids redundant in-flight calls before the refetch lands.
const enrichedSlugs = new Set<string>();

// Tracks managed instances for which refreshPackMeta has been called this session.
// The backend throttles to once/24h; this guard prevents redundant calls within a
// single session before a refetch has settled.
const refreshedSlugs = new Set<string>();

// ---------------------------------------------------------------------------
// Outlet context type — consumed by the four tab components
// ---------------------------------------------------------------------------

export type InstanceTabContext = {
  slug: string;
  instance: Instance;
  folderMods: FolderMod[];
  invalidate: () => void;
};

// ---------------------------------------------------------------------------
// Root shell component
// ---------------------------------------------------------------------------

export function InstanceDetail() {
  const { slug } = useParams();
  const qc = useQueryClient();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["instance", slug],
    queryFn: () => getInstance(slug!),
    enabled: !!slug,
  });

  // Settings query — used for T5 (showConsoleDefault) and T6 (keepLauncherOpen).
  const { data: appSettings } = useQuery({ queryKey: ["settings"], queryFn: getSettings });

  // Run state + logs come from the app-level store (populated by AppShell listeners).
  // Reading from here survives navigation: navigate away and back shows live status
  // and replayed logs including any exit that fired while the user was on another page.
  const runState = useAppStore((s) => (slug ? s.runs.get(slug) : undefined));
  const runLogLines = useAppStore((s) => (slug ? s.runLogs.get(slug) : undefined));

  // An instance is active while status is "preparing" or "running".
  const running = runState?.status === "preparing" || runState?.status === "running";

  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const consoleRef = useRef<HTMLPreElement>(null);

  // T5 show_console_default: console collapse state seeded from the setting.
  // `null` means "not yet seeded" — we wait for the settings query to resolve.
  const [consoleExpanded, setConsoleExpanded] = useState<boolean | null>(null);

  // PB-F2: update-available banner state (populated by refreshPackMeta).
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);

  // PB-F3: version/update modal open state.
  const [versionModalOpen, setVersionModalOpen] = useState(false);

  // Seed console expanded state once settings load (only once — null guard).
  useEffect(() => {
    if (appSettings !== undefined && consoleExpanded === null) {
      setConsoleExpanded(appSettings.showConsoleDefault ?? false);
    }
  }, [appSettings, consoleExpanded]);

  // Auto-scroll console to bottom when new log lines arrive.
  useEffect(() => {
    const el = consoleRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [runLogLines]);

  // PB-F2: once per session per managed instance, call refreshPackMeta.
  // The backend is throttled to 24h (returns cached with zero network when within 24h).
  // If checked=true (i.e. it actually polled), invalidate so stored icon/author/latest load.
  useEffect(() => {
    if (!slug || !data?.instance.source) return;
    if (refreshedSlugs.has(slug)) return;
    refreshedSlugs.add(slug);
    refreshPackMeta(slug)
      .then((result) => {
        if (result.updateAvailable) {
          setUpdateAvailable(true);
          setLatestVersion(result.latestVersion ?? null);
        }
        if (result.checked) {
          qc.invalidateQueries({ queryKey: ["instance", slug] });
        }
      })
      .catch((err) => {
        // Non-fatal — don't surface to the user, just log.
        console.warn("refreshPackMeta failed:", err);
        // Allow a retry on next mount if this was transient.
        refreshedSlugs.delete(slug);
      });
  }, [slug, data?.instance.source, qc]);

  async function handleLaunch() {
    if (!slug) return;
    setLaunchError(null);

    // Warn when another pack is already running (2nd-launch warning).
    const runs = useAppStore.getState().runs;
    const activeRuns = [...runs.values()].filter(
      (r) => r.status === "preparing" || r.status === "running",
    );
    if (activeRuns.length >= 1) {
      const confirmed = window.confirm(
        `${activeRuns.length} instance${activeRuns.length !== 1 ? "s are" : " is"} already running. Launch another?`,
      );
      if (!confirmed) return;
    }

    setLaunching(true);
    try {
      await launchInstance(slug);
      // T6 keep_launcher_open: minimize the window after a successful launch
      // when the setting is false. Default true = do nothing. Isolated try/catch
      // so a window-API error here never surfaces as a (false) launch failure.
      const keepOpen = appSettings?.keepLauncherOpen ?? true;
      if (!keepOpen) {
        try {
          await getCurrentWindow().minimize();
        } catch (e) {
          console.warn("minimize after launch failed:", e);
        }
      }
    } catch (err) {
      setLaunchError(String(err));
    } finally {
      setLaunching(false);
    }
  }

  async function handleStop() {
    if (!slug) return;
    try {
      await killInstance(slug);
    } catch (err) {
      setLaunchError(String(err));
    }
  }

  function invalidate() {
    qc.invalidateQueries({ queryKey: ["instance", slug] });
  }

  const tabBase =
    "rounded-md px-4 py-1.5 text-sm font-medium transition-colors text-muted hover:text-foreground";
  const tabActive = "bg-surface-2 text-foreground";

  return (
    <div className="flex flex-col px-8 py-7">
      <Link
        to="/instances"
        className="mb-4 inline-flex items-center gap-1.5 text-sm text-muted hover:text-foreground"
      >
        <ArrowLeft className="size-4" />
        Back to instances
      </Link>

      {isLoading ? (
        <p className="text-sm text-muted">Loading…</p>
      ) : isError ? (
        <p className="text-sm text-danger">{String(error)}</p>
      ) : data ? (
        <>
          {/* ----------------------------------------------------------------
              Persistent Bar — always-visible header across all tab switches
              PB-F1: icon + name + author + clickable version chip + source link
              PB-F2: updatable banner when refreshPackMeta reports update_available
          ---------------------------------------------------------------- */}
          <header className="mb-4 rounded-2xl border border-border bg-surface px-7 py-6">
            {/* Top row: icon + identity + actions */}
            <div className="flex items-center gap-6">
              {/* Pack icon (PB-F1) — grandiose: size-24 */}
              {data.instance.source?.iconUrl ? (
                <img
                  src={data.instance.source.iconUrl}
                  alt=""
                  referrerPolicy="no-referrer"
                  className="size-24 shrink-0 rounded-2xl object-cover shadow-lg"
                  loading="lazy"
                />
              ) : (
                <div className="grid size-24 shrink-0 place-items-center rounded-2xl bg-surface-2 text-muted shadow-inner">
                  <Package className="size-12" />
                </div>
              )}

              {/* Name + metadata */}
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-3">
                  <h1 className="text-3xl font-bold leading-tight tracking-tight">{data.instance.name}</h1>
                  {/* Clickable version chip (PB-F1, PB-F3) — only for managed instances */}
                  {data.instance.source && (
                    <button
                      onClick={() => setVersionModalOpen(true)}
                      title="View versions / update"
                      className="inline-flex items-center gap-1 rounded-full border border-border bg-surface-2 px-3 py-1 text-sm text-muted font-medium transition-colors hover:border-primary/50 hover:text-foreground"
                    >
                      v{data.instance.source.packVersion}
                    </button>
                  )}
                </div>

                {/* Author (PB-F1) — only when present */}
                {data.instance.source?.author && (
                  <p className="mt-1 text-base text-muted">by {data.instance.source.author}</p>
                )}

                {/* Loader / MC subtitle */}
                <p className="mt-1.5 text-sm text-muted/70">
                  {labelLoader(data.instance.loader.kind)}
                  {data.instance.loader.version ? ` ${data.instance.loader.version}` : ""} ·
                  Minecraft {data.instance.minecraft}
                </p>
              </div>

              {/* Right-side controls */}
              <div className="flex shrink-0 flex-col items-end gap-3">
                <div className="flex items-center gap-2">
                  {/* Pack source link (PB-F1) */}
                  {data.instance.source && (() => {
                    const src = data.instance.source;
                    const providerRoute: "modrinth" | "curseforge" =
                      src.provider === "modrinth"
                        ? "modrinth"
                        : "curseforge";
                    const pageUrl: string | null =
                      src.pageUrl ?? (providerRoute === "modrinth"
                        ? `https://modrinth.com/modpack/${src.projectId}`
                        : null);
                    return pageUrl ? (
                      <button
                        onClick={() => openUrl(pageUrl).catch(console.error)}
                        title="Open pack page in browser"
                        className="inline-flex items-center gap-1.5 rounded-lg border border-border bg-surface-2 px-3 py-2 text-sm font-medium text-muted transition-colors hover:text-foreground"
                      >
                        <ExternalLink className="size-4" />
                        Pack source
                      </button>
                    ) : null;
                  })()}

                  {/* Running badge */}
                  {running && (
                    <span className="inline-flex items-center gap-1.5 rounded-full bg-green-500/15 px-3 py-1.5 text-sm font-medium text-green-400">
                      <span className="size-2 animate-pulse rounded-full bg-green-400" />
                      Running
                    </span>
                  )}
                </div>

                {/* Launch / Stop — prominent */}
                {running ? (
                  <button
                    onClick={handleStop}
                    className="inline-flex items-center gap-2 rounded-xl border border-border bg-surface px-5 py-2.5 text-base font-medium text-danger hover:bg-surface-2"
                  >
                    <Square className="size-5" />
                    Stop
                  </button>
                ) : (
                  <button
                    onClick={handleLaunch}
                    disabled={launching}
                    className="inline-flex items-center gap-2 rounded-xl bg-accent px-5 py-2.5 text-base font-semibold text-white shadow-md hover:bg-accent/80 disabled:opacity-50"
                  >
                    <Play className="size-5" />
                    {launching ? "Launching…" : "Launch"}
                  </button>
                )}
              </div>
            </div>

            {/* PB-F2: Updatable banner */}
            {updateAvailable && (
              <div className="mt-4 flex items-center justify-between gap-3 rounded-xl border border-primary/30 bg-primary/10 px-4 py-2.5 text-sm">
                <span className="text-primary font-medium">
                  Update available{latestVersion ? `: v${latestVersion}` : ""}
                </span>
                <button
                  onClick={() => setVersionModalOpen(true)}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-primary/40 bg-primary/20 px-3 py-1.5 text-xs font-medium text-primary transition-colors hover:bg-primary/30"
                >
                  <RefreshCw className="size-3.5" />
                  Update
                </button>
              </div>
            )}
          </header>

          {/* PB-F3: Version / update modal */}
          {versionModalOpen && data.instance.source && (
            <VersionUpdateModal
              slug={slug!}
              source={data.instance.source}
              packLocked={data.instance.packLocked ?? false}
              onClose={() => setVersionModalOpen(false)}
              onUpdated={() => {
                setVersionModalOpen(false);
                setUpdateAvailable(false);
                qc.invalidateQueries({ queryKey: ["instance", slug] });
                qc.invalidateQueries({ queryKey: ["instances"] });
              }}
            />
          )}

          {launchError && (
            <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-4 py-2 text-sm text-danger">
              {launchError}
            </p>
          )}

          {/* ----------------------------------------------------------------
              Tab bar
          ---------------------------------------------------------------- */}
          <nav className="mb-6 flex gap-1 rounded-lg border border-border bg-surface p-1 self-start">
            <NavLink
              to="info"
              className={({ isActive }) =>
                tabBase + (isActive ? " " + tabActive : "")
              }
            >
              Info
            </NavLink>
            <NavLink
              to="modlist"
              className={({ isActive }) =>
                tabBase + (isActive ? " " + tabActive : "")
              }
            >
              Mods
            </NavLink>
            <NavLink
              to="tech"
              className={({ isActive }) =>
                tabBase + (isActive ? " " + tabActive : "")
              }
            >
              Pack settings
            </NavLink>
          </nav>

          {/* ----------------------------------------------------------------
              Tab outlet — receives the instance context
          ---------------------------------------------------------------- */}
          <div className="mb-8">
            <Outlet
              context={
                {
                  slug: slug!,
                  instance: data.instance,
                  folderMods: data.folderMods,
                  invalidate,
                } satisfies InstanceTabContext
              }
            />
          </div>

          {/* ----------------------------------------------------------------
              Console — persistent chrome, shown when the instance is running
              or has accumulated log lines. T5: collapsible, seeded from
              showConsoleDefault setting.
          ---------------------------------------------------------------- */}
          {(running || (runLogLines && runLogLines.length > 0)) && (
            <section className="mb-8">
              <button
                type="button"
                onClick={() => setConsoleExpanded((prev) => !(prev ?? false))}
                className="mb-3 flex items-center gap-2 text-sm font-semibold text-muted hover:text-foreground"
              >
                Console
                {consoleExpanded ?? false ? (
                  <ChevronUp className="size-3.5" />
                ) : (
                  <ChevronDown className="size-3.5" />
                )}
              </button>
              {(consoleExpanded ?? false) && (
                <pre
                  ref={consoleRef}
                  className="h-64 overflow-y-auto rounded-lg border border-border bg-surface p-3 font-mono text-xs text-foreground"
                >
                  {!runLogLines || runLogLines.length === 0
                    ? "Waiting for output…"
                    : runLogLines.map((l) => `[${l.stream}] ${l.line}`).join("\n")}
                </pre>
              )}
            </section>
          )}
        </>
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// PB-F3: Version / update modal
// Centered modal (reuses the BrowsePackInfo DownloadPrompt pattern):
//   - lazy getModVersions (fetched only when open)
//   - current version marked
//   - Update button → updateModpack → close + invalidate
//   - pack_locked respected (Update disabled with a note)
// ---------------------------------------------------------------------------

interface VersionUpdateModalProps {
  slug: string;
  source: {
    provider: string;
    projectId: string;
    fileId: string;
    packVersion: string;
  };
  packLocked: boolean;
  onClose: () => void;
  onUpdated: () => void;
}

function VersionUpdateModal({
  slug,
  source,
  packLocked,
  onClose,
  onUpdated,
}: VersionUpdateModalProps) {
  const [selectedVersionId, setSelectedVersionId] = useState<string>(source.fileId);
  const [updating, setUpdating] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);

  // Resolve the provider routing string (InstanceSource stores the wire value).
  const providerRoute: "modrinth" | "curseforge" =
    source.provider === "modrinth"
      ? "modrinth"
      : "curseforge";

  // Lazy: versions fetched only when modal is open (it's mounted only when open).
  const versionsQuery = useQuery({
    queryKey: ["packVersions", source.provider, source.projectId],
    queryFn: () => getModVersions(providerRoute, source.projectId, null, null),
    staleTime: 30_000,
  });

  async function handleUpdate() {
    if (packLocked) return;
    setUpdating(true);
    setUpdateError(null);
    try {
      await updateModpack(slug, selectedVersionId === source.fileId ? undefined : selectedVersionId);
      onUpdated();
    } catch (err) {
      setUpdateError(err instanceof Error ? err.message : String(err));
    } finally {
      setUpdating(false);
    }
  }

  return (
    /* Backdrop */
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      {/* Modal card */}
      <div className="w-full max-w-sm rounded-2xl border border-border bg-surface p-6 shadow-xl">
        {/* Header */}
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold">Pack versions</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted hover:bg-surface-2 hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* Version list */}
        <label className="mb-1 block text-xs text-muted">Version</label>
        <div className="relative mb-4">
          {versionsQuery.isLoading && (
            <div className="flex items-center gap-2 py-2 text-xs text-muted">
              <Loader2 className="size-3.5 animate-spin" />
              Loading versions…
            </div>
          )}
          {versionsQuery.isError && (
            <p className="text-xs text-danger">{String(versionsQuery.error)}</p>
          )}
          {versionsQuery.data && (
            <select
              value={selectedVersionId}
              onChange={(e) => setSelectedVersionId(e.target.value)}
              disabled={updating || packLocked}
              title={packLocked ? "Pack is managed — turn off managing in Tech Info to update" : undefined}
              className="input w-full disabled:opacity-50"
            >
              {versionsQuery.data.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.name || v.versionNumber}
                  {v.id === source.fileId ? " (current)" : ""}
                </option>
              ))}
            </select>
          )}
        </div>

        {/* Error */}
        {updateError != null && (
          <div className="mb-3 flex items-start gap-2 rounded-xl border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
            <AlertCircle className="mt-0.5 size-3.5 shrink-0" />
            <span>{updateError}</span>
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-2">
          <button
            onClick={onClose}
            className="flex-1 rounded-lg border border-border px-3 py-2 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
          >
            Cancel
          </button>
          <button
            onClick={handleUpdate}
            disabled={updating || packLocked || !versionsQuery.data}
            title={packLocked ? "Pack is managed — turn off managing in Tech Info to update" : undefined}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:cursor-default disabled:opacity-60"
          >
            {updating ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <RefreshCw className="size-4" />
            )}
            {updating ? "Updating…" : "Update"}
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Manage installs panel (full-page tab body for ModlistTab)
// Exported so ModlistTab can import it.
// ---------------------------------------------------------------------------

type ManageTab = "installed" | "add";

interface ManageInstallsPanelProps {
  instanceSlug: string;
  minecraft: string;
  loaderKind: string;
  folderMods: FolderMod[];
  modEntries: ModEntry[];
  packLocked: boolean;
  onMutate: () => void;
}

export function ManageInstallsPanel({
  instanceSlug,
  minecraft,
  loaderKind,
  folderMods,
  modEntries,
  packLocked,
  onMutate,
}: ManageInstallsPanelProps) {
  const [tab, setTab] = useState<ManageTab>("installed");

  return (
    <div className="flex flex-col gap-4">
      {/* Header: tab switcher */}
      <div className="flex items-center gap-4">
        {/* Tab switcher */}
        <div className="flex gap-1 rounded-lg border border-border bg-surface p-1 self-start">
          {(["installed", "add"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={
                "rounded-md px-4 py-1.5 text-sm font-medium transition-colors " +
                (tab === t
                  ? "bg-surface-2 text-foreground"
                  : "text-muted hover:text-foreground")
              }
            >
              {t === "installed" ? "Installed" : "Add Mods"}
            </button>
          ))}
        </div>
      </div>

      {tab === "installed" ? (
        <InstalledModsTab
          instanceSlug={instanceSlug}
          folderMods={folderMods}
          modEntries={modEntries}
          packLocked={packLocked}
          onMutate={onMutate}
        />
      ) : (
        <AddModTab
          instanceSlug={instanceSlug}
          minecraft={minecraft}
          loaderKind={loaderKind}
          modEntries={modEntries}
          packLocked={packLocked}
          onMutate={onMutate}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tab 1: Installed mods
// ---------------------------------------------------------------------------

interface InstalledModsTabProps {
  instanceSlug: string;
  folderMods: FolderMod[];
  modEntries: ModEntry[];
  packLocked: boolean;
  onMutate: () => void;
}

function InstalledModsTab({
  instanceSlug,
  folderMods,
  modEntries,
  packLocked,
  onMutate,
}: InstalledModsTabProps) {
  // Lazy backfill: modpack-imported mods carry no name/icon/summary. On first view
  // of an instance with metadata-less mods, fetch it in one batched provider call
  // (per provider), persist to the manifest, then refetch so icons/names appear.
  // Runs at most once per instance per session; backend is idempotent.
  useEffect(() => {
    if (!instanceSlug || enrichedSlugs.has(instanceSlug)) return;
    if (!modEntries.some((e) => e.name == null)) return;
    enrichedSlugs.add(instanceSlug);
    enrichInstanceMods(instanceSlug)
      .then((n) => {
        if (n > 0) onMutate();
      })
      .catch((err) => {
        // Allow a retry on a later view if this was transient.
        enrichedSlugs.delete(instanceSlug);
        console.error("enrich mod metadata failed:", err);
      });
  }, [instanceSlug, modEntries, onMutate]);

  if (folderMods.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-border bg-surface/40 px-4 py-8 text-center text-sm text-muted">
        No mods installed. Switch to "Add mod" to browse and install.
      </div>
    );
  }

  return (
    <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border">
      {folderMods.map((m) => {
        const entry = modEntries.find((e) => e.fileName === m.fileName);
        return (
          <ModRow
            key={m.fileName}
            mod={m}
            entry={entry}
            instanceSlug={instanceSlug}
            packLocked={packLocked}
            onMutate={onMutate}
          />
        );
      })}
    </ul>
  );
}

// ---------------------------------------------------------------------------
// Tab 2: Add a mod (search + install)
// ---------------------------------------------------------------------------

type ModProvider = "modrinth" | "curseforge";

interface AddModTabProps {
  instanceSlug: string;
  minecraft: string;
  loaderKind: string;
  modEntries: ModEntry[];
  packLocked: boolean;
  onMutate: () => void;
}

function AddModTab({ instanceSlug, minecraft, loaderKind, modEntries, packLocked, onMutate }: AddModTabProps) {
  const [provider, setProvider] = useState<ModProvider>("modrinth");
  const [rawQuery, setRawQuery] = useState("");
  const [query, setQuery] = useState("");
  const [cfNoticeDismissed, setCfNoticeDismissed] = useState(false);
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  // Debounce
  useEffect(() => {
    const id = setTimeout(() => setQuery(rawQuery.trim()), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [rawQuery]);

  // Reset CF notice when provider or query changes.
  useEffect(() => {
    setCfNoticeDismissed(false);
  }, [provider, query]);

  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
    isError,
    error,
    refetch,
  } = useInfiniteQuery({
    queryKey: ["modSearch", instanceSlug, provider, query, minecraft, loaderKind],
    queryFn: ({ pageParam }) =>
      searchMods(provider, query, minecraft, loaderKind, pageParam, PAGE_LIMIT, "mod"),
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
        if (entries[0].isIntersecting && hasNextPage && !isFetchingNextPage) {
          fetchNextPage();
        }
      },
      { threshold: 0.1 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const cfKeyMissing =
    provider === "curseforge" &&
    isError &&
    isProviderCommandError(error) &&
    error.kind === "key_missing";

  const hits: ProjectSummary[] = data?.pages.flatMap((p) => p.hits) ?? [];

  return (
    <div className="flex flex-col gap-4">
      {/* Source toggle */}
      <div className="flex items-center gap-3">
        <span className="text-xs font-medium text-muted">Source:</span>
        <div className="flex gap-1 rounded-lg border border-border bg-surface p-1">
          {(["modrinth", "curseforge"] as const).map((p) => (
            <button
              key={p}
              onClick={() => setProvider(p)}
              className={
                "rounded-md px-3 py-1 text-xs font-medium transition-colors " +
                (provider === p
                  ? "bg-surface-2 text-foreground"
                  : "text-muted hover:text-foreground")
              }
            >
              {p === "modrinth" ? "Modrinth" : "CurseForge"}
            </button>
          ))}
        </div>
      </div>

      {/* Search bar */}
      <div className="flex items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2">
        <Search className="size-4 shrink-0 text-muted" />
        <input
          className="w-full bg-transparent text-sm outline-none placeholder:text-muted"
          placeholder="Search mods…"
          value={rawQuery}
          onChange={(e) => setRawQuery(e.target.value)}
        />
        {rawQuery && (
          <button
            onClick={() => setRawQuery("")}
            className="text-muted hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        )}
      </div>

      {/* CF key-missing notice */}
      {cfKeyMissing && !cfNoticeDismissed && (
        <div className="flex items-center justify-between gap-3 rounded-xl border border-border bg-surface/60 px-4 py-3 text-sm">
          <div className="flex items-center gap-2 text-muted">
            <Key className="size-4 shrink-0" />
            <span>CurseForge API key required. Add it in Settings.</span>
          </div>
          <button
            onClick={() => setCfNoticeDismissed(true)}
            className="shrink-0 rounded-md p-0.5 text-muted hover:bg-surface-2 hover:text-foreground"
            title="Dismiss"
          >
            <X className="size-4" />
          </button>
        </div>
      )}

      {/* General error (non-key-missing) */}
      {isError && !cfKeyMissing && (
        <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          <AlertCircle className="size-4 shrink-0" />
          <span>
            {isProviderCommandError(error) ? error.message : String(error)}
          </span>
          <button
            onClick={() => refetch()}
            className="ml-auto shrink-0 text-xs underline hover:no-underline"
          >
            Retry
          </button>
        </div>
      )}

      {/* Loading state */}
      {isLoading && !cfKeyMissing && (
        <div className="flex items-center gap-2 py-4 text-sm text-muted">
          <Loader2 className="size-4 animate-spin" />
          Searching…
        </div>
      )}

      {/* Result list */}
      {!isLoading && !cfKeyMissing && (
        <div className="flex flex-col gap-2">
          {hits.length === 0 && !isError && (
            <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-10 text-sm text-muted">
              <Package className="mb-2 size-7" />
              No mods found
            </div>
          )}

          {hits.map((mod) => (
            <ModSearchCard
              key={`${mod.provider}:${mod.id}`}
              mod={mod}
              instanceSlug={instanceSlug}
              minecraft={minecraft}
              loaderKind={loaderKind}
              modEntries={modEntries}
              packLocked={packLocked}
              onInstalled={onMutate}
            />
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
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Search result card for a single mod
// ---------------------------------------------------------------------------

interface ModSearchCardProps {
  mod: ProjectSummary;
  instanceSlug: string;
  minecraft: string;
  loaderKind: string;
  modEntries: ModEntry[];
  packLocked: boolean;
  onInstalled: () => void;
}

function ModSearchCard({
  mod,
  instanceSlug,
  minecraft,
  loaderKind,
  modEntries,
  packLocked,
  onInstalled,
}: ModSearchCardProps) {
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);
  const [addTaskId, setAddTaskId] = useState<number | null>(null);
  // Hover state for "Added" → "Remove" swap on installed mods.
  const [hoverRemove, setHoverRemove] = useState(false);
  const qc = useQueryClient();

  // Resolve the provider routing string from the ProviderKind response value.
  // ProviderKind serializes as "modrinth" | "curseForge" (camelCase from Rust).
  const providerRoute: "modrinth" | "curseforge" =
    mod.provider === "modrinth"
      ? "modrinth"
      : mod.provider === "curseForge"
        ? "curseforge"
        : ((_: never) => "curseforge" as const)(mod.provider);

  // Check whether this mod is already in the installed list.
  const installed = modEntries.find((e) => e.projectId === mod.id);

  // Track the enqueued task so the button stays busy until the task is terminal.
  // AppShell populates the store from task://update — no new subscription needed.
  const addTask = useAppStore((s) =>
    addTaskId != null ? s.tasks.get(addTaskId) : undefined,
  );

  useEffect(() => {
    const kind = addTask?.status.kind;
    if (kind === "done" || kind === "failed" || kind === "cancelled") {
      setAdding(false);
      setAddTaskId(null);
      if (kind === "done") {
        onInstalled();
      }
      // On failed/cancelled: the global toast (D-F1) already reports the failure.
    }
  }, [addTask?.status.kind, onInstalled]);

  // AM-F1: versions are fetched ONLY on Add click — zero calls on render.
  async function handleAdd() {
    setAdding(true);
    setAddError(null);
    try {
      const versions = await getModVersions(providerRoute, mod.id, minecraft, loaderKind);
      if (!versions || versions.length === 0) {
        setAddError("No compatible version found for this Minecraft version / loader.");
        setAdding(false);
        return;
      }
      // addMod returns a task id; keep adding=true until the task is terminal.
      const taskId = await addMod(
        providerRoute,
        mod.id,
        versions[0].id,
        instanceSlug,
        minecraft,
        loaderKind,
        { name: mod.name, iconUrl: mod.iconUrl, summary: mod.summary },
      );
      setAddTaskId(taskId);
      // Do NOT call onInstalled() here — wait for the task to reach "done".
    } catch (err) {
      // Synchronous reject (e.g. enqueue failure) — stop immediately.
      setAddError(String(err));
      setAdding(false);
    }
  }

  async function handleRemove() {
    if (!installed || packLocked) return;
    try {
      await removeMod(instanceSlug, installed.fileName);
      qc.invalidateQueries({ queryKey: ["instance", instanceSlug] });
      onInstalled();
    } catch (err) {
      setAddError(String(err));
    }
  }

  return (
    <div className="flex flex-col gap-2 rounded-xl border border-border bg-surface px-4 py-3 text-sm">
      <div className="flex items-center gap-3">
        {/* Icon */}
        {mod.iconUrl ? (
          <img
            src={mod.iconUrl}
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
          <div className="flex flex-wrap items-center gap-1.5">
            <p className="font-medium text-foreground">{mod.name}</p>
            <ProviderBadge provider={mod.provider} />
          </div>
          <p className="mt-0.5 line-clamp-2 text-xs text-muted">{mod.summary}</p>
        </div>

        {/* Downloads */}
        <div className="flex shrink-0 items-center gap-1 text-xs text-muted">
          <Download className="size-3.5" />
          {formatDownloads(mod.downloads)}
        </div>

        {/* Action button: Add / spinner / Added (hover → Remove) */}
        {installed ? (
          <button
            onClick={handleRemove}
            disabled={packLocked}
            onMouseEnter={() => setHoverRemove(true)}
            onMouseLeave={() => setHoverRemove(false)}
            title={packLocked ? "Pack is managed — turn off managing in Tech Info to remove mods" : "Remove mod"}
            className={
              "inline-flex shrink-0 items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors disabled:opacity-50 " +
              (hoverRemove && !packLocked
                ? "border-danger/40 bg-danger/10 text-danger hover:bg-danger/20"
                : "border-green-500/40 bg-green-500/10 text-green-400")
            }
          >
            {hoverRemove && !packLocked ? (
              <>
                <X className="size-3.5" />
                Remove
              </>
            ) : (
              <>
                <Check className="size-3.5" />
                Added
              </>
            )}
          </button>
        ) : (
          <button
            onClick={handleAdd}
            disabled={adding || packLocked}
            title={packLocked ? "Pack is managed — turn off managing in Tech Info to add mods" : "Add mod"}
            className="inline-flex shrink-0 items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent/80 disabled:opacity-50"
          >
            {adding ? (
              <>
                <Loader2 className="size-3.5 animate-spin" />
                Adding…
              </>
            ) : (
              "Add"
            )}
          </button>
        )}
      </div>

      {/* Add / remove error */}
      {addError && (
        <p className="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">
          {addError}
        </p>
      )}

      {/* Add enqueued — result arrives via task://update (CP-9 toast) */}
    </div>
  );
}


// ---------------------------------------------------------------------------
// Installed mod row (relocated from inline page, behavior unchanged)
// ---------------------------------------------------------------------------

interface ModRowProps {
  mod: FolderMod;
  entry: ModEntry | undefined;
  instanceSlug: string;
  packLocked: boolean;
  onMutate: () => void;
}

function ModRow({ mod, entry, instanceSlug, packLocked, onMutate }: ModRowProps) {
  const [updateTaskId, setUpdateTaskId] = useState<number | null>(null);
  const [isUpdating, setIsUpdating] = useState(false);

  const toggleMutation = useMutation({
    mutationFn: () => setModEnabled(instanceSlug, mod.fileName, mod.disabled),
    onSuccess: onMutate,
  });

  const removeMutation = useMutation({
    mutationFn: () => removeMod(instanceSlug, mod.fileName),
    onSuccess: onMutate,
  });

  // updateMod returns a task id; keep the spinner until the task is terminal.
  const updateMutation = useMutation({
    mutationFn: () => {
      if (!entry) return Promise.reject(new Error("no mod entry"));
      return updateMod(instanceSlug, entry.projectId);
    },
    onSuccess: (taskId) => {
      setUpdateTaskId(taskId);
      setIsUpdating(true);
    },
    onError: () => {
      setIsUpdating(false);
    },
  });

  // Track the enqueued update task until it reaches a terminal state.
  const updateTask = useAppStore((s) =>
    updateTaskId != null ? s.tasks.get(updateTaskId) : undefined,
  );

  useEffect(() => {
    const kind = updateTask?.status.kind;
    if (kind === "done" || kind === "failed" || kind === "cancelled") {
      setIsUpdating(false);
      setUpdateTaskId(null);
      if (kind === "done") {
        onMutate();
      }
      // On failed/cancelled: the global toast (D-F1) already reports the failure.
    }
  }, [updateTask?.status.kind, onMutate]);

  const isBusy =
    toggleMutation.isPending || removeMutation.isPending || updateMutation.isPending || isUpdating;
  const isDisabled = isBusy || packLocked;

  // Display name: stored metadata name, else filename.
  const displayName = entry?.name ?? mod.fileName;

  return (
    <li className="flex flex-col gap-1.5 bg-surface px-4 py-4 text-sm">
      <div className="flex items-center gap-3">
        {/* Icon: stored iconUrl or fallback */}
        {entry?.iconUrl ? (
          <img
            src={entry.iconUrl}
            alt=""
            referrerPolicy="no-referrer"
            className={`size-16 shrink-0 rounded-lg object-cover ${mod.disabled ? "opacity-50" : ""}`}
            loading="lazy"
          />
        ) : (
          <div className="grid size-16 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted">
            <FileBox className="size-8" />
          </div>
        )}

        {/* Text block */}
        <div className="min-w-0 flex-1">
          {/* Title row */}
          <div className="flex flex-wrap items-center gap-2">
            <span
              className={`font-semibold ${mod.disabled ? "text-muted line-through" : "text-foreground"}`}
            >
              {displayName}
            </span>
            {!mod.managed && (
              <span className="rounded bg-surface-2 px-1.5 py-0.5 text-xs text-muted">
                unmanaged
              </span>
            )}
          </div>

          {/* Subtitle: jar filename + size */}
          <p className="mt-0.5 text-xs text-muted">
            {mod.fileName} · {formatBytes(mod.sizeBytes)}
          </p>

          {/* Description: summary when available */}
          {entry?.summary && (
            <p className="mt-1 line-clamp-2 text-xs text-muted">{entry.summary}</p>
          )}
        </div>

        {/* Actions — only for managed mods that have a mod entry */}
        {entry && (
          <div className="flex shrink-0 items-center gap-1 pt-0.5">
            {/* Enable / disable — Apple-style toggle (LF-4) */}
            <span
              title={packLocked ? "Pack is managed — turn off managing in Tech Info to change mods" : mod.disabled ? "Enable mod" : "Disable mod"}
              className="flex items-center px-1"
            >
              <Toggle
                checked={!mod.disabled}
                onChange={() => toggleMutation.mutate()}
                disabled={isDisabled}
                label={mod.disabled ? "Enable mod" : "Disable mod"}
              />
            </span>

            {/* Update */}
            <button
              onClick={() => updateMutation.mutate()}
              disabled={isDisabled}
              title={packLocked ? "Pack is managed — turn off managing in Tech Info to change mods" : "Check for update"}
              className="rounded p-1 text-muted transition-colors hover:bg-surface-2 hover:text-foreground disabled:opacity-50"
            >
              {updateMutation.isPending || isUpdating ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <RefreshCw className="size-3.5" />
              )}
            </button>

            {/* Remove */}
            <button
              onClick={() => removeMutation.mutate()}
              disabled={isDisabled}
              title={packLocked ? "Pack is managed — turn off managing in Tech Info to change mods" : "Remove mod"}
              className="rounded p-1 text-muted transition-colors hover:bg-danger/10 hover:text-danger disabled:opacity-50"
            >
              {removeMutation.isPending ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Trash2 className="size-3.5" />
              )}
            </button>
          </div>
        )}
      </div>

      {/* Mutation errors */}
      {(toggleMutation.isError || removeMutation.isError || updateMutation.isError) && (
        <p className="text-xs text-danger">
          {String(
            toggleMutation.error ?? removeMutation.error ?? updateMutation.error,
          )}
        </p>
      )}
    </li>
  );
}


// ---------------------------------------------------------------------------
// Shared helpers — exported so tab files can import them
// ---------------------------------------------------------------------------

export function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-surface px-4 py-3">
      <dt className="text-xs text-muted">{label}</dt>
      <dd className="mt-0.5 font-medium">{value}</dd>
    </div>
  );
}

export function labelLoader(kind: string): string {
  return kind === "vanilla" ? "Vanilla" : kind[0].toUpperCase() + kind.slice(1);
}

export function formatDate(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? "—" : d.toLocaleDateString();
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDownloads(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function isProviderCommandError(v: unknown): v is ProviderCommandError {
  if (typeof v !== "object" || v === null) return false;
  if (!("kind" in v) || !("message" in v)) return false;
  // After the `in` checks, TypeScript narrows v to have `kind` and `message` properties.
  // We index into the narrowed type directly — no cast needed.
  return typeof v.kind === "string" && typeof v.message === "string";
}

/** Convert total seconds to a human-readable string. 0 → "0m". */
export function formatPlaytime(totalSec: number): string {
  if (totalSec === 0) return "0m";
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  if (h === 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return `${h}h ${m}m`;
}
