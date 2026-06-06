import { Outlet } from "react-router-dom";
import { Sidebar } from "@/components/Sidebar";

/** Top-level chrome: fixed sidebar + scrollable content area. */
export function AppShell() {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
