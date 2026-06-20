/**
 * CP1: Responsive instance card for the Home grid.
 * Visual language mirrors BrowseCard.tsx: Link root, inner buttons with
 * preventDefault/stopPropagation, fallback lucide tile, chip styling.
 */

import { useState } from "react";
import { Link } from "react-router-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Box, Loader2, Lock, Play, Square, Trash2 } from "lucide-react";
import { deleteInstance, killInstance, launchInstance, type Instance } from "@/lib/ipc";
import { useAppStore } from "@/lib/store";
import { ProviderBadge, toProviderKind } from "@/components/ProviderBadge";
import { labelLoader, formatDate, formatPlaytime } from "@/routes/InstanceDetail";

// ---------------------------------------------------------------------------
// InstanceCard
// ---------------------------------------------------------------------------

export interface InstanceCardProps {
  instance: Instance;
}

export function InstanceCard({ instance }: InstanceCardProps) {
  const qc = useQueryClient();
  const [launching, setLaunching] = useState(false);

  // Run state for this specific instance (keyed by slug in the app store).
  const run = useAppStore((s) => s.runs.get(instance.slug));
  const running = run?.status === "preparing" || run?.status === "running";

  // Delete mutation
  const del = useMutation({
    mutationFn: () => deleteInstance(instance.slug),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["instances"] }),
  });

  // Update-available pill: source has a latestVersionId that differs from current fileId.
  const src = instance.source;
  const updateAvailable = !!(src?.latestVersionId && src.latestVersionId !== src.fileId);

  async function handlePlay(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();

    // Mirror InstanceDetail.tsx:159 — warn when another instance is already running.
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
      await launchInstance(instance.slug);
    } catch (err) {
      console.error("launchInstance failed:", err);
    } finally {
      setLaunching(false);
    }
  }

  async function handleStop(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    try {
      await killInstance(instance.slug);
    } catch (err) {
      console.error("killInstance failed:", err);
    }
  }

  function handleDelete(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (confirm(`Delete instance "${instance.name}"? This cannot be undone.`)) {
      del.mutate();
    }
  }

  // Stat line pieces — only include non-empty fragments.
  const statParts: string[] = [];
  if (src?.packVersion) statParts.push(`v${src.packVersion}`);
  statParts.push(`MC ${instance.minecraft}`);
  statParts.push(
    labelLoader(instance.loader.kind) +
      (instance.loader.version ? ` ${instance.loader.version}` : ""),
  );
  statParts.push(`${instance.mods.length} mods`);

  return (
    <Link
      to={`/instances/${instance.slug}`}
      className="group relative flex flex-col gap-3 rounded-xl border border-border bg-surface p-4 text-left transition-colors hover:border-primary"
    >
      {/* Hover: delete button (corner overlay) */}
      <button
        title="Delete instance"
        onClick={handleDelete}
        className="absolute right-2 top-2 z-10 rounded-lg p-1.5 text-muted opacity-0 transition-all hover:bg-surface-2 hover:text-danger group-hover:opacity-100"
      >
        <Trash2 className="size-4" />
      </button>

      {/* Main row: icon + content + big Play/Stop on the right */}
      <div className="flex items-center gap-4">
        {/* Icon */}
        {src?.iconUrl ? (
          <img
            src={src.iconUrl}
            alt=""
            referrerPolicy="no-referrer"
            className="size-28 shrink-0 rounded-xl object-cover"
            loading="lazy"
          />
        ) : (
          <div className="grid size-28 shrink-0 place-items-center rounded-xl bg-surface-2 text-muted">
            <Box className="size-14" />
          </div>
        )}

        <div className="min-w-0 flex-1">
          {/* Name + badges */}
          <div className="flex flex-wrap items-start gap-1.5 pr-6">
            <p className="font-semibold leading-snug text-foreground">{instance.name}</p>
            {src && <ProviderBadge provider={toProviderKind(src.provider)} />}
            {updateAvailable && (
              <span className="inline-flex items-center rounded-md bg-amber-500/15 px-1.5 py-0.5 text-xs font-medium text-amber-400 ring-1 ring-inset ring-amber-500/20">
                Update
              </span>
            )}
            {instance.packLocked && (
              <span
                title="Pack is locked"
                className="inline-flex items-center rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-muted ring-1 ring-inset ring-border"
              >
                <Lock className="size-3" />
              </span>
            )}
          </div>

          {/* Stat line */}
          <p className="mt-1 truncate text-xs text-muted">{statParts.join(" · ")}</p>

          {/* Short description */}
          {src?.summary && (
            <p className="mt-1.5 line-clamp-2 text-sm text-muted">{src.summary}</p>
          )}

          {/* Category chips */}
          {src?.categories && src.categories.length > 0 && (
            <div className="mt-1.5 flex flex-wrap gap-1">
              {src.categories.slice(0, 5).map((cat) => (
                <span
                  key={cat}
                  className="rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-muted"
                >
                  {cat}
                </span>
              ))}
            </div>
          )}

          {/* Playtime + last-played */}
          <p className="mt-1.5 text-xs text-muted">
            {formatPlaytime(instance.totalPlaytimeSec)} ·{" "}
            {instance.lastPlayed ? formatDate(instance.lastPlayed) : "Never played"}
          </p>
        </div>

        {/* Big purple Play / Stop icon button (right, vertically centered) */}
        {running ? (
          <button
            onClick={handleStop}
            title="Stop instance"
            className="relative grid size-14 shrink-0 place-items-center rounded-2xl bg-danger text-white shadow-lg transition hover:brightness-110"
          >
            <span className="absolute right-1.5 top-1.5 size-2 animate-pulse rounded-full bg-white/80" />
            <Square className="size-6" />
          </button>
        ) : (
          <button
            onClick={handlePlay}
            disabled={launching}
            title="Launch instance"
            className="grid size-14 shrink-0 place-items-center rounded-2xl bg-accent text-white shadow-lg transition hover:brightness-125 disabled:opacity-50"
          >
            {launching ? (
              <Loader2 className="size-6 animate-spin" />
            ) : (
              <Play className="size-7 fill-current" />
            )}
          </button>
        )}
      </div>
    </Link>
  );
}
