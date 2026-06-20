import { useOutletContext } from "react-router-dom";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { ManageInstallsPanel } from "@/routes/InstanceDetail";

export function ModlistTab() {
  const { slug, instance, folderMods, invalidate } = useOutletContext<InstanceTabContext>();

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ManageInstallsPanel
        instanceSlug={slug}
        minecraft={instance.minecraft}
        loaderKind={instance.loader.kind}
        folderMods={folderMods}
        modEntries={instance.mods}
        packLocked={instance.packLocked ?? false}
        onMutate={invalidate}
      />
    </div>
  );
}
