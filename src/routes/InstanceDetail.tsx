import { useParams, Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, FileBox, Package } from "lucide-react";
import { getInstance, type FolderMod } from "@/lib/ipc";

export function InstanceDetail() {
  const { slug } = useParams();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["instance", slug],
    queryFn: () => getInstance(slug!),
    enabled: !!slug,
  });

  return (
    <div className="px-8 py-7">
      <Link
        to="/instances"
        className="mb-4 inline-flex items-center gap-1.5 text-sm text-muted hover:text-foreground"
      >
        <ArrowLeft className="size-4" />
        Back to instances
      </Link>

      {isLoading ? (
        <p className="text-sm text-muted">Loading…</p>
      ) : isError ? (
        <p className="text-sm text-danger">{String(error)}</p>
      ) : data ? (
        <>
          <header className="mb-6">
            <h1 className="text-2xl font-semibold">{data.instance.name}</h1>
            <p className="text-sm text-muted">
              {labelLoader(data.instance.loader.kind)}
              {data.instance.loader.version ? ` ${data.instance.loader.version}` : ""} ·
              Minecraft {data.instance.minecraft}
            </p>
          </header>

          <dl className="mb-8 grid grid-cols-2 gap-4 sm:grid-cols-4">
            <Stat label="Memory" value={`${data.instance.java.memoryMb} MB`} />
            <Stat
              label="Java"
              value={data.instance.java.major ? `Java ${data.instance.java.major}` : "Auto"}
            />
            <Stat label="Managed mods" value={`${data.instance.mods.length}`} />
            <Stat label="Created" value={formatDate(data.instance.created)} />
          </dl>

          <section>
            <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-muted">
              <Package className="size-4" />
              Mods ({data.folderMods.length})
            </h2>
            {data.folderMods.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border bg-surface/40 px-4 py-8 text-center text-sm text-muted">
                No mods in this instance yet. Adding mods from CurseForge &
                Modrinth arrives in Phase 5.
              </div>
            ) : (
              <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border">
                {data.folderMods.map((m) => (
                  <ModRow key={m.fileName} mod={m} />
                ))}
              </ul>
            )}
          </section>
        </>
      ) : null}
    </div>
  );
}

function ModRow({ mod }: { mod: FolderMod }) {
  return (
    <li className="flex items-center gap-3 bg-surface px-4 py-2.5 text-sm">
      <FileBox className="size-4 shrink-0 text-muted" />
      <span className={`min-w-0 flex-1 truncate ${mod.disabled ? "text-muted line-through" : ""}`}>
        {mod.fileName}
      </span>
      {!mod.managed && (
        <span className="rounded bg-surface-2 px-1.5 py-0.5 text-xs text-muted">
          unmanaged
        </span>
      )}
      <span className="shrink-0 text-xs text-muted">{formatBytes(mod.sizeBytes)}</span>
    </li>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-surface px-4 py-3">
      <dt className="text-xs text-muted">{label}</dt>
      <dd className="mt-0.5 font-medium">{value}</dd>
    </div>
  );
}

function labelLoader(kind: string): string {
  return kind === "vanilla" ? "Vanilla" : kind[0].toUpperCase() + kind.slice(1);
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? "—" : d.toLocaleDateString();
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}
