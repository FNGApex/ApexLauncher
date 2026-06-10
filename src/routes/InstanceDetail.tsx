import { useEffect, useRef, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, FileBox, Package, Play, Square } from "lucide-react";
import {
  getInstance,
  launchInstance,
  killInstance,
  type FolderMod,
  type LaunchLogPayload,
  type LaunchExitPayload,
  type InstallLogPayload,
  LAUNCH_LOG_EVENT,
  LAUNCH_EXIT_EVENT,
  INSTALL_LOG_EVENT,
} from "@/lib/ipc";

/** Max log lines kept in the console buffer before oldest are dropped. */
const MAX_LOG_LINES = 500;

export function InstanceDetail() {
  const { slug } = useParams();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["instance", slug],
    queryFn: () => getInstance(slug!),
    enabled: !!slug,
  });

  const [running, setRunning] = useState(false);
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [logLines, setLogLines] = useState<string[]>([]);
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

          <section>
            <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-muted">
              <Package className="size-4" />
              Mods ({data.folderMods.length})
            </h2>
            {data.folderMods.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border bg-surface/40 px-4 py-8 text-center text-sm text-muted">
                No mods in this instance yet. Adding mods from CurseForge &
                Modrinth arrives in Phase 5.
              </div>
            ) : (
              <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border">
                {data.folderMods.map((m) => (
                  <ModRow key={m.fileName} mod={m} />
                ))}
              </ul>
            )}
          </section>
        </>
      ) : null}
    </div>
  );
}

function ModRow({ mod }: { mod: FolderMod }) {
  return (
    <li className="flex items-center gap-3 bg-surface px-4 py-2.5 text-sm">
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
    </li>
  );
}

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

/** Convert total seconds to a human-readable string. 0 → "0m". */
function formatPlaytime(totalSec: number): string {
  if (totalSec === 0) return "0m";
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  if (h === 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return `${h}h ${m}m`;
}
