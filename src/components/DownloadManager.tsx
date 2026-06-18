/**
 * Download Manager panel — reads entirely from the Zustand tasks slice.
 * No component-local operation state; every update arrives via store events.
 */

import { useState } from "react";
import { X, Download, XCircle, CheckCircle2, AlertCircle, Clock, Loader2 } from "lucide-react";
import { useAppStore, type Task, type TaskStatus } from "@/lib/store";
import { cancelTask } from "@/lib/ipc";

// ---------------------------------------------------------------------------
// Public entry: trigger button + panel (mounted inside Sidebar)
// ---------------------------------------------------------------------------

export function DownloadManagerButton() {
  const [open, setOpen] = useState(false);
  const tasks = useAppStore((s) => s.tasks);
  const activeCount = [...tasks.values()].filter((t) => isActive(t.status)).length;

  return (
    <>
      <button
        onClick={() => setOpen((v) => !v)}
        className="relative flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-muted hover:bg-surface-2 hover:text-foreground transition-colors"
        aria-label="Toggle Download Manager"
      >
        {activeCount > 0 ? (
          <Loader2 className="size-4 shrink-0 animate-spin text-primary" />
        ) : (
          <Download className="size-4 shrink-0" />
        )}
        <span className="flex-1 text-left">Downloads</span>
        {activeCount > 0 && (
          <span className="rounded-full bg-primary px-1.5 py-0.5 text-[10px] font-semibold leading-none text-primary-foreground">
            {activeCount}
          </span>
        )}
      </button>

      {open && <DownloadManagerPanel onClose={() => setOpen(false)} />}
    </>
  );
}

// ---------------------------------------------------------------------------
// Panel (fixed, left-anchored so it doesn't cover the main content area)
// ---------------------------------------------------------------------------

interface DownloadManagerPanelProps {
  onClose: () => void;
}

function DownloadManagerPanel({ onClose }: DownloadManagerPanelProps) {
  const tasks = useAppStore((s) => s.tasks);

  // Sort by id descending: newest first.
  const sorted = [...tasks.values()].sort((a, b) => b.id - a.id);

  return (
    <>
      {/* Backdrop — click outside to close */}
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Panel anchored to the left edge, below header */}
      <div className="fixed left-0 top-0 z-50 flex h-full w-72 flex-col border-r border-border bg-surface shadow-xl">
        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-border px-4 py-3">
          <h2 className="text-sm font-semibold">Download Manager</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
            aria-label="Close Download Manager"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* Task list */}
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {sorted.length === 0 ? (
            <p className="mt-4 text-center text-xs text-muted">No downloads yet.</p>
          ) : (
            <ul className="flex flex-col gap-2">
              {sorted.map((task) => (
                <TaskRow key={task.id} task={task} />
              ))}
            </ul>
          )}
        </div>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------
// Individual task row
// ---------------------------------------------------------------------------

interface TaskRowProps {
  task: Task;
}

function TaskRow({ task }: TaskRowProps) {
  const [cancelling, setCancelling] = useState(false);

  async function handleCancel() {
    setCancelling(true);
    try {
      await cancelTask(task.id);
    } catch {
      // Backend may reject if already terminal; ignore — store update reflects truth.
    } finally {
      setCancelling(false);
    }
  }

  const canCancel = isActive(task.status);

  return (
    <li className="rounded-lg border border-border bg-surface-2 p-3">
      {/* Parent label + status badge */}
      <div className="flex items-start justify-between gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-medium" title={task.parentLabel}>
          {task.parentLabel}
        </span>
        <StatusBadge status={task.status} />
      </div>

      {/* Current child label */}
      {task.currentChild && (
        <p className="mt-1 truncate text-xs text-muted" title={task.currentChild}>
          {task.currentChild}
        </p>
      )}

      {/* Progress counts + cancel */}
      <div className="mt-2 flex items-center gap-2">
        {task.total > 0 && (
          <span className="text-xs text-muted tabular-nums">
            {task.done}/{task.total}
          </span>
        )}

        {/* Progress bar — only while downloading */}
        {task.status.kind === "downloading" && task.total > 0 && (
          <div className="h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-border">
            <div
              className="h-full rounded-full bg-primary transition-all"
              style={{ width: `${Math.min(100, (task.done / task.total) * 100)}%` }}
            />
          </div>
        )}

        <div className="ml-auto">
          {canCancel && (
            <button
              onClick={handleCancel}
              disabled={cancelling}
              className="flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-muted transition-colors hover:bg-surface hover:text-danger disabled:opacity-50"
              aria-label="Cancel task"
            >
              {cancelling ? (
                <Loader2 className="size-3 animate-spin" />
              ) : (
                <XCircle className="size-3" />
              )}
              Cancel
            </button>
          )}
        </div>
      </div>

      {/* Error message */}
      {task.status.kind === "failed" && (
        <p className="mt-1.5 text-xs text-danger">{task.status.message}</p>
      )}
    </li>
  );
}

// ---------------------------------------------------------------------------
// Status badge
// ---------------------------------------------------------------------------

function StatusBadge({ status }: { status: TaskStatus }) {
  switch (status.kind) {
    case "queued":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-border px-2 py-0.5 text-[10px] font-medium text-muted">
          <Clock className="size-2.5" />
          Queued
        </span>
      );
    case "planning":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-border px-2 py-0.5 text-[10px] font-medium text-muted">
          <Loader2 className="size-2.5 animate-spin" />
          Planning
        </span>
      );
    case "downloading":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-primary/20 px-2 py-0.5 text-[10px] font-medium text-primary">
          <Loader2 className="size-2.5 animate-spin" />
          Downloading
        </span>
      );
    case "applying":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-accent/20 px-2 py-0.5 text-[10px] font-medium text-accent">
          <Loader2 className="size-2.5 animate-spin" />
          Applying
        </span>
      );
    case "done":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-success/20 px-2 py-0.5 text-[10px] font-medium text-success">
          <CheckCircle2 className="size-2.5" />
          Done
        </span>
      );
    case "failed":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-danger/20 px-2 py-0.5 text-[10px] font-medium text-danger">
          <AlertCircle className="size-2.5" />
          Failed
        </span>
      );
    case "cancelled":
      return (
        <span className="flex shrink-0 items-center gap-1 rounded-full bg-border px-2 py-0.5 text-[10px] font-medium text-muted">
          <XCircle className="size-2.5" />
          Cancelled
        </span>
      );
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function isActive(status: TaskStatus): boolean {
  return (
    status.kind === "queued" ||
    status.kind === "planning" ||
    status.kind === "downloading" ||
    status.kind === "applying"
  );
}
