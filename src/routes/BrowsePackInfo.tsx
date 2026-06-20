/**
 * Pack info page — navigated to from Browse cards.
 * Route: /browse/:provider/:id  (provider = "modrinth" | "curseforge")
 *
 * BP-2: header + lazy getPackInfo → PackDescription
 * BP-3: Download button → version-select modal, lazy getModVersions
 */

import { useState } from "react";
import { Link, useLocation, useParams } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, ArrowLeft, Download, Loader2, Package, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  getModVersions,
  getPackInfo,
  installModpack,
  type ProjectSummary,
} from "@/lib/ipc";
import { ProviderBadge } from "@/components/ProviderBadge";
import { PackDescription } from "@/components/PackDescription";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatDownloads(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/**
 * Map the routing string back to the wire provider value used by installModpack.
 * When `pack` is available, prefer `pack.provider` (already wire form).
 */
function toWireProvider(routeProvider: string): string {
  return routeProvider === "curseforge" ? "curseForge" : "modrinth";
}

// ---------------------------------------------------------------------------
// Root component
// ---------------------------------------------------------------------------

export function BrowsePackInfo() {
  const { provider = "", id = "" } = useParams<{ provider: string; id: string }>();
  const location = useLocation();
  const pack = (location.state as { pack?: ProjectSummary } | null)?.pack;

  // provider is the routing string "modrinth" | "curseforge"
  const providerParam = provider as "modrinth" | "curseforge";

  const [promptOpen, setPromptOpen] = useState(false);

  // Lazy: fetch pack description only on this page, not on Browse list.
  const infoQuery = useQuery({
    queryKey: ["pack-info", provider, id],
    queryFn: () => getPackInfo(providerParam, id),
    staleTime: 5 * 60_000,
    enabled: !!provider && !!id,
  });

  const title = pack?.name ?? infoQuery.data?.title ?? id;
  const iconUrl = pack?.iconUrl ?? infoQuery.data?.iconUrl ?? null;
  const providerWire = pack?.provider ?? toWireProvider(provider);

  return (
    <div className="flex h-full flex-col px-8 py-7">
      {/* Back link */}
      <Link
        to="/browse"
        className="mb-6 flex w-fit items-center gap-1.5 text-sm text-muted hover:text-foreground"
      >
        <ArrowLeft className="size-4" />
        Back to Browse
      </Link>

      {/* Header */}
      <div className="mb-6 flex items-center gap-4">
        {iconUrl ? (
          <img
            src={iconUrl}
            alt=""
            className="size-16 shrink-0 rounded-xl object-cover"
            loading="lazy"
          />
        ) : (
          <div className="grid size-16 shrink-0 place-items-center rounded-xl bg-surface-2 text-muted">
            <Package className="size-8" />
          </div>
        )}

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-2xl font-semibold">{title}</h1>
            <ProviderBadge provider={providerWire as "modrinth" | "curseForge"} />
          </div>
          {pack?.summary && (
            <p className="mt-1 text-sm text-muted">{pack.summary}</p>
          )}
          {pack?.downloads != null && (
            <div className="mt-1 flex items-center gap-1 text-xs text-muted">
              <Download className="size-3.5" />
              <span>{formatDownloads(pack.downloads)}</span>
            </div>
          )}
        </div>

        {/* Download button — always available; shows "Latest" immediately */}
        <div className="shrink-0 text-right">
          <p className="mb-1.5 text-xs text-muted">Latest version</p>
          <button
            onClick={() => setPromptOpen(true)}
            className="flex items-center gap-1.5 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
          >
            <Download className="size-4" />
            Download
          </button>
        </div>
      </div>

      {/* Description */}
      <div className="min-h-0 flex-1 overflow-y-auto">
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
          <PackDescription markdown={infoQuery.data.description} />
        )}
      </div>

      {/* Download prompt modal (BP-3) */}
      {promptOpen && (
        <DownloadPrompt
          provider={provider}
          providerWire={providerWire}
          id={id}
          pack={pack}
          onClose={() => setPromptOpen(false)}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// BP-3: Download prompt
// ---------------------------------------------------------------------------

interface DownloadPromptProps {
  provider: string;
  providerWire: string;
  id: string;
  pack: ProjectSummary | undefined;
  onClose: () => void;
}

function DownloadPrompt({ provider, providerWire, id, pack, onClose }: DownloadPromptProps) {
  const qc = useQueryClient();
  const [selectedVersionId, setSelectedVersionId] = useState<string>("latest");
  const [installing, setInstalling] = useState(false);
  const [installQueued, setInstallQueued] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  // Lazy: versions fetched only when the prompt opens.
  const versionsQuery = useQuery({
    queryKey: ["packVersions", provider, id],
    queryFn: () => getModVersions(provider as "modrinth" | "curseforge", id, null, null),
    staleTime: 30_000,
    enabled: true, // prompt is already open when this mounts
  });

  async function handleInstall() {
    setInstalling(true);
    setInstallError(null);
    try {
      await installModpack(
        providerWire,
        id,
        pack?.pageUrl ?? undefined,
        selectedVersionId === "latest" ? undefined : selectedVersionId,
      );
      qc.invalidateQueries({ queryKey: ["instances"] });
      setInstallQueued(true);
    } catch (err) {
      const raw = err instanceof Error ? err.message : String(err);
      // Distribution-disabled pack: backend returns Err("MANUAL:<json>") where
      // json is {"pageUrl":"...","fileName":"..."}. Open the page and show a
      // targeted message instead of surfacing the raw error string.
      if (raw.startsWith("MANUAL:")) {
        try {
          const payload = JSON.parse(raw.slice("MANUAL:".length)) as {
            pageUrl?: string;
            fileName?: string;
          };
          if (payload.pageUrl) {
            openUrl(payload.pageUrl).catch(console.error);
          }
          setInstallError(
            `This pack cannot be auto-downloaded. Opening the page${payload.fileName ? ` to download "${payload.fileName}"` : ""} manually.`,
          );
        } catch {
          setInstallError(raw);
        }
      } else {
        setInstallError(raw);
      }
    } finally {
      setInstalling(false);
    }
  }

  return (
    /* Backdrop */
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      {/* Modal */}
      <div className="w-full max-w-sm rounded-2xl border border-border bg-surface p-6 shadow-xl">
        {/* Header */}
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold">Download</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted hover:bg-surface-2 hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* Version select */}
        <label className="mb-1 block text-xs text-muted">Version</label>
        <div className="relative mb-4">
          <select
            value={selectedVersionId}
            onChange={(e) => setSelectedVersionId(e.target.value)}
            disabled={installing}
            className="input w-full disabled:opacity-50"
          >
            <option value="latest">Latest</option>
            {versionsQuery.data?.map((v) => (
              <option key={v.id} value={v.id}>
                {v.name || v.versionNumber}
              </option>
            ))}
          </select>
          {versionsQuery.isLoading && (
            <Loader2 className="pointer-events-none absolute right-8 top-1/2 size-3.5 -translate-y-1/2 animate-spin text-muted" />
          )}
        </div>

        {/* Error */}
        {installError != null && (
          <div className="mb-3 flex items-start gap-2 rounded-xl border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
            <AlertCircle className="mt-0.5 size-3.5 shrink-0" />
            <span>{installError}</span>
          </div>
        )}

        {/* Queued notice */}
        {installQueued && (
          <div className="mb-3 rounded-xl border border-border bg-surface/60 px-3 py-2 text-xs text-muted">
            Install queued — check progress in the Download Manager.
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-2">
          <button
            onClick={onClose}
            className="flex-1 rounded-lg border border-border px-3 py-2 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
          >
            {installQueued ? "Close" : "Cancel"}
          </button>
          {!installQueued && (
            <button
              onClick={handleInstall}
              disabled={installing}
              className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:cursor-default disabled:opacity-60"
            >
              {installing ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Download className="size-4" />
              )}
              {installing ? "Installing…" : "Install"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
