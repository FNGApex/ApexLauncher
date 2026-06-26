import { useOutletContext } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Loader2, AlertCircle, ExternalLink, AlertTriangle } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { getPackInfo } from "@/lib/ipc";
import { PackDescription } from "@/components/PackDescription";

export function InfoTab() {
  const { instance } = useOutletContext<InstanceTabContext>();
  const pending = instance.pendingManual ?? [];

  // Provider routing: wire value "modrinth" | "curseForge" → routing string "modrinth" | "curseforge".
  const providerRoute: "modrinth" | "curseforge" | null = instance.source
    ? instance.source.provider === "modrinth"
      ? "modrinth"
      : instance.source.provider === "curseForge"
        ? "curseforge"
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
          <div className="mb-2 flex items-center gap-2 font-medium text-amber-300">
            <AlertTriangle className="size-4 shrink-0" />
            {pending.length} file{pending.length !== 1 ? "s" : ""} need a manual download
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
