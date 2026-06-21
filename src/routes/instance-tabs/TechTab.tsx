import { useOutletContext } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { Stat, formatDate, formatPlaytime, labelLoader } from "@/routes/InstanceDetail";
import { getSettings, setPackLock } from "@/lib/ipc";
import { Toggle } from "@/components/Toggle";
import { JavaTab } from "@/routes/instance-tabs/JavaTab";

export function TechTab() {
  const { slug, instance, folderMods, invalidate } = useOutletContext<InstanceTabContext>();
  const qc = useQueryClient();

  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
    staleTime: 60_000,
  });

  const packLockMutation = useMutation({
    mutationFn: () => setPackLock(slug, !(instance.packLocked ?? false)),
    onSuccess: () => {
      invalidate();
      qc.invalidateQueries({ queryKey: ["instances"] });
    },
  });

  const settings = settingsQuery.data;

  // --- Effective memory ---
  const usePackSettings = instance.java.usePackSettings ?? false;
  const effectiveMemoryMb = usePackSettings
    ? instance.java.memoryMb
    : (settings?.defaultMemoryMb ?? instance.java.memoryMb);
  const memoryTier = usePackSettings ? "(pack settings)" : "(global default)";

  // --- Effective java ---
  const pathOverride = instance.java.pathOverride ?? null;
  const major = instance.java.major ?? null;
  let effectiveJava: string;
  if (usePackSettings && pathOverride) {
    effectiveJava = pathOverride;
  } else if (major) {
    effectiveJava = `Auto (Java ${major})`;
  } else {
    effectiveJava = "Auto";
  }
  const javaTier = usePackSettings ? "(pack settings)" : "(global default)";

  // --- Loader display ---
  const loaderLabel =
    instance.loader.kind === "vanilla"
      ? "Vanilla"
      : `${labelLoader(instance.loader.kind)}${instance.loader.version ? ` ${instance.loader.version}` : ""}`;

  // --- Mod counts ---
  const managedCount = instance.mods.length;
  const totalCount = folderMods.length;

  return (
    <div className="flex flex-col gap-6">
      {/* Managed pack toggle */}
      {instance.source && (
        <div className="flex items-center justify-between gap-4 rounded-lg border border-border bg-surface px-4 py-3">
          <div>
            <p className="text-sm font-medium">Managed pack</p>
            <p className="mt-0.5 text-xs text-muted">
              When on, this pack's mods are managed automatically — manual mod changes are disabled.
            </p>
          </div>
          <Toggle
            checked={instance.packLocked ?? false}
            onChange={() => packLockMutation.mutate()}
            disabled={packLockMutation.isPending}
            label="Managed pack"
          />
        </div>
      )}

      {/* Version & loader */}
      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        <Stat label="Minecraft" value={instance.minecraft} />
        <Stat label="Loader" value={loaderLabel} />
        <Stat
          label="Mods"
          value={
            managedCount === totalCount
              ? `${totalCount}`
              : `${totalCount} total · ${managedCount} managed`
          }
        />
      </dl>

      {/* Time stats */}
      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        <Stat label="Created" value={formatDate(instance.created)} />
        <Stat
          label="Last played"
          value={instance.lastPlayed ? formatDate(instance.lastPlayed) : "Never"}
        />
        <Stat
          label="Total playtime"
          value={formatPlaytime(instance.totalPlaytimeSec)}
        />
      </dl>

      {/* Effective resource settings */}
      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-2">
        <div className="rounded-lg border border-border bg-surface px-4 py-3">
          <dt className="text-xs text-muted">Memory</dt>
          <dd className="mt-0.5 font-medium">{effectiveMemoryMb} MB</dd>
          {usePackSettings && instance.java.minMemoryMb ? (
            <dd className="mt-0.5 text-xs text-muted">
              Min: {instance.java.minMemoryMb} MB
            </dd>
          ) : null}
          <dd className="mt-1 text-xs text-muted">{memoryTier}</dd>
        </div>

        <div className="rounded-lg border border-border bg-surface px-4 py-3">
          <dt className="text-xs text-muted">Java</dt>
          <dd className="mt-0.5 break-all font-medium">{effectiveJava}</dd>
          <dd className="mt-1 text-xs text-muted">{javaTier}</dd>
        </div>
      </dl>

      {/* Java & Memory settings (merged from the former Java tab) */}
      <div className="border-t border-border pt-6">
        <h2 className="mb-4 text-sm font-semibold text-foreground">Java &amp; Memory</h2>
        <JavaTab />
      </div>
    </div>
  );
}
