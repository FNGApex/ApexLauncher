import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Box, Loader2, Plus, Server, Trash2, Upload, X } from "lucide-react";
import {
  deleteInstance,
  importCurseforgeZip,
  importMrpack,
  listInstances,
  type CfImportResult,
  type Instance,
  type MrpackImportResult,
} from "@/lib/ipc";
import { NewInstanceModal } from "@/components/NewInstanceModal";

export function Home() {
  const [showModal, setShowModal] = useState(false);
  const [importResult, setImportResult] = useState<MrpackImportResult | null>(null);
  const [cfImportResult, setCfImportResult] = useState<CfImportResult | null>(null);
  const { data: instances, isLoading } = useQuery({
    queryKey: ["instances"],
    queryFn: listInstances,
  });
  const qc = useQueryClient();
  const navigate = useNavigate();

  const importMutation = useMutation({
    mutationFn: async () => {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Modrinth modpack", extensions: ["mrpack"] }],
      });
      if (!selected) return null;
      return importMrpack(selected);
    },
    onSuccess: async (result) => {
      if (!result) return;
      await qc.invalidateQueries({ queryKey: ["instances"] });
      setImportResult(result);
      navigate(`/instances/${result.slug}`);
    },
  });

  const importCfMutation = useMutation({
    mutationFn: async () => {
      const selected = await open({
        multiple: false,
        filters: [{ name: "CurseForge modpack", extensions: ["zip"] }],
      });
      if (!selected) return null;
      return importCurseforgeZip(selected);
    },
    onSuccess: async (result) => {
      if (!result) return;
      await qc.invalidateQueries({ queryKey: ["instances"] });
      setCfImportResult(result);
      navigate(`/instances/${result.slug}`);
    },
  });

  return (
    <div className="px-8 py-7">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Instances</h1>
          <p className="text-sm text-muted">Your Minecraft profiles</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => importMutation.mutate()}
            disabled={importMutation.isPending}
            className="flex items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-surface disabled:opacity-50"
          >
            {importMutation.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Upload className="size-4" />
            )}
            Import .mrpack
          </button>
          <button
            onClick={() => importCfMutation.mutate()}
            disabled={importCfMutation.isPending}
            className="flex items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-surface disabled:opacity-50"
          >
            {importCfMutation.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Upload className="size-4" />
            )}
            Import CurseForge .zip
          </button>
          <button
            onClick={() => setShowModal(true)}
            className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
          >
            <Plus className="size-4" />
            New instance
          </button>
        </div>
      </header>

      {importMutation.isError && (
        <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          Import failed: {String(importMutation.error)}
        </p>
      )}

      {importCfMutation.isError && (
        <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          Import failed: {String(importCfMutation.error)}
        </p>
      )}

      {isLoading ? (
        <p className="text-sm text-muted">Loading…</p>
      ) : instances && instances.length > 0 ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
          {instances.map((inst) => (
            <InstanceCard key={inst.id} instance={inst} />
          ))}
        </div>
      ) : (
        <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-20 text-center">
          <Server className="mb-4 size-10 text-muted" />
          <h2 className="text-lg font-medium">No instances yet</h2>
          <p className="mt-1 max-w-sm text-sm text-muted">
            Create a fresh instance, or head to{" "}
            <span className="text-foreground">Browse</span> to install a modpack
            from CurseForge or Modrinth.
          </p>
        </div>
      )}

      {showModal && <NewInstanceModal onClose={() => setShowModal(false)} />}

      {importResult && (
        <ImportResultToast result={importResult} onClose={() => setImportResult(null)} />
      )}

      {cfImportResult && (
        <CfImportResultToast result={cfImportResult} onClose={() => setCfImportResult(null)} />
      )}
    </div>
  );
}

function ImportResultToast({
  result,
  onClose,
}: {
  result: MrpackImportResult;
  onClose: () => void;
}) {
  return (
    <div className="fixed bottom-6 right-6 z-50 w-80 rounded-xl border border-border bg-surface p-4 shadow-xl">
      <div className="mb-2 flex items-start justify-between gap-2">
        <p className="font-medium">{result.name} imported</p>
        <button
          onClick={onClose}
          className="shrink-0 rounded-md p-1 text-muted hover:bg-background hover:text-foreground"
        >
          <X className="size-4" />
        </button>
      </div>
      <p className="text-sm text-muted">
        {result.installed} installed · {result.skipped} skipped · {result.failed} failed
      </p>
      {result.failedFiles.length > 0 && (
        <ul className="mt-2 max-h-28 overflow-y-auto text-xs text-danger">
          {result.failedFiles.map((f) => (
            <li key={f} className="truncate">
              {f}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function CfImportResultToast({
  result,
  onClose,
}: {
  result: CfImportResult;
  onClose: () => void;
}) {
  return (
    <div className="fixed bottom-6 right-6 z-50 w-80 rounded-xl border border-border bg-surface p-4 shadow-xl">
      <div className="mb-2 flex items-start justify-between gap-2">
        <p className="font-medium">{result.name} imported</p>
        <button
          onClick={onClose}
          className="shrink-0 rounded-md p-1 text-muted hover:bg-background hover:text-foreground"
        >
          <X className="size-4" />
        </button>
      </div>
      <p className="text-sm text-muted">
        {result.installed} installed · {result.failed} failed
      </p>
      {result.manual.length > 0 && (
        <div className="mt-2">
          <p className="text-xs font-medium text-foreground">
            {result.manual.length} file{result.manual.length === 1 ? "" : "s"} need manual download:
          </p>
          <ul className="mt-1 max-h-28 overflow-y-auto text-xs">
            {result.manual.map((m) => (
              <li key={`${m.projectId}-${m.fileId}`} className="truncate">
                <a
                  href={m.pageUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="text-primary hover:underline"
                >
                  {m.fileName}
                </a>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function InstanceCard({ instance }: { instance: Instance }) {
  const qc = useQueryClient();
  const del = useMutation({
    mutationFn: () => deleteInstance(instance.slug),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["instances"] }),
  });

  const loaderLabel =
    instance.loader.kind === "vanilla"
      ? "Vanilla"
      : instance.loader.kind[0].toUpperCase() + instance.loader.kind.slice(1);

  return (
    <Link
      to={`/instances/${instance.slug}`}
      className="group relative flex flex-col gap-3 rounded-xl border border-border bg-surface p-4 transition-colors hover:border-primary"
    >
      <div className="grid size-12 place-items-center rounded-lg bg-surface-2 text-muted">
        <Box className="size-6" />
      </div>
      <div className="min-w-0">
        <h3 className="truncate font-medium">{instance.name}</h3>
        <p className="text-xs text-muted">
          {loaderLabel} · {instance.minecraft}
        </p>
      </div>

      <button
        title="Delete instance"
        onClick={(e) => {
          e.preventDefault();
          if (confirm(`Delete instance "${instance.name}"? This cannot be undone.`)) {
            del.mutate();
          }
        }}
        className="absolute right-3 top-3 rounded-md p-1.5 text-muted opacity-0 transition-opacity hover:bg-background hover:text-danger group-hover:opacity-100"
      >
        <Trash2 className="size-4" />
      </button>
    </Link>
  );
}
