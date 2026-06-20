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
  Lock,
  Loader2,
  Package,
  Play,
  RefreshCw,
  Search,
  Square,
  Trash2,
  Unlock,
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
  removeMod,
  searchMods,
  setModEnabled,
  setPackLock,
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

const PAGE_LIMIT = 20;
const DEBOUNCE_MS = 400;

// Tracks instances whose mod metadata backfill has been triggered this session, so
// the lazy enrich (for modpack-imported mods without metadata) runs at most once per
// instance. The backend enrich is also idempotent (0 network calls when nothing is
// missing); this just avoids redundant in-flight calls before the refetch lands.
const enrichedSlugs = new Set<string>();

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
              Header — persistent across all tab switches
          ---------------------------------------------------------------- */}
          <header className="mb-4 flex items-start justify-between gap-4">
            <div>
              <h1 className="text-2xl font-semibold">{data.instance.name}</h1>
              <div className="mt-0.5 flex flex-wrap items-center gap-2">
                <p className="text-sm text-muted">
                  {labelLoader(data.instance.loader.kind)}
                  {data.instance.loader.version ? ` ${data.instance.loader.version}` : ""} ·
                  Minecraft {data.instance.minecraft}
                </p>
                {data.instance.source && (
                  <span className="inline-flex items-center rounded-full border border-border bg-surface px-2 py-0.5 text-xs text-muted">
                    Pack v{data.instance.source.packVersion}
                  </span>
                )}
              </div>
            </div>

            <div className="flex shrink-0 items-center gap-3">
              {running && (
                <span className="inline-flex items-center gap-1.5 rounded-full bg-green-500/15 px-2.5 py-1 text-xs font-medium text-green-400">
                  <span className="size-1.5 animate-pulse rounded-full bg-green-400" />
                  Running
                </span>
              )}
              {running ? (
                <button
                  onClick={handleStop}
                  className="inline-flex items-center gap-2 rounded-lg border border-border bg-surface px-4 py-2 text-sm font-medium text-danger hover:bg-surface-2"
                >
                  <Square className="size-4" />
                  Stop
                </button>
              ) : (
                <button
                  onClick={handleLaunch}
                  disabled={launching}
                  className="inline-flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent/80 disabled:opacity-50"
                >
                  <Play className="size-4" />
                  {launching ? "Launching…" : "Launch"}
                </button>
              )}
            </div>
          </header>

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
              Tech Info
            </NavLink>
            <NavLink
              to="java"
              className={({ isActive }) =>
                tabBase + (isActive ? " " + tabActive : "")
              }
            >
              Java
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
// Pack source panel — shown in InfoTab when instance was installed via Browse
// Exported so InfoTab can import it.
// ---------------------------------------------------------------------------

interface PackSourcePanelProps {
  slug: string;
  source: { provider: string; projectId: string; fileId: string; packVersion: string; pageUrl?: string | null };
  packLocked: boolean;
  provider: string;
  projectId: string;
  onMutate: () => void;
}

