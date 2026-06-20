import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus, Search, Server, X } from "lucide-react";
import { listInstances, type Instance } from "@/lib/ipc";
import { NewInstanceModal } from "@/components/NewInstanceModal";
import { InstanceCard } from "@/components/InstanceCard";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEBOUNCE_MS = 300;

type SortKey = "lastPlayed" | "name" | "created" | "playtime";

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

export function Home() {
  const [showModal, setShowModal] = useState(false);
  const [rawQuery, setRawQuery] = useState("");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("lastPlayed");

  const { data: instances, isLoading } = useQuery({
    queryKey: ["instances"],
    queryFn: listInstances,
  });

  // Debounce search input (mirror Browse.tsx pattern).
  useEffect(() => {
    const id = setTimeout(() => setQuery(rawQuery.trim()), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [rawQuery]);

  // Derive: filter then sort.
  const filtered = deriveList(instances ?? [], query, sort);

  return (
    <div className="px-8 py-7">
      {/* Header row */}
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

      {/* Controls row: search + sort */}
      <div className="mb-5 flex items-center gap-3">
        {/* Search field — styled like Browse.tsx */}
        <div className="flex min-w-0 flex-1 items-center gap-3 rounded-xl border border-border bg-surface px-4 py-2.5 transition-colors focus-within:border-primary">
          <Search className="size-5 shrink-0 text-muted" />
          <input
            className="w-full bg-transparent text-base outline-none placeholder:text-muted"
            placeholder="Search instances…"
            value={rawQuery}
            onChange={(e) => setRawQuery(e.target.value)}
          />
          {rawQuery.length > 0 && (
            <button
              onClick={() => setRawQuery("")}
              className="shrink-0 rounded-md p-0.5 text-muted hover:text-foreground"
              title="Clear search"
            >
              <X className="size-4" />
            </button>
          )}
        </div>

        {/* Sort select */}
        <select
          value={sort}
          onChange={(e) => setSort(e.target.value as SortKey)}
          className="rounded-xl border border-border bg-surface px-3 py-2.5 text-sm text-foreground outline-none transition-colors hover:border-primary/50 focus:border-primary"
        >
          <option value="lastPlayed">Last played</option>
          <option value="name">Name (A–Z)</option>
          <option value="created">Newest first</option>
          <option value="playtime">Most played</option>
        </select>
      </div>

      {/* Content */}
      {isLoading ? (
        <p className="text-sm text-muted">Loading…</p>
      ) : instances && instances.length > 0 ? (
        filtered.length > 0 ? (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {filtered.map((inst) => (
              <InstanceCard key={inst.id} instance={inst} />
            ))}
          </div>
        ) : (
          /* No-match state */
          <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-16 text-center">
            <Search className="mb-4 size-8 text-muted" />
            <h2 className="text-base font-medium">No instances match</h2>
            <p className="mt-1 text-sm text-muted">
              Try a different search term or{" "}
              <button
                className="text-foreground underline hover:no-underline"
                onClick={() => setRawQuery("")}
              >
                clear the filter
              </button>
              .
            </p>
          </div>
        )
      ) : (
        /* Empty state */
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

// ---------------------------------------------------------------------------
// Derive: filter then sort
// ---------------------------------------------------------------------------

function deriveList(instances: Instance[], query: string, sort: SortKey): Instance[] {
  const lower = query.toLowerCase();
  const filtered = lower
    ? instances.filter((i) => i.name.toLowerCase().includes(lower))
    : instances;

  return [...filtered].sort((a, b) => {
    switch (sort) {
      case "name":
        return a.name.localeCompare(b.name);
      case "created":
        return new Date(b.created).getTime() - new Date(a.created).getTime();
      case "playtime":
        return b.totalPlaytimeSec - a.totalPlaytimeSec;
      case "lastPlayed":
      default: {
        // null lastPlayed sorts last
        if (a.lastPlayed === null && b.lastPlayed === null) return 0;
        if (a.lastPlayed === null) return 1;
        if (b.lastPlayed === null) return -1;
        return new Date(b.lastPlayed).getTime() - new Date(a.lastPlayed).getTime();
      }
    }
  });
}
