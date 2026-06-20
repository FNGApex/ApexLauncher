import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  Task,
  TaskProgressPayload,
  RunUpdatePayload,
  RunLogPayload,
} from "@/lib/bindings";

// ---------------------------------------------------------------------------
// IPC types — generated from the Rust backend (tauri-specta). Re-exported here
// under the names store consumers import; the store no longer hand-mirrors them.
// ---------------------------------------------------------------------------

export type {
  Task,
  TaskKind,
  TaskStatus,
  ChildItem,
  TaskResult,
} from "@/lib/bindings";

/**
 * Run state held in the runs slice: the `run://update` payload plus the
 * optional `elapsedMs` that only `list_running` hydration carries (the event
 * payload omits it). Composed from generated types, not hand-redeclared.
 */
export type RunState = RunUpdatePayload & { elapsedMs?: number | null };

/** Buffered log line in the runs slice (mirrors the `get_run_logs` / `launch://log` shape). */
export type RunLogLine = RunLogPayload;

// ---------------------------------------------------------------------------
// Store slices
// ---------------------------------------------------------------------------

interface TasksSlice {
  /** All tasks keyed by numeric id. Entries survive navigation. */
  tasks: Map<number, Task>;
  upsertTask: (task: Task) => void;
  patchTaskProgress: (update: TaskProgressPayload) => void;
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

// ---------------------------------------------------------------------------
// UI store — persisted to localStorage (key: "apex-ui")
// Only UI preferences live here. The Map-based AppStore above is NOT persisted
// (Maps don't serialize cleanly with JSON storage).
// ---------------------------------------------------------------------------

interface UiState {
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  setSidebarCollapsed: (v: boolean) => void;
  browseProvider: "curseforge" | "modrinth";
  setBrowseProvider: (p: "curseforge" | "modrinth") => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      setSidebarCollapsed: (v) => set({ sidebarCollapsed: v }),
      browseProvider: "curseforge",
      setBrowseProvider: (p) => set({ browseProvider: p }),
    }),
    { name: "apex-ui" },
  ),
);
