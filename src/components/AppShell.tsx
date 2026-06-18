import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "@/components/Sidebar";
import {
  LAUNCH_LOG_EVENT,
  LAUNCH_EXIT_EVENT,
  INSTALL_LOG_EVENT,
  listenTaskUpdate,
  listenTaskProgress,
  listenRunUpdate,
  listRunning,
  listTasks,
  getRunLogs,
  type RunUpdatePayload,
  type RunInfoPayload,
  type RunLogPayload,
  type TaskProgressPayload,
} from "@/lib/ipc";
import { useAppStore, type Task, type RunState, type RunLogLine } from "@/lib/store";
import { listen } from "@tauri-apps/api/event";
import type { LaunchLogPayload, LaunchExitPayload, InstallLogPayload } from "@/lib/ipc";

/** Top-level chrome: fixed sidebar + scrollable content area.
 *
 * AppShell never unmounts, so event subscriptions here survive navigation.
 * The store slices (tasks, runs, runLogs) are populated here and consumed
 * by any route without re-subscribing. */
export function AppShell() {
  const upsertTask = useAppStore((s) => s.upsertTask);
  const patchTaskProgress = useAppStore((s) => s.patchTaskProgress);
  const upsertRun = useAppStore((s) => s.upsertRun);
  const appendLog = useAppStore((s) => s.appendLog);
  const setLogs = useAppStore((s) => s.setLogs);

  useEffect(() => {
    let cancelled = false;

    // Collect unlisten handles so we can tear down cleanly if the component
    // ever unmounts (e.g. in a test environment; in production it never does).
    const unlisteners: Array<() => void> = [];

    function reg(fn: () => void) {
      unlisteners.push(fn);
    }

    // Subscribe to task lifecycle events.
    listenTaskUpdate((raw) => {
      // The payload shape matches Task from store.ts exactly.
      upsertTask(raw as Task);
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    listenTaskProgress((payload: TaskProgressPayload) => {
      patchTaskProgress(payload);
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // Subscribe to run lifecycle events.
    listenRunUpdate((payload: RunUpdatePayload) => {
      upsertRun({
        slug: payload.slug,
        status: payload.status,
        exitCode: payload.exitCode,
      });
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // Subscribe to log events and buffer into the runs slice.
    listen<LaunchLogPayload>(LAUNCH_LOG_EVENT, (event) => {
      const { instanceId, stream, line } = event.payload;
      appendLog(instanceId, { stream, line });
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    listen<LaunchExitPayload>(LAUNCH_EXIT_EVENT, (event) => {
      const { instanceId, code } = event.payload;
      appendLog(instanceId, { stream: "launcher", line: `process exited (code ${code ?? "unknown"})` });
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // install://log has no instanceId; attribute to whichever run is Preparing.
    listen<InstallLogPayload>(INSTALL_LOG_EVENT, (event) => {
      const { stream, line } = event.payload;
      const runs = useAppStore.getState().runs;
      const preparingSlug = [...runs.values()].find((r) => r.status === "preparing")?.slug;
      if (preparingSlug) {
        appendLog(preparingSlug, { stream: `install:${stream}`, line });
      }
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // Hydrate on mount: pull existing tasks and running instances so a fresh
    // load or page reload re-syncs without waiting for the next event.
    listTasks().then((rawTasks) => {
      if (cancelled) return;
      for (const raw of rawTasks) {
        upsertTask(raw as Task);
      }
    }).catch(console.error);

    listRunning().then(async (runs: RunInfoPayload[]) => {
      if (cancelled) return;
      for (const run of runs) {
        upsertRun({
          slug: run.slug,
          status: run.status,
          exitCode: run.exitCode,
          elapsedMs: run.elapsedMs,
        } satisfies RunState);
        // Replay buffered log lines for each active run.
        const logs = await getRunLogs(run.slug).catch(() => null);
        if (!cancelled && logs) {
          setLogs(run.slug, logs.map((l: RunLogPayload) => ({ stream: l.stream, line: l.line } satisfies RunLogLine)));
        }
      }
    }).catch(console.error);

    return () => {
      cancelled = true;
      for (const fn of unlisteners) fn();
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []); // mount-once; stable store selectors from zustand don't need to be deps

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
