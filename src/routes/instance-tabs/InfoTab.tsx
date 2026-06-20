import { useOutletContext } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Loader2, AlertCircle } from "lucide-react";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { PackSourcePanel } from "@/routes/InstanceDetail";
import { getPackInfo } from "@/lib/ipc";
import { PackDescription } from "@/components/PackDescription";

export function InfoTab() {
  const { slug, instance, invalidate } = useOutletContext<InstanceTabContext>();

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
        <div className="flex flex-col gap-4">
          {/* Title row: icon + name */}
          <div className="flex items-center gap-3">
            {infoQuery.data.iconUrl && (
              <img
                src={infoQuery.data.iconUrl}
                alt=""
                className="size-14 shrink-0 rounded-xl object-cover"
                loading="lazy"
              />
            )}
            <h2 className="text-xl font-semibold">{infoQuery.data.title}</h2>
          </div>

          {/* Description: single render path handles both Modrinth Markdown and CF HTML */}
          <PackDescription markdown={infoQuery.data.description} />
        </div>
      )}

      {/* Pack source panel — update/lock stays reachable */}
      <PackSourcePanel
        slug={slug}
        source={instance.source}
        packLocked={instance.packLocked ?? false}
        provider={instance.source.provider}
        projectId={instance.source.projectId}
        onMutate={invalidate}
      />
    </div>
  );
}
