import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, FolderOpen, Loader2 } from "lucide-react";
import {
  getAppPaths,
  getSettings,
  saveSettings,
  type Settings as SettingsModel,
} from "@/lib/ipc";

export function Settings() {
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["settings"], queryFn: getSettings });
  const { data: paths } = useQuery({ queryKey: ["app-paths"], queryFn: getAppPaths });

  // Local editable copy, seeded once settings load.
  const [form, setForm] = useState<SettingsModel | null>(null);
  useEffect(() => {
    if (data && !form) setForm(data);
  }, [data, form]);

  const save = useMutation({
    mutationFn: (next: SettingsModel) => saveSettings(next),
    onSuccess: (saved) => {
      setForm(saved);
      qc.setQueryData(["settings"], saved);
    },
  });

  if (isLoading || !form) {
    return (
      <div className="px-8 py-7">
        <p className="text-sm text-muted">Loading…</p>
      </div>
    );
  }

  const dirty = JSON.stringify(form) !== JSON.stringify(data);

  return (
    <div className="px-8 py-7">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Settings</h1>
          <p className="text-sm text-muted">Launcher preferences</p>
        </div>
        <button
          onClick={() => save.mutate(form)}
          disabled={!dirty || save.isPending}
          className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
        >
          {save.isPending ? (
            <Loader2 className="size-4 animate-spin" />
          ) : save.isSuccess && !dirty ? (
            <Check className="size-4" />
          ) : null}
          {save.isSuccess && !dirty ? "Saved" : "Save"}
        </button>
      </header>

      <div className="space-y-4">
        <SettingRow
          title="Default memory allocation"
          desc="RAM given to new instances (configurable per-instance later)."
        >
          <div className="flex items-center gap-2">
            <input
              type="number"
              min={512}
              step={512}
              value={form.defaultMemoryMb}
              onChange={(e) =>
                setForm({ ...form, defaultMemoryMb: Number(e.target.value) || 0 })
              }
              className="input w-28 text-right"
            />
            <span className="text-sm text-muted">MB</span>
          </div>
        </SettingRow>

        <SettingRow
          title="Default JVM arguments"
          desc="Applied to instances without a per-instance override."
        >
          <input
            value={form.defaultJavaArgs}
            onChange={(e) => setForm({ ...form, defaultJavaArgs: e.target.value })}
            placeholder="-XX:+UseG1GC"
            className="input w-72 font-mono text-xs"
          />
        </SettingRow>

        <SettingRow
          title="Java"
          desc="Per-instance JREs are downloaded automatically (Phase 2)."
        >
          <span className="text-sm text-muted">Auto</span>
        </SettingRow>

        {paths && (
          <SettingRow
            title="Data folder"
            desc="Where instances, settings, and shared caches live."
          >
            <span className="flex items-center gap-2 text-xs text-muted">
              <FolderOpen className="size-4 shrink-0" />
              <span className="max-w-md truncate font-mono" title={paths.dataDir}>
                {paths.dataDir}
              </span>
            </span>
          </SettingRow>
        )}
      </div>

      <div className="mt-8 space-y-4">
        <div>
          <h2 className="text-base font-semibold">Advanced</h2>
          <p className="text-xs text-muted">API keys and other advanced options.</p>
        </div>

        <div>
          <h3 className="text-xs font-medium uppercase tracking-wide text-muted">API Keys</h3>
          <SettingRow
            title="CurseForge API key"
            desc="Optional override. A key is bundled by default — enter your own to use it instead."
          >
            <input
              type="password"
              value={form.curseforgeApiKey ?? ""}
              onChange={(e) =>
                setForm({ ...form, curseforgeApiKey: e.target.value || null })
              }
              placeholder="Leave blank to use bundled key"
              className="input w-72 font-mono text-xs"
            />
          </SettingRow>
        </div>
      </div>

      {save.isError && (
        <p className="mt-4 text-sm text-danger">{String(save.error)}</p>
      )}
    </div>
  );
}

function SettingRow({
  title,
  desc,
  children,
}: {
  title: string;
  desc: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border border-border bg-surface px-5 py-4">
      <div className="min-w-0">
        <div className="text-sm font-medium">{title}</div>
        <div className="text-xs text-muted">{desc}</div>
      </div>
      {children}
    </div>
  );
}
