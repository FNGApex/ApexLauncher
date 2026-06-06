import { useQuery } from "@tanstack/react-query";
import { Plus, Server, Wifi, WifiOff } from "lucide-react";
import { getAppInfo } from "@/lib/ipc";

export function Home() {
  // The one real Phase-0 round-trip: proves the React <-> Rust bridge works.
  const { data, isLoading, isError } = useQuery({
    queryKey: ["app-info"],
    queryFn: getAppInfo,
  });

  return (
    <div className="px-8 py-7">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Instances</h1>
          <p className="text-sm text-muted">Your Minecraft profiles</p>
        </div>
        <button className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90">
          <Plus className="size-4" />
          New instance
        </button>
      </header>

      {/* Empty state — instance management lands in Phase 1. */}
      <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-20 text-center">
        <Server className="mb-4 size-10 text-muted" />
        <h2 className="text-lg font-medium">No instances yet</h2>
        <p className="mt-1 max-w-sm text-sm text-muted">
          Create a fresh instance, or head to{" "}
          <span className="text-foreground">Browse</span> to install a modpack
          from CurseForge or Modrinth.
        </p>
      </div>

      {/* IPC status pill — temporary Phase-0 proof; remove once instances exist. */}
      <div className="mt-6 inline-flex items-center gap-2 rounded-full border border-border bg-surface px-3 py-1.5 text-xs">
        {isLoading ? (
          <span className="text-muted">Connecting to backend…</span>
        ) : isError ? (
          <>
            <WifiOff className="size-3.5 text-danger" />
            <span className="text-danger">Backend unreachable</span>
          </>
        ) : (
          <>
            <Wifi className="size-3.5 text-success" />
            <span className="text-muted">
              Connected · {data?.name} v{data?.version} · Tauri{" "}
              {data?.tauriVersion}
            </span>
          </>
        )}
      </div>
    </div>
  );
}
