import { useState } from "react";
import { Search } from "lucide-react";

type Provider = "all" | "modrinth" | "curseforge";

const TABS: { id: Provider; label: string }[] = [
  { id: "all", label: "All" },
  { id: "modrinth", label: "Modrinth" },
  { id: "curseforge", label: "CurseForge" },
];

export function Browse() {
  const [provider, setProvider] = useState<Provider>("all");

  return (
    <div className="px-8 py-7">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold">Browse</h1>
        <p className="text-sm text-muted">
          Search modpacks and mods across providers
        </p>
      </header>

      <div className="mb-5 flex items-center gap-3">
        <div className="flex flex-1 items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2">
          <Search className="size-4 text-muted" />
          <input
            className="w-full bg-transparent text-sm outline-none placeholder:text-muted"
            placeholder="Search modpacks…  (wired up in Phase 5/6)"
          />
        </div>
        <div className="flex gap-1 rounded-lg border border-border bg-surface p-1">
          {TABS.map((t) => (
            <button
              key={t.id}
              onClick={() => setProvider(t.id)}
              className={
                "rounded-md px-3 py-1.5 text-sm font-medium transition-colors " +
                (provider === t.id
                  ? "bg-surface-2 text-foreground"
                  : "text-muted hover:text-foreground")
              }
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>

      <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-20 text-center text-sm text-muted">
        Provider search ({provider}) arrives in Phase 5 — see docs/ROADMAP.md
      </div>
    </div>
  );
}
