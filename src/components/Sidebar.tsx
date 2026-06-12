import { NavLink } from "react-router-dom";
import { LayoutGrid, Compass, User, Settings as Cog, Box } from "lucide-react";
import { cn } from "@/lib/utils";

const NAV = [
  { to: "/instances", label: "Instances", icon: LayoutGrid },
  { to: "/browse", label: "Browse", icon: Compass },
  { to: "/accounts", label: "Accounts", icon: User },
  { to: "/settings", label: "Settings", icon: Cog },
];

export function Sidebar() {
  return (
    <aside className="flex h-full w-60 shrink-0 flex-col border-r border-border bg-surface">
      <div className="flex items-center gap-2 px-5 py-5">
        <div className="grid size-9 place-items-center rounded-lg bg-primary text-primary-foreground">
          <Box className="size-5" />
        </div>
        <div className="leading-tight">
          <div className="text-sm font-semibold">ApexLauncher</div>
          <div className="text-xs text-muted">Minecraft launcher</div>
        </div>
      </div>

      <nav className="flex flex-1 flex-col gap-1 px-3">
        {NAV.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                "text-muted hover:bg-surface-2 hover:text-foreground",
                isActive && "bg-surface-2 text-foreground",
              )
            }
          >
            <Icon className="size-[18px]" />
            {label}
          </NavLink>
        ))}
      </nav>

      <div className="px-5 py-4 text-xs text-muted">v0.1.0 · pre-alpha</div>
    </aside>
  );
}
