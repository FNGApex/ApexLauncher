import { useState } from "react";
import { Link } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Box, Plus, Server, Trash2 } from "lucide-react";
import {
  deleteInstance,
  listInstances,
  type Instance,
} from "@/lib/ipc";
import { NewInstanceModal } from "@/components/NewInstanceModal";

export function Home() {
  const [showModal, setShowModal] = useState(false);
  const { data: instances, isLoading } = useQuery({
    queryKey: ["instances"],
    queryFn: listInstances,
  });

  return (
    <div className="px-8 py-7">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Instances</h1>
          <p className="text-sm text-muted">Your Minecraft profiles</p>
        </div>
        <button
          onClick={() => setShowModal(true)}
          className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
        >
          <Plus className="size-4" />
          New instance
        </button>
      </header>

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
            Create a fresh instance or import a modpack via{" "}
            <span className="text-foreground">New instance</span>.
          </p>
        </div>
      )}

      {showModal && (
        <NewInstanceModal onClose={() => setShowModal(false)} />
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
