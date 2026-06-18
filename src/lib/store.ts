import { create } from "zustand";
import type {
  AddModResult,
  CfImportResult,
  MrpackImportResult,
  ModpackInstallResult,
  PackUpdateResult,
  UpdateModResult,
} from "@/lib/ipc";

// ---------------------------------------------------------------------------
// Task types — mirror task_manager.rs (serde rename_all = "camelCase")
// ---------------------------------------------------------------------------

export type TaskKind = "synthetic" | "packInstall" | "packUpdate" | "modAdd" | "modUpdate";

/** Mirrors TaskStatus (#[serde(tag = "kind")]) */
export type TaskStatus =
  | { kind: "queued" }
  | { kind: "planning" }
  | { kind: "downloading" }
  | { kind: "applying" }
  | { kind: "done" }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" };

export interface ChildItem {
  label: string;
}

/**
 * Terminal result carried by a `Done` task.
 * CP-3/CP-4 populate this; CP-7 forward-declares the union so callers can
 * consume it without needing a separate query.
 */
export type TaskResult =
  | ModpackInstallResult
  | CfImportResult
  | MrpackImportResult
  | PackUpdateResult
  | AddModResult
  | UpdateModResult;

/** Mirrors task_manager::Task (serde camelCase) */
export interface Task {
  id: number;
  kind: TaskKind;
  parentLabel: string;
  status: TaskStatus;
  children: ChildItem[];
  currentChild: string | null;
  done: number;
  total: number;
  /** Terminal result; present only when status.kind === "done". CP-3/CP-4 populate. */
  result?: TaskResult | null;
}

/** Mirrors TaskProgressPayload (serde camelCase) */
export interface TaskProgressUpdate {
  taskId: number;
  currentChild: string | null;
  done: number;
  total: number;
  bytes: number;
}

// ---------------------------------------------------------------------------
// Run types — mirror RunUpdatePayload / RunInfoPayload / RunLogPayload
// ---------------------------------------------------------------------------

/** Mirrors RunUpdatePayload / RunInfoPayload (serde camelCase) */
export interface RunState {
  slug: string;
  /** `"preparing"` | `"running"` | `"exited"` | `"killed"` | `"failed"` */
  status: string;
  exitCode: number | null;
  /** Present only from list_running hydration (elapsed_ms). */
  elapsedMs?: number;
}

/** Mirrors RunLogPayload (serde camelCase) */
export interface RunLogLine {
  stream: string;
  line: string;
}

// ---------------------------------------------------------------------------
// Store slices
// ---------------------------------------------------------------------------

interface TasksSlice {
  /** All tasks keyed by numeric id. Entries survive navigation. */
  tasks: Map<number, Task>;
  upsertTask: (task: Task) => void;
  patchTaskProgress: (update: TaskProgressUpdate) => void;
}

interface RunsSlice {
  /** Running/preparing/terminal runs keyed by instance slug. */
  runs: Map<string, RunState>;
  /** Buffered log lines per instance slug. */
  runLogs: Map<string, RunLogLine[]>;
  upsertRun: (run: RunState) => void;
  appendLog: (slug: string, line: RunLogLine) => void;
  setLogs: (slug: string, lines: RunLogLine[]) => void;
}

export type AppStore = TasksSlice & RunsSlice;

export const useAppStore = create<AppStore>()((set) => ({
  // --- tasks slice ---
  tasks: new Map(),

  upsertTask: (task) =>
    set((state) => {
      const next = new Map(state.tasks);
      next.set(task.id, task);
      return { tasks: next };
    }),

  patchTaskProgress: (update) =>
    set((state) => {
      const existing = state.tasks.get(update.taskId);
      if (!existing) return {};
      const next = new Map(state.tasks);
      next.set(update.taskId, {
        ...existing,
        currentChild: update.currentChild,
        done: update.done,
        total: update.total,
      });
      return { tasks: next };
    }),

  // --- runs slice ---
  runs: new Map(),
  runLogs: new Map(),

  upsertRun: (run) =>
    set((state) => {
      const next = new Map(state.runs);
      next.set(run.slug, run);
      return { runs: next };
    }),

  appendLog: (slug, line) =>
    set((state) => {
      const prev = state.runLogs.get(slug) ?? [];
      const next = new Map(state.runLogs);
      next.set(slug, [...prev, line]);
      return { runLogs: next };
    }),

  setLogs: (slug, lines) =>
    set((state) => {
      const next = new Map(state.runLogs);
      next.set(slug, lines);
      return { runLogs: next };
    }),
}));
