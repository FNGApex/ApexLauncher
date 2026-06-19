import { useOutletContext } from "react-router-dom";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { PackSourcePanel } from "@/routes/InstanceDetail";

export function InfoTab() {
  const { slug, instance, invalidate } = useOutletContext<InstanceTabContext>();

  if (!instance.source) {
    return (
      <div className="rounded-lg border border-dashed border-border bg-surface/40 px-4 py-8 text-center text-sm text-muted">
        This instance wasn't installed from a provider.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <PackSourcePanel
        slug={slug}
        source={instance.source}
        packLocked={instance.packLocked ?? false}
        provider={instance.source.provider}
        projectId={instance.source.projectId}
        onMutate={invalidate}
      />
      <p className="text-sm text-muted">Provider description coming soon.</p>
    </div>
  );
}
