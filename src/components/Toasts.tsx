import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { X, CheckCircle } from "lucide-react";
import { useAppStore } from "@/lib/store";
import type { TaskResult } from "@/lib/store";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Extract an instance slug from a terminal task result, if the result
 *  represents a newly created or updated instance that can be opened.
 *
 *  Variants with a slug: MrpackImportResult, CfImportResult,
 *  ModpackInstallResult{kind:"mrpack"|"curseforge"}.
 *  The "manual" variant of ModpackInstallResult has no slug (no instance created),
 *  PackUpdateResult/AddModResult/UpdateModResult also have no slug field. */
function extractSlug(result: TaskResult): string | null {
  if ("slug" in result && typeof result.slug === "string") {
    return result.slug;
  }
  return null;
}

/** Derive a human-readable label from a terminal task result. */
function labelFor(result: TaskResult): string {
  if ("name" in result && typeof result.name === "string") {
    return `"${result.name}" installed`;
  }
  // PackUpdateResult: added/removed/kept counts.
  if ("added" in result && "kept" in result) {
    const r = result as { added: number; removed: number; kept: number };
    return `Pack updated (+${r.added} −${r.removed} =${r.kept})`;
  }
  return "Task complete";
}

// ---------------------------------------------------------------------------
// Single toast entry
// ---------------------------------------------------------------------------

interface ToastEntry {
  id: number;
  label: string;
  slug: string | null;
}

// ---------------------------------------------------------------------------
// Toast surface — mount once in AppShell
// ---------------------------------------------------------------------------

export function Toasts() {
  const tasks = useAppStore((s) => s.tasks);
  // Track which task ids have already produced a toast so we show each once.
  const shownRef = useRef<Set<number>>(new Set());
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  const navigate = useNavigate();

  useEffect(() => {
    const newToasts: ToastEntry[] = [];

    for (const [id, task] of tasks) {
      if (task.status.kind !== "done") continue;
      if (!task.result) continue;
      if (shownRef.current.has(id)) continue;

      shownRef.current.add(id);
      newToasts.push({
        id,
        label: labelFor(task.result),
        slug: extractSlug(task.result),
      });
    }

    if (newToasts.length > 0) {
      setToasts((prev) => [...prev, ...newToasts]);
    }
  }, [tasks]);

  function dismiss(id: number) {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className="flex items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3 shadow-lg text-sm"
        >
          <CheckCircle className="size-4 shrink-0 text-green-400" />
          <span className="min-w-0 flex-1 text-foreground">{toast.label}</span>
          {toast.slug != null && (
            <button
              onClick={() => {
                navigate(`/instances/${toast.slug}`);
                dismiss(toast.id);
              }}
              className="shrink-0 rounded-lg bg-accent px-3 py-1 text-xs font-medium text-white hover:bg-accent/80"
            >
              Open
            </button>
          )}
          <button
            onClick={() => dismiss(toast.id)}
            className="shrink-0 rounded-md p-0.5 text-muted hover:bg-surface-2 hover:text-foreground"
            title="Dismiss"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
