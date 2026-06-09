import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Loader2, LogIn, Star, Trash2, UserPlus, X } from "lucide-react";
import {
  beginLogin,
  cancelLogin,
  listAccounts,
  listenDeviceCode,
  removeAccount,
  setActiveAccount,
  type AccountMeta,
  type DeviceCodePayload,
} from "@/lib/ipc";

export function Accounts() {
  const qc = useQueryClient();

  const { data: accounts, isLoading, isError, error } = useQuery({
    queryKey: ["accounts"],
    queryFn: listAccounts,
  });

  // The active account id is not returned by listAccounts — track locally.
  // Seeded from setActiveAccount calls and from beginLogin (new account is always active).
  const [activeId, setActiveId] = useState<string | null>(null);

  // Device-code flow state.
  const [loginState, setLoginState] = useState<"idle" | "pending" | "polling">("idle");
  const [deviceCode, setDeviceCode] = useState<DeviceCodePayload | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);

  // Unlisten ref so we can clean up the device-code event listener.
  const unlistenRef = useRef<(() => void) | null>(null);

  // Clean up device-code listener on unmount.
  useEffect(() => {
    return () => {
      unlistenRef.current?.();
    };
  }, []);

  async function handleAddAccount() {
    setLoginError(null);
    setDeviceCode(null);
    setLoginState("pending");

    // Subscribe to auth://device-code before invoking beginLogin so we never miss the event.
    const unlisten = await listenDeviceCode((payload) => {
      setDeviceCode(payload);
      setLoginState("polling");
    });
    unlistenRef.current = unlisten;

    try {
      const meta = await beginLogin();
      // New account is always the active one after login.
      setActiveId(meta.id);
      await qc.invalidateQueries({ queryKey: ["accounts"] });
    } catch (err) {
      // Tauri serializes AuthCommandError as the error payload.
      setLoginError(extractMessage(err));
    } finally {
      unlisten();
      unlistenRef.current = null;
      setLoginState("idle");
      setDeviceCode(null);
    }
  }

  async function handleCancel() {
    try {
      await cancelLogin();
    } catch {
      // If cancel fails (e.g. no login in progress), ignore — the flow will
      // resolve or error on its own.
    }
    // UI resets via the finally block in handleAddAccount.
  }

  const removeMutation = useMutation({
    mutationFn: (id: string) => removeAccount(id),
    onSuccess: (_data, id) => {
      if (activeId === id) setActiveId(null);
      qc.invalidateQueries({ queryKey: ["accounts"] });
    },
  });

  const activateMutation = useMutation({
    mutationFn: (id: string) => setActiveAccount(id),
    onSuccess: (_data, id) => {
      setActiveId(id);
    },
  });

  const inLoginFlow = loginState !== "idle";

  return (
    <div className="px-8 py-7">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Accounts</h1>
          <p className="text-sm text-muted">Microsoft / Minecraft accounts</p>
        </div>
        <button
          onClick={handleAddAccount}
          disabled={inLoginFlow}
          className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
        >
          {loginState === "pending" ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <UserPlus className="size-4" />
          )}
          {loginState === "pending" ? "Connecting…" : "Add account"}
        </button>
      </header>

      {/* Device-code flow panel */}
      {inLoginFlow && deviceCode && (
        <div className="mb-6 rounded-xl border border-border bg-surface p-5">
          <div className="mb-3 flex items-center gap-2 text-sm font-medium">
            <LogIn className="size-4 text-accent" />
            Sign in with Microsoft
          </div>
          <p className="mb-4 text-sm text-muted">
            Open the link below and enter the code to complete sign-in.
          </p>
          <div className="mb-4 flex flex-wrap items-center gap-4">
            <a
              href={deviceCode.verificationUri}
              target="_blank"
              rel="noreferrer"
              className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent/80"
            >
              Open microsoft.com/link
            </a>
            <span className="font-mono text-lg font-semibold tracking-widest">
              {deviceCode.userCode}
            </span>
          </div>
          <p className="mb-4 text-xs text-muted">
            Waiting for sign-in to complete…
          </p>
          <button
            onClick={handleCancel}
            className="flex items-center gap-1.5 text-sm text-danger hover:underline"
          >
            <X className="size-4" />
            Cancel
          </button>
        </div>
      )}

      {/* Waiting for device-code event (pending but no code yet) */}
      {loginState === "pending" && !deviceCode && (
        <div className="mb-6 flex items-center gap-3 rounded-xl border border-border bg-surface px-5 py-4 text-sm text-muted">
          <Loader2 className="size-4 animate-spin shrink-0" />
          Requesting device code from Microsoft…
          <button
            onClick={handleCancel}
            className="ml-auto flex items-center gap-1.5 text-danger hover:underline"
          >
            <X className="size-4" />
            Cancel
          </button>
        </div>
      )}

      {/* Error banner */}
      {loginError && (
        <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-4 py-2 text-sm text-danger">
          {loginError}
        </p>
      )}

      {/* Mutation errors */}
      {removeMutation.isError && (
        <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-4 py-2 text-sm text-danger">
          {extractMessage(removeMutation.error)}
        </p>
      )}
      {activateMutation.isError && (
        <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-4 py-2 text-sm text-danger">
          {extractMessage(activateMutation.error)}
        </p>
      )}

      {/* Account list */}
      {isLoading ? (
        <p className="text-sm text-muted">Loading…</p>
      ) : isError ? (
        <p className="text-sm text-danger">{String(error)}</p>
      ) : !accounts || accounts.length === 0 ? (
        <div className="grid place-items-center rounded-xl border border-dashed border-border bg-surface/40 py-20 text-center text-sm text-muted">
          No accounts added. Click &ldquo;Add account&rdquo; to sign in with Microsoft.
        </div>
      ) : (
        <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border">
          {accounts.map((account) => (
            <AccountRow
              key={account.id}
              account={account}
              isActive={activeId === account.id}
              onSetActive={() => activateMutation.mutate(account.id)}
              onRemove={() => removeMutation.mutate(account.id)}
              isActivating={
                activateMutation.isPending && activateMutation.variables === account.id
              }
              isRemoving={
                removeMutation.isPending && removeMutation.variables === account.id
              }
            />
          ))}
        </ul>
      )}
    </div>
  );
}

