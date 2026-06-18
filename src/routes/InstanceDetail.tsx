import { useEffect, useRef, useState } from "react";
import { useParams, Link } from "react-router-dom";
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  ArrowLeft,
  Download,
  FileBox,
  Key,
  Lock,
  Loader2,
  Package,
  Play,
  RefreshCw,
  Search,
  Settings2,
  Square,
  Trash2,
  Unlock,
  X,
} from "lucide-react";
import {
  addMod,
  getInstance,
  getModVersions,
  launchInstance,
  killInstance,
  removeMod,
  searchMods,
  setModEnabled,
  setPackLock,
  updateMod,
  updateModpack,
  type AddModResult,
  type FolderMod,
  type LaunchLogPayload,
  type LaunchExitPayload,
  type InstallLogPayload,
  type ManualMod,
  type ModEntry,
  type ProjectSummary,
  type ProviderCommandError,
  type UpdateModResult,
  LAUNCH_LOG_EVENT,
  LAUNCH_EXIT_EVENT,
  INSTALL_LOG_EVENT,
} from "@/lib/ipc";
import { SlideOver } from "@/components/SlideOver";
import { ProviderBadge } from "@/components/ProviderBadge";

/** Max log lines kept in the console buffer before oldest are dropped. */
const MAX_LOG_LINES = 500;

const PAGE_LIMIT = 20;
const DEBOUNCE_MS = 400;

// ---------------------------------------------------------------------------
// Root page component
// ---------------------------------------------------------------------------

