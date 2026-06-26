import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Loader2, AlertCircle, ExternalLink, AlertTriangle, RefreshCw, Upload } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import {
  getPackInfo,
  importManualFile,
  rescanPendingManual,
  startPendingWatch,
  stopPendingWatch,
} from "@/lib/ipc";
import { PackDescription } from "@/components/PackDescription";

export function InfoTab() {
  const { slug, instance, invalidate } = useOutletContext<InstanceTabContext>();
  const pending = instance.pendingManual ?? [];
  const pendingCount = pending.length;
  const [rescanning, setRescanning] = useState(false);
  const [importing, setImporting] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  // Lazy mods/ watcher: run only while this detail page is mounted AND the pack
  // still has pending manual files. Stops on unmount or when the list empties.
  // A drop into mods/ then clears the row live (the backend emits manual://resolved,
  // AppShell invalidates the instance query). Out-of-app drops are caught by the
  // launch-time scan + the Re-scan button below.
  useEffect(() => {
    if (pendingCount === 0) return;
    startPendingWatch(slug).catch(console.error);
    return () => {
      stopPendingWatch(slug).catch(console.error);
    };
  }, [slug, pendingCount]);

  async function handleRescan() {
    setRescanning(true);
    try {
      await rescanPendingManual(slug);
      invalidate();
    } catch (e) {
      console.error("rescan pending manual failed:", e);
    } finally {
      setRescanning(false);
    }
  }

  async function importPaths(paths: string[]) {
    const jars = paths.filter((p) => p.toLowerCase().endsWith(".jar"));
    if (jars.length === 0) {
      setImportError("Only .jar files can be added.");
      return;
    }
    setImporting(true);
    setImportError(null);
    try {
      for (const p of jars) {
        await importManualFile(slug, p);
      }
      invalidate();
    } catch (e) {
      setImportError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function handlePick() {
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [{ name: "Mod jar", extensions: ["jar"] }],
    });
    if (!selected) return;
    await importPaths(Array.isArray(selected) ? selected : [selected]);
  }

  // Tauri native file-drop: while the panel is visible with pending files, accept
  // dropped .jar paths. The drop event is webview-global; we only act while the
  // pending panel is mounted (pendingCount > 0).
  useEffect(() => {
    if (pendingCount === 0) return;
    let unlisten: (() => void) | undefined;
    let active = true;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over" || event.payload.type === "enter") {
          setDragOver(true);
        } else if (event.payload.type === "leave") {
          setDragOver(false);
        } else if (event.payload.type === "drop") {
          setDragOver(false);
          void importPaths(event.payload.paths);
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(console.error);
    return () => {
      active = false;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [slug, pendingCount]);

  // Provider routing: wire value → lowercase routing string.
  const providerRoute: "modrinth" | "curseforge" | "ftb" | null = instance.source
    ? instance.source.provider === "modrinth"
      ? "modrinth"
      : instance.source.provider === "curseForge"
        ? "curseforge"
        : instance.source.provider === "ftb"
          ? "ftb"
          : null
    : null;

  const infoQuery = useQuery({
    queryKey: ["pack-info", providerRoute, instance.source?.projectId],
    queryFn: () => getPackInfo(providerRoute!, instance.source!.projectId),
    enabled: !!instance.source && providerRoute !== null,
    staleTime: 5 * 60_000,
  });

  if (!instance.source) {
    return (
      <div className="rounded-lg border border-dashed border-border bg-surface/40 px-4 py-8 text-center text-sm text-muted">
        This instance wasn't installed from a provider.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Pending manual-download panel (CF allowModDistribution:false) — only when pending. */}
      {pending.length > 0 && (
        <section className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 text-sm">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 font-medium text-amber-300">
              <AlertTriangle className="size-4 shrink-0" />
              {pending.length} file{pending.length !== 1 ? "s" : ""} need a manual download
            </div>
            <button
              onClick={handleRescan}
              disabled={rescanning}
              title="Re-scan the mods folder for dropped files"
              className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-amber-500/40 bg-amber-500/15 px-2.5 py-1 text-xs font-medium text-amber-200 transition-colors hover:bg-amber-500/25 disabled:opacity-50"
            >
              <RefreshCw className={`size-3.5 ${rescanning ? "animate-spin" : ""}`} />
              Re-scan
            </button>
          </div>
          <p className="mb-3 text-xs text-amber-200/80">
            These mods disable automatic downloads. Get them from CurseForge and drop the
            .jar into this instance's <code className="font-mono">mods/</code> folder — they'll
            be detected automatically.
          </p>
          <ul className="flex flex-col gap-1.5">
            {pending.map((p) => (
              <li
                key={`${p.projectId}:${p.fileId}`}
                className="flex items-center justify-between gap-3 rounded-lg border border-amber-500/20 bg-surface/40 px-3 py-2"
              >
                <span className="min-w-0 truncate font-mono text-xs text-foreground">
                  {p.fileName}
                </span>
                <button
                  onClick={() => openUrl(p.pageUrl).catch(console.error)}
                  title="Open the CurseForge file page in your browser"
                  className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-amber-500/40 bg-amber-500/15 px-2.5 py-1 text-xs font-medium text-amber-200 transition-colors hover:bg-amber-500/25"
                >
                  <ExternalLink className="size-3.5" />
                  Open page
                </button>
              </li>
            ))}
          </ul>

          {/* Drop zone + pick-file fallback */}
          <div
            className={
              "mt-3 flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed px-4 py-5 text-center text-xs transition-colors " +
              (dragOver
                ? "border-amber-400 bg-amber-500/15 text-amber-200"
                : "border-amber-500/30 text-amber-200/70")
            }
          >
            {importing ? (
              <span className="inline-flex items-center gap-2">
                <Loader2 className="size-4 animate-spin" />
                Adding…
              </span>
            ) : (
              <>
                <Upload className="size-5 text-amber-300/80" />
                <span>Drag .jar files here to add them</span>
                <button
                  onClick={handlePick}
                  className="mt-1 inline-flex items-center gap-1.5 rounded-lg border border-amber-500/40 bg-amber-500/15 px-3 py-1 font-medium text-amber-200 transition-colors hover:bg-amber-500/25"
                >
                  Pick file…
                </button>
              </>
            )}
          </div>

          {importError && (
            <p className="mt-2 rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">
              {importError}
            </p>
          )}
        </section>
      )}

      {/* Pack info header + description */}
      {infoQuery.isLoading && (
        <div className="flex items-center gap-2 py-4 text-sm text-muted">
          <Loader2 className="size-4 animate-spin" />
          Loading description…
        </div>
      )}

      {infoQuery.isError && (
        <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          <AlertCircle className="size-4 shrink-0" />
          <span>Couldn't load description: {String(infoQuery.error)}</span>
        </div>
      )}

      {infoQuery.data && (
        // Icon + name are shown in the persistent bar above — Info tab is just the
        // provider description (single render path: Modrinth Markdown / CF HTML).
        <PackDescription markdown={infoQuery.data.description} />
      )}
    </div>
  );
}
