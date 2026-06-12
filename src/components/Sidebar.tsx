import { useEffect, useRef, useState } from "react";
import { NavLink } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LayoutGrid, Compass, Settings as Cog, Box, LogIn, LogOut, Loader2, X } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  beginLogin,
  cancelLogin,
  getAccount,
  logout,
  listenDeviceCode,
  type DeviceCodePayload,
} from "@/lib/ipc";

const NAV = [
  { to: "/instances", label: "Instances", icon: LayoutGrid },
  { to: "/browse", label: "Browse", icon: Compass },
  { to: "/settings", label: "Settings", icon: Cog },
];

export function Sidebar() {
  const qc = useQueryClient();

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
      const unlisten = await listenDeviceCode((payload) => {
        setDeviceCode(payload);
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

      {/* Bottom-left auth control */}
      <div className="border-t border-border px-3 py-3">
        {account ? (
          <LoggedInControl
            username={account.username}
            onLogout={handleLogout}
            logoutError={logoutError}
          />
        ) : (
          <LoggedOutControl
            loginState={loginState}
            deviceCode={deviceCode}
            loginError={loginError}
            onLogin={handleLogin}
            onCancel={handleCancel}
          />
        )}
      </div>

      <div className="px-5 py-4 text-xs text-muted">v0.1.0 · pre-alpha</div>
    </aside>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

interface LoggedInControlProps {
  username: string;
  onLogout: () => void;
  logoutError: string | null;
}

function LoggedInControl({ username, onLogout, logoutError }: LoggedInControlProps) {
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
}

function LoggedOutControl({
  loginState,
  deviceCode,
  loginError,
  onLogin,
  onCancel,
}: LoggedOutControlProps) {
  const inLoginFlow = loginState !== "idle";

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

/** Extract a human-readable message from a Tauri command error. */
function extractMessage(err: unknown): string {
  if (!err || typeof err !== "object") return String(err);
  if (!("message" in err)) return String(err);
  const { message } = err;
  if (typeof message !== "string") return String(err);
  const kind = "kind" in err ? err.kind : undefined;
  return typeof kind === "string" ? `[${kind}] ${message}` : message;
}