interface AccountRowProps {
  account: AccountMeta;
  isActive: boolean;
  onSetActive: () => void;
  onRemove: () => void;
  isActivating: boolean;
  isRemoving: boolean;
}

function AccountRow({
  account,
  isActive,
  onSetActive,
  onRemove,
  isActivating,
  isRemoving,
}: AccountRowProps) {
  return (
    <li className="flex items-center gap-3 bg-surface px-5 py-3">
      {/* Active indicator */}
      <span className="size-2 shrink-0 rounded-full" style={{ background: isActive ? "var(--color-accent, #22c55e)" : "var(--color-border, #333)" }} />

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-sm">{account.username}</span>
          {isActive && (
            <span className="rounded bg-accent/15 px-1.5 py-0.5 text-xs text-accent">
              Active
            </span>
          )}
        </div>
        <span className="text-xs text-muted font-mono truncate block">{account.id}</span>
      </div>

      <div className="flex shrink-0 items-center gap-2">
        {!isActive && (
          <button
            onClick={onSetActive}
            disabled={isActivating}
            title="Set as active account"
            className="flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs font-medium hover:bg-surface-2 disabled:opacity-50"
          >
            {isActivating ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <Star className="size-3.5" />
            )}
            Set active
          </button>
        )}
        {isActive && (
          <span className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs text-accent">
            <Check className="size-3.5" />
            Active
          </span>
        )}
        <button
          onClick={onRemove}
          disabled={isRemoving}
          title="Remove account"
          className="flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs font-medium text-danger hover:bg-danger/10 disabled:opacity-50"
        >
          {isRemoving ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Trash2 className="size-3.5" />
          )}
          Remove
        </button>
      </div>
    </li>
  );
}

/** Extract a human-readable message from a Tauri command error. */
function extractMessage(err: unknown): string {
  if (err && typeof err === "object") {
    const e = err as Record<string, unknown>;
    if (typeof e.message === "string") return e.message;
    if (typeof e.kind === "string" && typeof e.message === "string") {
      return `[${e.kind}] ${e.message}`;
    }
  }
  return String(err);
}
