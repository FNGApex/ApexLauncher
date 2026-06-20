import { useEffect, useRef, useState } from "react";
import { NavLink } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  LayoutGrid,
  Compass,
  Settings as Cog,
  Box,
  LogIn,
  LogOut,
  Loader2,
  X,
  Gamepad2,
  ChevronsLeft,
  ChevronsRight,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  events,
  beginLogin,
  cancelLogin,
  getAccount,
  logout,
  type DeviceCodePayload,
} from "@/lib/ipc";
import { DownloadManagerButton } from "@/components/DownloadManager";
import { useAppStore, useUiStore } from "@/lib/store";

export function Sidebar() {
  const qc = useQueryClient();
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);

  const { data: account } = useQuery({
    queryKey: ["account"],
    queryFn: getAccount,
  });

  const [loginState, setLoginState] = useState<"idle" | "pending" | "polling">("idle");
  const [deviceCode, setDeviceCode] = useState<DeviceCodePayload | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);
  const [logoutError, setLogoutError] = useState<string | null>(null);

  const unlistenRef = useRef<(() => void) | null>(null);

  // Clean up device-code listener on unmount.
  useEffect(() => {
    return () => {
      unlistenRef.current?.();
    };
  }, []);

  async function handleLogin() {
    setLoginError(null);
    setDeviceCode(null);
    setLoginState("pending");

    try {
      // Subscribe before invoking beginLogin so we never miss the event.
      const unlisten = await events.authDeviceCode.listen((event) => {
        setDeviceCode(event.payload);
        setLoginState("polling");
      });
      unlistenRef.current = unlisten;

      await beginLogin();
      await qc.invalidateQueries({ queryKey: ["account"] });
    } catch (err) {
      setLoginError(extractMessage(err));
    } finally {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setLoginState("idle");
      setDeviceCode(null);
    }
  }

  async function handleCancel() {
    try {
      await cancelLogin();
    } catch (err) {
      // Cancel failing is usually benign (no login in progress) — the flow
      // resolves or errors on its own.
      console.warn("cancelLogin failed:", err);
    }
    // UI resets via the finally block in handleLogin.
  }

  async function handleLogout() {
    setLogoutError(null);
    try {
      await logout();
      await qc.invalidateQueries({ queryKey: ["account"] });
    } catch (err) {
      setLogoutError(extractMessage(err));
    }
  }

  return (
    <aside
      className={cn(
        "flex h-full shrink-0 flex-col border-r border-border bg-surface transition-[width] duration-200",
        collapsed ? "w-16" : "w-60",
      )}
    >
      {/* Brand row + toggle button */}
      <div
        className={cn(
          "flex items-center px-3 py-4",
          collapsed ? "justify-center" : "gap-2 px-5",
        )}
      >
        {!collapsed && (
          <>
            <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground">
              <Box className="size-5" />
            </div>
            <div className="min-w-0 flex-1 leading-tight">
              <div className="truncate text-sm font-semibold">ApexLauncher</div>
              <div className="text-xs text-muted">Minecraft launcher</div>
            </div>
          </>
        )}
        {collapsed && (
          <div className="grid size-9 place-items-center rounded-lg bg-primary text-primary-foreground">
            <Box className="size-5" />
          </div>
        )}
      </div>

      {/* Toggle button — below brand row, full-width */}
      <button
        onClick={toggleSidebar}
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        className={cn(
          "mx-3 mb-2 flex items-center rounded-lg py-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-foreground",
          collapsed ? "justify-center px-0" : "gap-2 px-2",
        )}
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
      >
        {collapsed ? (
          <ChevronsRight className="size-4 shrink-0" />
        ) : (
          <>
            <ChevronsLeft className="size-4 shrink-0" />
            <span className="text-xs">Collapse</span>
          </>
        )}
      </button>

      {/* Nav */}
      <nav className={cn("flex flex-1 flex-col gap-1", collapsed ? "px-2" : "px-3")}>
        {/* Instances */}
        <NavLink
          to="/instances"
          title={collapsed ? "Instances" : undefined}
          className={({ isActive }) =>
            cn(
              "flex items-center rounded-lg py-2 text-sm font-medium transition-colors",
              "text-muted hover:bg-surface-2 hover:text-foreground",
              isActive && "bg-surface-2 text-foreground",
              collapsed ? "justify-center px-0" : "gap-3 px-3",
            )
          }
        >
          <LayoutGrid className="size-[18px] shrink-0" />
          {!collapsed && "Instances"}
        </NavLink>

        {/* Browse — parent link + sub-items when expanded */}
        <BrowseNav collapsed={collapsed} />

        {/* Settings */}
        <NavLink
          to="/settings"
          title={collapsed ? "Settings" : undefined}
          className={({ isActive }) =>
            cn(
              "flex items-center rounded-lg py-2 text-sm font-medium transition-colors",
              "text-muted hover:bg-surface-2 hover:text-foreground",
              isActive && "bg-surface-2 text-foreground",
              collapsed ? "justify-center px-0" : "gap-3 px-3",
            )
          }
        >
          <Cog className="size-[18px] shrink-0" />
          {!collapsed && "Settings"}
        </NavLink>
      </nav>

      {/* Running indicator */}
      <RunningIndicator collapsed={collapsed} />

      {/* Download Manager trigger */}
      <div className={cn("border-t border-border py-2", collapsed ? "px-2" : "px-3")}>
        <DownloadManagerButton collapsed={collapsed} />
      </div>

      {/* Bottom-left auth control */}
      <div className={cn("border-t border-border py-3", collapsed ? "px-2" : "px-3")}>
        {account ? (
          <LoggedInControl
            username={account.username}
            onLogout={handleLogout}
            logoutError={logoutError}
            collapsed={collapsed}
          />
        ) : (
          <LoggedOutControl
            loginState={loginState}
            deviceCode={deviceCode}
            loginError={loginError}
            onLogin={handleLogin}
            onCancel={handleCancel}
            collapsed={collapsed}
          />
        )}
      </div>

      {/* Version line — hidden when collapsed */}
      {!collapsed && (
        <div className="px-5 py-4 text-xs text-muted">v0.1.0 · pre-alpha</div>
      )}
    </aside>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

/** Browse parent link + sub-provider items (expanded only). */
function BrowseNav({ collapsed }: { collapsed: boolean }) {
  const browseProvider = useUiStore((s) => s.browseProvider);

  return (
    <>
      {/* Browse parent link — goes to last-used provider */}
      <NavLink
        to={`/browse/${browseProvider}`}
        title={collapsed ? "Browse" : undefined}
        className={({ isActive }) =>
          cn(
            "flex items-center rounded-lg py-2 text-sm font-medium transition-colors",
            "text-muted hover:bg-surface-2 hover:text-foreground",
            isActive && "bg-surface-2 text-foreground",
            collapsed ? "justify-center px-0" : "gap-3 px-3",
          )
        }
      >
        <Compass className="size-[18px] shrink-0" />
        {!collapsed && "Browse"}
      </NavLink>

      {/* Sub-items — shown only when expanded */}
      {!collapsed && (
        <div className="ml-6 flex flex-col gap-0.5">
          {/* CurseForge — active link */}
          <NavLink
            to="/browse/curseforge"
            className={({ isActive }) =>
              cn(
                "flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition-colors",
                "text-muted hover:bg-surface-2 hover:text-foreground",
                isActive && "bg-surface-2 text-foreground",
              )
            }
          >
            CurseForge
          </NavLink>

          {/* Modrinth — active link */}
          <NavLink
            to="/browse/modrinth"
            className={({ isActive }) =>
              cn(
                "flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition-colors",
                "text-muted hover:bg-surface-2 hover:text-foreground",
                isActive && "bg-surface-2 text-foreground",
              )
            }
          >
            Modrinth
          </NavLink>

          {/* FTB — coming soon (not a link) */}
          <div
            title="Coming soon"
            className="flex cursor-not-allowed items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium text-muted/40"
          >
            FTB
            <span className="rounded bg-muted/20 px-1 py-0.5 text-[10px] leading-tight">
              Soon
            </span>
          </div>

          {/* ATLauncher — coming soon (not a link) */}
          <div
            title="Coming soon"
            className="flex cursor-not-allowed items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium text-muted/40"
          >
            ATLauncher
            <span className="rounded bg-muted/20 px-1 py-0.5 text-[10px] leading-tight">
              Soon
            </span>
          </div>
        </div>
      )}
    </>
  );
}

interface LoggedInControlProps {
  username: string;
  onLogout: () => void;
  logoutError: string | null;
  collapsed: boolean;
}

function LoggedInControl({ username, onLogout, logoutError, collapsed }: LoggedInControlProps) {
  if (collapsed) {
    return (
      <button
        onClick={onLogout}
        title={`${username} — click to log out`}
        className="flex w-full items-center justify-center rounded-lg py-2 text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
        aria-label={`${username} — click to log out`}
      >
        <div className="size-6 grid place-items-center rounded-full bg-accent/20 text-xs font-semibold text-accent">
          {username.charAt(0).toUpperCase()}
        </div>
      </button>
    );
  }

  return (
    <div>
      <div className="mb-2 flex items-center gap-2 rounded-lg px-2 py-1.5">
        <div className="size-7 shrink-0 grid place-items-center rounded-full bg-accent/20 text-xs font-semibold text-accent">
          {username.charAt(0).toUpperCase()}
        </div>
        <span className="min-w-0 flex-1 truncate text-sm font-medium">{username}</span>
      </div>
      {logoutError && (
        <p className="mb-2 rounded px-2 py-1 text-xs text-danger">{logoutError}</p>
      )}
      <button
        onClick={onLogout}
        className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-muted hover:bg-surface-2 hover:text-foreground transition-colors"
      >
        <LogOut className="size-4 shrink-0" />
        Log out
      </button>
    </div>
  );
}

interface LoggedOutControlProps {
  loginState: "idle" | "pending" | "polling";
  deviceCode: DeviceCodePayload | null;
  loginError: string | null;
  onLogin: () => void;
  onCancel: () => void;
  collapsed: boolean;
}

function LoggedOutControl({
  loginState,
  deviceCode,
  loginError,
  onLogin,
  onCancel,
  collapsed,
}: LoggedOutControlProps) {
  const inLoginFlow = loginState !== "idle";

  if (collapsed) {
    // In collapsed mode: just show a sign-in icon. If a flow is in progress,
    // user can expand the sidebar to see the device-code UI.
    return (
      <button
        onClick={inLoginFlow ? onCancel : onLogin}
        disabled={false}
        title={inLoginFlow ? "Sign-in in progress — click to cancel" : "Sign in with Microsoft"}
        className="flex w-full items-center justify-center rounded-lg py-2 text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
        aria-label={inLoginFlow ? "Cancel sign-in" : "Sign in with Microsoft"}
      >
        {loginState === "pending" ? (
          <Loader2 className="size-4 shrink-0 animate-spin" />
        ) : (
          <LogIn className="size-4 shrink-0" />
        )}
      </button>
    );
  }

  return (
    <div>
      {/* Device-code instructions — shown once we receive the code event */}
      {inLoginFlow && deviceCode && (
        <div className="mb-2 rounded-lg border border-border bg-surface p-3 text-xs">
          <p className="mb-2 font-medium">Sign in with Microsoft</p>
          <p className="mb-2 text-muted">Open the link and enter the code:</p>
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <a
              href={deviceCode.verificationUri}
              target="_blank"
              rel="noreferrer"
              className="rounded bg-accent px-2 py-1 text-white hover:bg-accent/80"
            >
              microsoft.com/link
            </a>
            <span className="font-mono font-semibold tracking-widest">{deviceCode.userCode}</span>
          </div>
          <p className="mb-2 text-muted">Waiting for sign-in…</p>
          <button
            onClick={onCancel}
            className="flex items-center gap-1 text-danger hover:underline"
          >
            <X className="size-3" />
            Cancel
          </button>
        </div>
      )}

      {/* Waiting for device-code (pending but no code yet) */}
      {loginState === "pending" && !deviceCode && (
        <div className="mb-2 flex items-center gap-2 rounded-lg border border-border px-3 py-2 text-xs text-muted">
          <Loader2 className="size-3.5 animate-spin shrink-0" />
          <span className="flex-1">Requesting code…</span>
          <button onClick={onCancel} className="text-danger hover:underline">
            <X className="size-3.5" />
          </button>
        </div>
      )}

      {loginError && (
        <p className="mb-2 rounded px-2 py-1 text-xs text-danger">{loginError}</p>
      )}

      <button
        onClick={onLogin}
        disabled={inLoginFlow}
        className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-muted hover:bg-surface-2 hover:text-foreground transition-colors disabled:opacity-50"
      >
        {loginState === "pending" ? (
          <Loader2 className="size-4 shrink-0 animate-spin" />
        ) : (
          <LogIn className="size-4 shrink-0" />
        )}
        {inLoginFlow ? "Signing in…" : "Sign in with Microsoft"}
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Running indicator — shows a live count of active (preparing/running) instances
// ---------------------------------------------------------------------------

function RunningIndicator({ collapsed }: { collapsed: boolean }) {
  const runs = useAppStore((s) => s.runs);
  const activeCount = [...runs.values()].filter(
    (r) => r.status === "preparing" || r.status === "running",
  ).length;

  if (activeCount === 0) return null;

  if (collapsed) {
    return (
      <div className="px-2 pb-1">
        <div
          title={`${activeCount} running`}
          className="flex items-center justify-center rounded-lg border border-green-500/30 bg-green-500/10 py-2 text-green-400"
        >
          <Gamepad2 className="size-3.5 shrink-0" />
        </div>
      </div>
    );
  }

  return (
    <div className="px-3 pb-1">
      <div className="flex items-center gap-2 rounded-lg border border-green-500/30 bg-green-500/10 px-3 py-2 text-xs text-green-400">
        <Gamepad2 className="size-3.5 shrink-0" />
        <span className="flex items-center gap-1.5">
          <span className="size-1.5 animate-pulse rounded-full bg-green-400" />
          {activeCount} running
        </span>
      </div>
    </div>
  );
}

/** Extract a human-readable message from a Tauri command error. */
function extractMessage(err: unknown): string {
  if (!err || typeof err !== "object") return String(err);
  if (!("message" in err)) return String(err);
  const { message } = err;
  if (typeof message !== "string") return String(err);
  const kind = "kind" in err ? err.kind : undefined;
  return typeof kind === "string" ? `[${kind}] ${message}` : message;
}
