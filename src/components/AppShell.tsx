import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "@/components/Sidebar";
import { Toasts } from "@/components/Toasts";
import {
  events,
  listRunning,
  listTasks,
  getRunLogs,
  getSettings,
  type RunInfoPayload,
  type RunLogPayload,
} from "@/lib/ipc";
import { useAppStore, useUiStore, type Task, type RunState, type RunLogLine } from "@/lib/store";

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

  // First-run sidebar seed: if the user has never manually toggled the sidebar
  // (i.e. the "apex-ui" key doesn't exist in localStorage yet), apply the
  // backend Setting's `sidebarStartCollapsed` default so users who configured
  // "start collapsed" get that behaviour before their first manual toggle.
  useEffect(() => {
    if (localStorage.getItem("apex-ui") !== null) return;
    getSettings()
      .then((settings) => {
        if (localStorage.getItem("apex-ui") !== null) return; // race guard
        useUiStore.getState().setSidebarCollapsed(settings.sidebarStartCollapsed ?? false);
      })
      .catch(() => {
        // Best-effort — non-critical. Default (expanded) is fine.
      });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let cancelled = false;

    // Collect unlisten handles so we can tear down cleanly if the component
    // ever unmounts (e.g. in a test environment; in production it never does).
    const unlisteners: Array<() => void> = [];

    function reg(fn: () => void) {
      unlisteners.push(fn);
    }

    // Subscribe to task lifecycle events.
    events.taskUpdate.listen((event) => {
      // The deserialized payload is a Task (store re-exports the generated type).
      upsertTask(event.payload as Task);
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    events.taskProgress.listen((event) => {
      patchTaskProgress(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // Subscribe to run lifecycle events.
    events.runUpdate.listen((event) => {
      upsertRun({
        slug: event.payload.slug,
        status: event.payload.status,
        exitCode: event.payload.exitCode,
      });
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // Subscribe to log events and buffer into the runs slice.
    events.launchLog.listen((event) => {
      const { instanceId, stream, line } = event.payload;
      appendLog(instanceId, { stream, line });
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    events.launchExit.listen((event) => {
      const { instanceId, code } = event.payload;
      appendLog(instanceId, { stream: "launcher", line: `process exited (code ${code ?? "unknown"})` });
    }).then((fn) => {
      if (cancelled) fn();
      else reg(fn);
    }).catch(console.error);

    // install://log has no instanceId; attribute to whichever run is Preparing.
    events.installLog.listen((event) => {
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
      <Toasts />
    </div>
  );
}