export function InstanceDetail() {
  const { slug } = useParams();
  const qc = useQueryClient();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["instance", slug],
    queryFn: () => getInstance(slug!),
    enabled: !!slug,
  });

  const [running, setRunning] = useState(false);
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [logLines, setLogLines] = useState<string[]>([]);
  const [slideOverOpen, setSlideOverOpen] = useState(false);
  const consoleRef = useRef<HTMLPreElement>(null);

  // Subscribe to launch://log, launch://exit, and install://log for this instance's slug.
  useEffect(() => {
    if (!slug) return;

    let cancelled = false;
    let unlistenLog: (() => void) | undefined;
    let unlistenExit: (() => void) | undefined;
    let unlistenInstall: (() => void) | undefined;

    listen<LaunchLogPayload>(LAUNCH_LOG_EVENT, (event) => {
      if (event.payload.instanceId !== slug) return;
      setLogLines((prev) => {
        const next = [...prev, `[${event.payload.stream}] ${event.payload.line}`];
        return next.length > MAX_LOG_LINES ? next.slice(next.length - MAX_LOG_LINES) : next;
      });
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenLog = fn;
    }).catch(console.error);

    listen<LaunchExitPayload>(LAUNCH_EXIT_EVENT, (event) => {
      if (event.payload.instanceId !== slug) return;
      setRunning(false);
      const code = event.payload.code;
      setLogLines((prev) => [
        ...prev,
        `[launcher] process exited (code ${code ?? "unknown"})`,
      ]);
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenExit = fn;
    }).catch(console.error);

    // install://log carries no instanceId — the installer runs at most once at a
    // time, so all lines are attributed to the instance being launched right now.
    listen<InstallLogPayload>(INSTALL_LOG_EVENT, (event) => {
      setLogLines((prev) => {
        const next = [...prev, `[install:${event.payload.stream}] ${event.payload.line}`];
        return next.length > MAX_LOG_LINES ? next.slice(next.length - MAX_LOG_LINES) : next;
      });
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenInstall = fn;
    }).catch(console.error);

    return () => {
      cancelled = true;
      unlistenLog?.();
      unlistenExit?.();
      unlistenInstall?.();
    };
  }, [slug]);

  // Auto-scroll console to bottom on new lines.
  useEffect(() => {
    const el = consoleRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [logLines]);

  async function handleLaunch() {
    if (!slug) return;
    setLaunchError(null);
    setLaunching(true);
    setLogLines([]);
    try {
      await launchInstance(slug);
      setRunning(true);
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

  return (
    <div className="px-8 py-7">
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
          <header className="mb-6 flex items-start justify-between gap-4">
            <div>
              <h1 className="text-2xl font-semibold">{data.instance.name}</h1>
              <p className="text-sm text-muted">
                {labelLoader(data.instance.loader.kind)}
                {data.instance.loader.version ? ` ${data.instance.loader.version}` : ""} ·
                Minecraft {data.instance.minecraft}
              </p>
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

          <dl className="mb-8 grid grid-cols-2 gap-4 sm:grid-cols-4">
            <Stat label="Memory" value={`${data.instance.java.memoryMb} MB`} />
            <Stat
              label="Java"
              value={data.instance.java.major ? `Java ${data.instance.java.major}` : "Auto"}
            />
            <Stat label="Managed mods" value={`${data.instance.mods.length}`} />
            <Stat label="Created" value={formatDate(data.instance.created)} />
          </dl>

          <dl className="mb-8 grid grid-cols-2 gap-4 sm:grid-cols-2">
            <Stat
              label="Last played"
              value={data.instance.lastPlayed ? formatDate(data.instance.lastPlayed) : "Never"}
            />
            <Stat
              label="Total playtime"
              value={formatPlaytime(data.instance.totalPlaytimeSec)}
            />
          </dl>

          {(running || logLines.length > 0) && (
            <section className="mb-8">
              <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-muted">
                Console
              </h2>
              <pre
                ref={consoleRef}
                className="h-64 overflow-y-auto rounded-lg border border-border bg-surface p-3 font-mono text-xs text-foreground"
              >
                {logLines.length === 0
                  ? "Waiting for output…"
                  : logLines.join("\n")}
              </pre>
            </section>
          )}

          {/* Pack source + Update + Pack Lock (only when installed via Browse) */}
          {data.instance.source && (
            <PackSourcePanel
              slug={slug!}
              source={data.instance.source}
              packLocked={data.instance.packLocked ?? false}
              provider={data.instance.source.provider}
              projectId={data.instance.source.projectId}
              onMutate={invalidate}
            />
          )}

          {/* Mods summary + Manage installs button */}
          <section>
            <div className="flex items-center justify-between gap-4">
              <div>
                <h2 className="flex items-center gap-2 text-sm font-semibold text-muted">
                  <Package className="size-4" />
                  Mods
                </h2>
                <p className="mt-0.5 text-xs text-muted">
                  {data.folderMods.length === 0
                    ? "No mods installed"
                    : `${data.folderMods.length} file${data.folderMods.length !== 1 ? "s" : ""}` +
                      (data.instance.mods.length > 0
                        ? ` · ${data.instance.mods.length} managed`
                        : "")}
                </p>
              </div>
              <button
                onClick={() => setSlideOverOpen(true)}
                className="inline-flex items-center gap-2 rounded-lg border border-border bg-surface px-4 py-2 text-sm font-medium text-foreground hover:bg-surface-2"
              >
                <Settings2 className="size-4" />
                Manage installs
              </button>
            </div>
          </section>

          {/* Slide-over: manage installs */}
          <SlideOver
            open={slideOverOpen}
            onClose={() => setSlideOverOpen(false)}
            title="Manage installs"
            widthClass="w-full max-w-2xl"
          >
            <ManageInstallsPanel
              instanceSlug={slug!}
              minecraft={data.instance.minecraft}
              loaderKind={data.instance.loader.kind}
              folderMods={data.folderMods}
              modEntries={data.instance.mods}
              packLocked={data.instance.packLocked ?? false}
              onMutate={invalidate}
            />
          </SlideOver>
        </>
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Pack source panel — shown when instance was installed via Browse
// ---------------------------------------------------------------------------

interface PackSourcePanelProps {
  slug: string;
  source: { provider: string; projectId: string; fileId: string; packVersion: string };
  packLocked: boolean;
  provider: string;
  projectId: string;
  onMutate: () => void;
}

function PackSourcePanel({
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
// Manage installs panel (slide-over body)
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

function ManageInstallsPanel({
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
  packLocked: boolean;
  onMutate: () => void;
}

function AddModTab({ instanceSlug, minecraft, loaderKind, packLocked, onMutate }: AddModTabProps) {
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
  packLocked: boolean;
  onInstalled: () => void;
}

function ModSearchCard({
  mod,
  instanceSlug,
  minecraft,
  loaderKind,
  packLocked,
  onInstalled,
}: ModSearchCardProps) {
  const [installing, setInstalling] = useState(false);
  const [result, setResult] = useState<AddModResult | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  // Resolve the provider routing string from the ProviderKind response value.
  // ProviderKind serializes as "modrinth" | "curseForge" (camelCase from Rust).
  const providerRoute: "modrinth" | "curseforge" =
    mod.provider === "modrinth"
      ? "modrinth"
      : mod.provider === "curseForge"
        ? "curseforge"
        : ((_: never) => "curseforge" as const)(mod.provider);

  const versionsQuery = useQuery({
    queryKey: [
      "modVersions",
      mod.provider,
      mod.id,
      minecraft,
      loaderKind,
    ],
    queryFn: () =>
      getModVersions(providerRoute, mod.id, minecraft, loaderKind),
    staleTime: 30_000,
    // Fetch eagerly — the user may click Install immediately.
    enabled: true,
  });

  const primaryVersion = versionsQuery.data?.[0];

  async function handleInstall() {
    if (!primaryVersion) return;
    setInstalling(true);
    setInstallError(null);
    setResult(null);
    try {
      const res = await addMod(
        providerRoute,
        mod.id,
        primaryVersion.id,
        instanceSlug,
        minecraft,
        loaderKind,
      );
      setResult(res);
      if (res.added.length > 0) onInstalled();
    } catch (err) {
      setInstallError(String(err));
    } finally {
      setInstalling(false);
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

        {/* Install button */}
        {result ? null : (
          <button
            onClick={handleInstall}
            disabled={installing || versionsQuery.isLoading || !primaryVersion || packLocked}
            title={
              packLocked
                ? "Pack is locked — unlock to add mods"
                : versionsQuery.isLoading
                  ? "Fetching compatible versions…"
                  : !primaryVersion
                    ? "No compatible version found"
                    : `Install ${primaryVersion.versionNumber}`
            }
            className="inline-flex shrink-0 items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent/80 disabled:opacity-50"
          >
            {installing ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : versionsQuery.isLoading ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              "Install"
            )}
          </button>
        )}
      </div>

      {/* Version fetch error */}
      {versionsQuery.isError && (
        <p className="text-xs text-danger">
          Could not fetch compatible versions
          {versionsQuery.error instanceof Error
            ? `: ${versionsQuery.error.message}`
            : "."}
        </p>
      )}

      {/* Install error */}
      {installError && (
        <p className="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">
          {installError}
        </p>
      )}

      {/* Install result summary */}
      {result && (
        <AddResultSummary result={result} onDismiss={() => setResult(null)} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Add-mod result summary
// ---------------------------------------------------------------------------

interface AddResultSummaryProps {
  result: AddModResult;
  onDismiss: () => void;
}

function AddResultSummary({ result, onDismiss }: AddResultSummaryProps) {
  return (
    <div className="rounded-lg border border-border bg-surface-2 px-3 py-2.5 text-xs">
      <div className="space-y-2">
        {result.added.length > 0 && (
          <p className="text-green-400">
            Added {result.added.length} mod{result.added.length !== 1 ? "s" : ""} successfully.
          </p>
        )}

        {result.manual.length > 0 && (
          <div>
            <p className="mb-1 font-medium text-muted">Requires manual download:</p>
            <ul className="space-y-1">
              {result.manual.map((m) => (
                <ManualEntry key={`${m.projectId}:${m.versionId}`} entry={m} />
              ))}
            </ul>
          </div>
        )}

        {result.unresolved.length > 0 && (
          <div>
            <p className="mb-0.5 font-medium text-muted">Unresolved dependencies:</p>
            <ul className="space-y-0.5 text-muted">
              {result.unresolved.map((u) => (
                <li key={u.projectId}>
                  {u.projectId} — {u.reason}
                </li>
              ))}
            </ul>
          </div>
        )}

        {result.failed.length > 0 && (
          <div>
            <p className="mb-0.5 font-medium text-danger">Failed downloads:</p>
            <ul className="space-y-0.5 text-danger">
              {result.failed.map((f) => (
                <li key={f.fileName}>
                  {f.fileName}: {f.error}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>

      <button
        onClick={onDismiss}
        className="mt-2 flex items-center gap-1 text-muted hover:text-foreground"
      >
        <X className="size-3" />
        Dismiss
      </button>
    </div>
  );
}

function ManualEntry({ entry }: { entry: ManualMod }) {
  return (
    <li className="flex items-center justify-between gap-2 rounded-lg border border-border bg-surface px-3 py-1.5">
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
  const [updateResult, setUpdateResult] = useState<UpdateModResult | null>(null);

  const toggleMutation = useMutation({
    mutationFn: () => setModEnabled(instanceSlug, mod.fileName, mod.disabled),
    onSuccess: onMutate,
  });

  const removeMutation = useMutation({
    mutationFn: () => removeMod(instanceSlug, mod.fileName),
    onSuccess: onMutate,
  });

  const updateMutation = useMutation({
    mutationFn: () => {
      if (!entry) return Promise.reject(new Error("no mod entry"));
      return updateMod(instanceSlug, entry.projectId);
    },
    onSuccess: (res) => {
      setUpdateResult(res);
      if (res.status === "updated") onMutate();
    },
  });

  const isBusy =
    toggleMutation.isPending || removeMutation.isPending || updateMutation.isPending;
  const isDisabled = isBusy || packLocked;

  return (
    <li className="flex flex-col gap-1.5 bg-surface px-4 py-2.5 text-sm">
      <div className="flex items-center gap-3">
        <FileBox className="size-4 shrink-0 text-muted" />
        <span className={`min-w-0 flex-1 truncate ${mod.disabled ? "text-muted line-through" : ""}`}>
          {mod.fileName}
        </span>
        {!mod.managed && (
          <span className="rounded bg-surface-2 px-1.5 py-0.5 text-xs text-muted">
            unmanaged
          </span>
        )}
        <span className="shrink-0 text-xs text-muted">{formatBytes(mod.sizeBytes)}</span>

        {entry && (
          <div className="flex shrink-0 items-center gap-1">
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
              {updateMutation.isPending ? (
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

      {/* Update result feedback */}
      {updateResult && (
        <UpdateResultBadge result={updateResult} onDismiss={() => setUpdateResult(null)} />
      )}

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

interface UpdateResultBadgeProps {
  result: UpdateModResult;
  onDismiss: () => void;
}

function UpdateResultBadge({ result, onDismiss }: UpdateResultBadgeProps) {
  const label = {
    updated: "Updated to latest version.",
    upToDate: "Already up to date.",
    unresolved: "No compatible version found.",
    failed: result.error ?? "Update failed.",
    manual: "Manual download required.",
  }[result.status] ?? result.status;

  return (
    <div className="flex items-center justify-between gap-2 rounded-md border border-border bg-surface-2 px-2.5 py-1 text-xs">
      <span className={result.status === "failed" ? "text-danger" : "text-muted"}>
        {label}
      </span>
      {result.status === "manual" && result.pageUrl && (
        <button
          onClick={() => openUrl(result.pageUrl ?? "").catch(console.error)}
          className="font-medium text-primary hover:underline"
        >
          Open page
        </button>
      )}
      <button onClick={onDismiss} className="text-muted hover:text-foreground">
        <X className="size-3" />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-surface px-4 py-3">
      <dt className="text-xs text-muted">{label}</dt>
      <dd className="mt-0.5 font-medium">{value}</dd>
    </div>
  );
}

function labelLoader(kind: string): string {
  return kind === "vanilla" ? "Vanilla" : kind[0].toUpperCase() + kind.slice(1);
}

function formatDate(iso: string): string {
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
function formatPlaytime(totalSec: number): string {
  if (totalSec === 0) return "0m";
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  if (h === 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return `${h}h ${m}m`;
}
