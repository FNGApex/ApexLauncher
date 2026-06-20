import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { X, CheckCircle, AlertTriangle, AlertCircle, ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppStore } from "@/lib/store";
import type { TaskResult } from "@/lib/store";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Extract an instance slug from a terminal task result, if the result
 *  represents a newly created or updated instance that can be opened. */
function extractSlug(result: TaskResult): string | null {
  if ("slug" in result && typeof result.slug === "string") {
    return result.slug;
  }
  return null;
}

/** Derive a human-readable label from a clean (no failures) task result. */
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

/** Length of a result field when it is an array (e.g. AddModResult.added /
 *  .failed / .manual are arrays; PackUpdateResult.added is a number → 0 here). */
function arrLen(result: TaskResult, key: string): number {
  const v = (result as Record<string, unknown>)[key];
  return Array.isArray(v) ? v.length : 0;
}

/** Collect `pageUrl`s from a result's `manual` array (CF allowModDistribution:false). */
function manualPageUrls(result: TaskResult): string[] {
  const v = (result as Record<string, unknown>)["manual"];
  if (!Array.isArray(v)) return [];
  return v
    .map((m) => (m as Record<string, unknown>)["pageUrl"])
    .filter((u): u is string => typeof u === "string");
}

type ToastKind = "success" | "partial" | "failed";

interface ToastEntry {
  id: number;
  kind: ToastKind;
  label: string;
  slug: string | null;
  /** Project pages to open for CF distribution-disabled mods. */
  manualUrls: string[];
}

/** Summarize a terminal (done) result into a toast: clean success, or a partial
 *  outcome when some files failed or require a manual download. */
function summarizeDone(id: number, result: TaskResult): ToastEntry {
  const slug = extractSlug(result);
  const failed = arrLen(result, "failed");
  const manual = arrLen(result, "manual");

  if (failed === 0 && manual === 0) {
    return { id, kind: "success", label: labelFor(result), slug, manualUrls: [] };
  }

  // Partial: some installed, some failed and/or need manual download.
  const added = arrLen(result, "added");
  const parts: string[] = [];
  if (added > 0) parts.push(`${added} installed`);
  if (failed > 0) parts.push(`${failed} failed`);
  if (manual > 0) parts.push(`${manual} need manual download`);
  return {
    id,
    kind: "partial",
    label: parts.join(" · ") || "Completed with issues",
    slug,
    manualUrls: manualPageUrls(result),
  };
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
      if (shownRef.current.has(id)) continue;

      if (task.status.kind === "failed") {
        // F1-1 makes installs/imports fail honestly when nothing was installed.
        shownRef.current.add(id);
        newToasts.push({
          id,
          kind: "failed",
          label: task.status.message || "Task failed",
          slug: null,
          manualUrls: [],
        });
      } else if (task.status.kind === "done" && task.result) {
        shownRef.current.add(id);
        newToasts.push(summarizeDone(id, task.result));
      }
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
          {toast.kind === "failed" ? (
            <AlertCircle className="size-4 shrink-0 text-danger" />
          ) : toast.kind === "partial" ? (
            <AlertTriangle className="size-4 shrink-0 text-amber-400" />
          ) : (
            <CheckCircle className="size-4 shrink-0 text-green-400" />
          )}
          <span className="min-w-0 flex-1 text-foreground">{toast.label}</span>

          {/* Open project page(s) for CF distribution-disabled mods. */}
          {toast.manualUrls.length > 0 && (
            <button
              onClick={() => {
                for (const url of toast.manualUrls) openUrl(url).catch(console.error);
                dismiss(toast.id);
              }}
              className="inline-flex shrink-0 items-center gap-1 rounded-lg border border-border bg-surface px-3 py-1 text-xs font-medium text-foreground hover:bg-surface-2"
            >
              <ExternalLink className="size-3.5" />
              Open page{toast.manualUrls.length > 1 ? `s (${toast.manualUrls.length})` : ""}
            </button>
          )}

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
