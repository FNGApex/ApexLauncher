import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, X } from "lucide-react";
import {
  createInstance,
  getLoaders,
  listMinecraftVersions,
  type LoaderKind,
} from "@/lib/ipc";
import { META_STALE_TIME } from "@/lib/query";

const LOADER_LABELS: Record<LoaderKind, string> = {
  vanilla: "Vanilla",
  fabric: "Fabric",
  quilt: "Quilt",
  neoforge: "NeoForge",
  forge: "Forge",
};

/** Create-instance dialog. MC version and loader builds are fetched live from
 *  Mojang/Forge/Fabric/Quilt/NeoForge metadata; the loader list is filtered to
 *  what each MC version actually supports. */
export function NewInstanceModal({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [minecraft, setMinecraft] = useState("");
  const [loaderKind, setLoaderKind] = useState<LoaderKind>("vanilla");
  const [loaderVersion, setLoaderVersion] = useState("");

  const mcQuery = useQuery({
    queryKey: ["mc-versions"],
    queryFn: listMinecraftVersions,
    staleTime: META_STALE_TIME,
  });

  // Default to the newest release once versions load.
  useEffect(() => {
    if (mcQuery.data && mcQuery.data.length > 0 && !minecraft) {
      setMinecraft(mcQuery.data[0].id);
    }
  }, [mcQuery.data, minecraft]);

  const loadersQuery = useQuery({
    queryKey: ["loaders", minecraft],
    queryFn: () => getLoaders(minecraft),
    enabled: !!minecraft,
    staleTime: META_STALE_TIME,
  });
  const loaders = loadersQuery.data;

  // When the available loaders change (new MC), keep the selection valid and
  // reset the build to that loader's newest.
  useEffect(() => {
    if (!loaders) return;
    const current = loaders.find((l) => l.kind === loaderKind) ?? loaders[0];
    if (current.kind !== loaderKind) setLoaderKind(current.kind);
    const next = current.kind === "vanilla" ? "" : current.versions[0] ?? "";
    setLoaderVersion((prev) =>
      current.versions.includes(prev) && current.kind !== "vanilla" ? prev : next,
    );
  }, [loaders, loaderKind]);

  const selected = loaders?.find((l) => l.kind === loaderKind);

  const mutation = useMutation({
    mutationFn: () =>
      createInstance({
        name,
        minecraft,
        loader: {
          kind: loaderKind,
          version: loaderKind === "vanilla" ? null : loaderVersion || null,
        },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["instances"] });
      onClose();
    },
  });

  const needsBuild = loaderKind !== "vanilla";
  const canSubmit =
    name.trim() !== "" &&
    minecraft !== "" &&
    (!needsBuild || loaderVersion !== "") &&
    !mutation.isPending;

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-xl border border-border bg-surface p-6 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="mb-5 flex items-center justify-between">
          <h2 className="text-lg font-semibold">New instance</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted hover:bg-background hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>

        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (canSubmit) mutation.mutate();
          }}
        >
          <Field label="Name">
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="My instance"
              className="input"
            />
          </Field>

          <Field
            label={
              mcQuery.isLoading ? "Minecraft version (loading…)" : "Minecraft version"
            }
          >
            <select
              value={minecraft}
              onChange={(e) => setMinecraft(e.target.value)}
              disabled={mcQuery.isLoading || !mcQuery.data}
              className="input"
            >
              {mcQuery.data?.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.id}
                </option>
              ))}
            </select>
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Mod loader">
              <select
                value={loaderKind}
                onChange={(e) => setLoaderKind(e.target.value as LoaderKind)}
                disabled={loadersQuery.isFetching || !loaders}
                className="input"
              >
                {loaders?.map((l) => (
                  <option key={l.kind} value={l.kind}>
                    {LOADER_LABELS[l.kind]}
                  </option>
                ))}
              </select>
            </Field>
            <Field
              label={
                needsBuild && selected
                  ? `Loader build (${selected.versions.length})`
                  : "Loader build"
              }
            >
              <select
                value={loaderVersion}
                onChange={(e) => setLoaderVersion(e.target.value)}
                disabled={!needsBuild || loadersQuery.isFetching}
                className="input disabled:opacity-40"
              >
                {!needsBuild ? (
                  <option value="">—</option>
                ) : (
                  selected?.versions.map((v) => (
                    <option key={v} value={v}>
                      {v}
                    </option>
                  ))
                )}
              </select>
            </Field>
          </div>

          {loadersQuery.isError && (
            <p className="text-sm text-danger">
              Couldn’t load loader versions: {String(loadersQuery.error)}
            </p>
          )}
          {mutation.isError && (
            <p className="text-sm text-danger">{String(mutation.error)}</p>
          )}

          <div className="flex justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg px-4 py-2 text-sm font-medium text-muted hover:bg-background hover:text-foreground"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!canSubmit}
              className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
            >
              {mutation.isPending && <Loader2 className="size-4 animate-spin" />}
              Create
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-xs font-medium text-muted">{label}</span>
      {children}
    </label>
  );
}
