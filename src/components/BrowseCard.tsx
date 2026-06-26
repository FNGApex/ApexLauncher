/**
 * BR-D: Larger responsive grid card for Browse.
 * BR-B: Renders "Installed" pill and deep-links to instance when matched.
 */

import { CheckCircle, Download, ExternalLink, Package } from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ProjectSummary } from "@/lib/ipc";
import { ProviderBadge } from "@/components/ProviderBadge";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export function formatDownloads(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function providerRoute(pack: ProjectSummary): "modrinth" | "curseforge" | "ftb" {
  return pack.provider === "modrinth"
    ? "modrinth"
    : pack.provider === "curseForge"
      ? "curseforge"
      : "ftb";
}

// ---------------------------------------------------------------------------
// BrowseCard
// ---------------------------------------------------------------------------

export interface BrowseCardProps {
  pack: ProjectSummary;
  /** Slug of the installed instance, or null if not installed. */
  installedSlug: string | null;
}

export function BrowseCard({ pack, installedSlug }: BrowseCardProps) {
  const navigate = useNavigate();
  const route = providerRoute(pack);

  function handleOpenPage(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (pack.pageUrl == null) return;
    openUrl(pack.pageUrl).catch(console.error);
  }

  function handleOpenInstance(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (installedSlug == null) return;
    navigate(`/instances/${installedSlug}`);
  }

  return (
    <Link
      to={`/browse/${route}/${pack.id}`}
      state={{ pack }}
      className="group relative flex flex-col gap-3 rounded-xl border border-border bg-surface p-4 text-left transition-colors hover:border-primary"
    >
      {/* Top row: icon + title + provider badge + external link */}
      <div className="flex items-start gap-3">
        {/* Icon */}
        {pack.iconUrl ? (
          <img
            src={pack.iconUrl}
            alt=""
            referrerPolicy="no-referrer"
            className="size-16 shrink-0 rounded-xl object-cover"
            loading="lazy"
          />
        ) : (
          <div className="grid size-16 shrink-0 place-items-center rounded-xl bg-surface-2 text-muted">
            <Package className="size-8" />
          </div>
        )}

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-start gap-1.5">
            <p className="font-semibold leading-snug text-foreground">{pack.name}</p>
            <ProviderBadge provider={pack.provider} />
            {/* BR-B: installed pill */}
            {installedSlug != null && (
              <button
                onClick={handleOpenInstance}
                title="Open instance"
                className="inline-flex items-center gap-1 rounded-md bg-success/15 px-1.5 py-0.5 text-xs font-medium text-success ring-1 ring-inset ring-success/20 hover:bg-success/25 transition-colors"
              >
                <CheckCircle className="size-3" />
                Installed
              </button>
            )}
          </div>

          {/* Downloads */}
          <div className="mt-1 flex items-center gap-1 text-xs text-muted">
            <Download className="size-3.5" />
            <span>{formatDownloads(pack.downloads)}</span>
          </div>
        </div>

        {/* External link (secondary action) */}
        {pack.pageUrl != null && (
          <button
            onClick={handleOpenPage}
            title="Open project page"
            className="ml-auto shrink-0 rounded-lg p-1.5 text-muted opacity-0 transition-all hover:bg-surface-2 hover:text-foreground group-hover:opacity-100"
          >
            <ExternalLink className="size-4" />
          </button>
        )}
      </div>

      {/* Summary */}
      <p className="line-clamp-3 text-sm text-muted">{pack.summary}</p>

      {/* Category chips */}
      {pack.categories.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {pack.categories.slice(0, 5).map((cat) => (
            <span
              key={cat}
              className="rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-muted"
            >
              {cat}
            </span>
          ))}
        </div>
      )}
    </Link>
  );
}