export function PackSourcePanel({
  slug,
  source,
  packLocked,
  provider,
  projectId,
  onMutate,
}: PackSourcePanelProps) {
  const qc = useQueryClient();
  const [selectedVersionId, setSelectedVersionId] = useState<string>("latest");
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [lockError, setLockError] = useState<string | null>(null);

  // Resolve the provider routing string (InstanceSource stores the wire value).
  const providerRoute: "modrinth" | "curseforge" =
    provider === "modrinth"
      ? "modrinth"
      : provider === "curseForge"
        ? "curseforge"
        : ((_: never) => "curseforge" as const)(provider as never);

  // Fetch versions for the picker (no MC/loader filter).
  const versionsQuery = useQuery({
    queryKey: ["packVersions", provider, projectId],
    queryFn: () => getModVersions(providerRoute, projectId, null, null),
    staleTime: 30_000,
  });

  const updateMutation = useMutation({
    mutationFn: () =>
      updateModpack(slug, selectedVersionId === "latest" ? undefined : selectedVersionId),
    onSuccess: (_taskId) => {
      // updateModpack now returns a task id; the result arrives via task://update.
      // CP-9 wires the completion toast. Invalidate so data refreshes when done.
      setUpdateError(null);
      qc.invalidateQueries({ queryKey: ["instance", slug] });
      qc.invalidateQueries({ queryKey: ["instances"] });
      onMutate();
    },
    onError: (err) => {
      setUpdateError(err instanceof Error ? err.message : String(err));
    },
  });

  const lockMutation = useMutation({
    mutationFn: () => setPackLock(slug, !packLocked),
    onSuccess: () => {
      setLockError(null);
      qc.invalidateQueries({ queryKey: ["instance", slug] });
      onMutate();
    },
    onError: (err) => {
      setLockError(err instanceof Error ? err.message : String(err));
    },
  });

  // Build the project page URL: prefer the stored pageUrl, then construct a fallback.
  // CurseForge fallback is omitted (no stable URL template) — button is hidden in that case.
  const projectPageUrl: string | null = (() => {
    if (source.pageUrl) return source.pageUrl;
    const route = providerRoute;
    if (route === "modrinth") return `https://modrinth.com/modpack/${projectId}`;
    return null; // curseforge without a stored pageUrl → hide button
  })();

  return (
    <section className="mb-8 rounded-lg border border-border bg-surface px-4 py-4">
      <div className="mb-3 flex items-center justify-between gap-4">
        <div>
          <h2 className="text-sm font-semibold">Pack source</h2>
          <p className="mt-0.5 text-xs text-muted">
            <ProviderBadge provider={provider as "modrinth" | "curseForge"} />{" "}
            <span className="ml-1">Current version: {source.packVersion}</span>
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {/* Open project page */}
          {projectPageUrl && (
            <button
              onClick={() => openUrl(projectPageUrl).catch(console.error)}
              title="Open project page in browser"
              className="inline-flex items-center gap-1.5 rounded-lg border border-border bg-surface px-3 py-1.5 text-xs font-medium text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
            >
              <ExternalLink className="size-3.5" />
              Open project page
            </button>
          )}

          {/* Pack Lock toggle */}
          <button
            onClick={() => lockMutation.mutate()}
            disabled={lockMutation.isPending}
            title={packLocked ? "Unlock pack (allow mod changes)" : "Lock pack (freeze mod list)"}
            className={
              "inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors disabled:opacity-50 " +
              (packLocked
                ? "border-amber-500/40 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20"
                : "border-border bg-surface text-muted hover:bg-surface-2 hover:text-foreground")
            }
          >
            {lockMutation.isPending ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : packLocked ? (
              <Lock className="size-3.5" />
            ) : (
              <Unlock className="size-3.5" />
            )}
            {packLocked ? "Locked" : "Lock pack"}
          </button>
        </div>
      </div>

      {/* Lock error */}
      {lockError && (
        <p className="mt-2 rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">
          {lockError}
        </p>
      )}

      {/* Update row */}
      <div className="flex flex-wrap items-center gap-2">
        {versionsQuery.data && versionsQuery.data.length > 0 && (
          <select
            value={selectedVersionId}
            onChange={(e) => setSelectedVersionId(e.target.value)}
            disabled={updateMutation.isPending}
            className="input w-auto max-w-[180px] text-xs disabled:opacity-50"
          >
            <option value="latest">Latest</option>
            {versionsQuery.data.map((v) => (
              <option key={v.id} value={v.id}>
                {v.name || v.versionNumber}
              </option>
            ))}
          </select>
        )}

        <button
          onClick={() => updateMutation.mutate()}
          disabled={updateMutation.isPending || versionsQuery.isLoading}
          className="inline-flex items-center gap-1.5 rounded-lg bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-60"
        >
          {updateMutation.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <RefreshCw className="size-3.5" />
          )}
          {updateMutation.isPending ? "Updating…" : "Update"}
        </button>
      </div>

      {/* Update error */}
      {updateError && (
        <p className="mt-2 rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">
          {updateError}
        </p>
      )}

    </section>
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
      {/* Pack-locked notice */}
      {packLocked && (
        <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-2.5 text-xs text-amber-400">
          <Lock className="size-3.5 shrink-0" />
          <span>Pack is locked — mod changes are disabled. Unlock the pack to make changes.</span>
        </div>
      )}

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
            {t === "installed" ? "Installed" : "Add mod"}
          </button>
        ))}
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
            title={packLocked ? "Pack is locked — unlock to remove mods" : "Remove mod"}
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
            title={packLocked ? "Pack is locked — unlock to add mods" : "Add mod"}
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
      <div className="flex items-start gap-3">
        {/* Icon: stored iconUrl or fallback */}
        {entry?.iconUrl ? (
          <img
            src={entry.iconUrl}
            alt=""
            referrerPolicy="no-referrer"
            className={`size-10 shrink-0 rounded-lg object-cover ${mod.disabled ? "opacity-50" : ""}`}
            loading="lazy"
          />
        ) : (
          <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted">
            <FileBox className="size-5" />
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
            {/* Enable / disable toggle */}
            <button
              onClick={() => toggleMutation.mutate()}
              disabled={isDisabled}
              title={packLocked ? "Pack is locked" : mod.disabled ? "Enable mod" : "Disable mod"}
              className={
                "rounded px-2 py-0.5 text-xs font-medium transition-colors disabled:opacity-50 " +
                (mod.disabled
                  ? "bg-surface-2 text-muted hover:text-foreground"
                  : "bg-surface-2 text-foreground hover:text-muted")
              }
            >
              {toggleMutation.isPending ? (
                <Loader2 className="size-3 animate-spin" />
              ) : mod.disabled ? (
                "Enable"
              ) : (
                "Disable"
              )}
            </button>

            {/* Update */}
            <button
              onClick={() => updateMutation.mutate()}
              disabled={isDisabled}
              title={packLocked ? "Pack is locked" : "Check for update"}
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
              title={packLocked ? "Pack is locked" : "Remove mod"}
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
