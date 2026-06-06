import { UserPlus } from "lucide-react";

export function Accounts() {
  return (
    <div className="px-8 py-7">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Accounts</h1>
          <p className="text-sm text-muted">Microsoft / Minecraft accounts</p>
        </div>
        <button className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90">
          <UserPlus className="size-4" />
          Add account
        </button>
      </header>

      <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-20 text-center text-sm text-muted">
        Microsoft device-code sign-in arrives in Phase 3 — see docs/ROADMAP.md
      </div>
    </div>
  );
}
