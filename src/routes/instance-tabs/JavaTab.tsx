import { useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, CheckCircle2, FolderOpen, Loader2 } from "lucide-react";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { getSettings, setInstanceJava, validateJavaPath } from "@/lib/ipc";
import type { JavaCfg } from "@/lib/ipc";

// ---------------------------------------------------------------------------
// Form state shape — mirrors JavaCfg but uses strings for inputs
// ---------------------------------------------------------------------------

interface FormState {
  usePackSettings: boolean;
  memoryMb: number;
  minMemoryMb: string; // "" means null/unset
  argsOverride: string;
  pathOverride: string;
}

function initForm(java: JavaCfg): FormState {
  return {
    usePackSettings: java.usePackSettings ?? false,
    memoryMb: java.memoryMb ?? 2048,
    minMemoryMb: java.minMemoryMb != null ? String(java.minMemoryMb) : "",
    argsOverride: java.argsOverride ?? "",
    pathOverride: java.pathOverride ?? "",
  };
}

// ---------------------------------------------------------------------------
// JavaTab
// ---------------------------------------------------------------------------

export function JavaTab() {
  const { slug, instance, invalidate } = useOutletContext<InstanceTabContext>();

  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
    staleTime: 60_000,
  });

  const [form, setForm] = useState<FormState>(() => initForm(instance.java));

  // Java path probe state
  const [probeStatus, setProbeStatus] = useState<
    { kind: "idle" } | { kind: "probing" } | { kind: "ok"; major: number; version: string } | { kind: "error"; message: string }
  >({ kind: "idle" });

  // Save state
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [saveError, setSaveError] = useState<string | null>(null);

  const settings = settingsQuery.data;

  // ---------------------------------------------------------------------------
  // Handlers
  // ---------------------------------------------------------------------------

  async function handleBrowse() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Java executable", extensions: ["exe"] }],
    });
    if (!selected) return;

    setForm((f) => ({ ...f, pathOverride: selected }));
    setProbeStatus({ kind: "probing" });
    try {
      const probe = await validateJavaPath(selected);
      setProbeStatus({ kind: "ok", major: probe.major, version: probe.version });
    } catch (err) {
      setProbeStatus({ kind: "error", message: String(err) });
    }
  }

  function handlePathChange(value: string) {
    setForm((f) => ({ ...f, pathOverride: value }));
    setProbeStatus({ kind: "idle" });
  }

  async function handleSave() {
    setSaveState("saving");
    setSaveError(null);
    try {
      const cfg: JavaCfg = {
        major: instance.java.major ?? null,
        argsOverride: form.argsOverride.trim() || null,
        memoryMb: form.memoryMb,
        minMemoryMb: form.minMemoryMb !== "" ? parseInt(form.minMemoryMb, 10) : null,
        pathOverride: form.pathOverride.trim() || null,
        usePackSettings: form.usePackSettings,
      };
      await setInstanceJava(slug, cfg);
      invalidate();
      setSaveState("saved");
      setTimeout(() => setSaveState("idle"), 2000);
    } catch (err) {
      setSaveError(String(err));
      setSaveState("error");
    }
  }

  // ---------------------------------------------------------------------------
  // Render
  // ---------------------------------------------------------------------------

  return (
    <div className="flex flex-col gap-6">
      {/* Toggle: use per-instance settings */}
      <div className="flex flex-col gap-3 rounded-xl border border-border bg-surface p-4">
        <label className="flex cursor-pointer items-start gap-3">
          <input
            type="checkbox"
            className="mt-0.5 accent-accent"
            checked={form.usePackSettings}
            onChange={(e) => setForm((f) => ({ ...f, usePackSettings: e.target.checked }))}
          />
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium text-foreground">
              Use this instance's own Java &amp; memory settings
            </span>
            <span className="text-xs text-muted">
              When off, this instance uses the global default. CurseForge and
              Modrinth packs don't publish recommended Java settings, so these are
              your own per-instance values.
            </span>
          </div>
        </label>
      </div>

      {/* Global defaults (read-only, shown when override is OFF) */}
      {!form.usePackSettings && (
        <div className="flex flex-col gap-3 rounded-xl border border-border bg-surface/60 p-4">
          <p className="text-xs font-medium uppercase tracking-wide text-muted">
            Global defaults (read-only)
          </p>
          {settingsQuery.isLoading && (
            <div className="flex items-center gap-2 text-sm text-muted">
              <Loader2 className="size-4 animate-spin" />
              Loading…
            </div>
          )}
          {settings && (
            <div className="flex flex-col gap-2 text-sm">
              <div className="flex items-center gap-2">
                <span className="w-28 shrink-0 text-muted">Max RAM</span>
                <span className="font-mono">{settings.defaultMemoryMb ?? 2048} MB</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="w-28 shrink-0 text-muted">Extra JVM args</span>
                <span className="font-mono text-xs">
                  {settings.defaultJavaArgs ? settings.defaultJavaArgs : <em className="text-muted">none</em>}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="w-28 shrink-0 text-muted">Java</span>
                <span className="text-muted">Auto (bundled / auto-detected)</span>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Per-instance editable fields (shown when override is ON) */}
      {form.usePackSettings && (
        <div className="flex flex-col gap-4 rounded-xl border border-border bg-surface p-4">
          <p className="text-xs font-medium uppercase tracking-wide text-muted">
            Per-instance settings
          </p>

          {/* Max RAM */}
          <div className="flex flex-col gap-1">
            <label className="text-sm font-medium text-foreground" htmlFor="java-max-ram">
              Max RAM (MB)
            </label>
            <input
              id="java-max-ram"
              type="number"
              min={512}
              step={256}
              className="w-36 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-accent"
              value={form.memoryMb}
              onChange={(e) => setForm((f) => ({ ...f, memoryMb: parseInt(e.target.value, 10) || 2048 }))}
            />
          </div>

          {/* Min RAM */}
          <div className="flex flex-col gap-1">
            <label className="text-sm font-medium text-foreground" htmlFor="java-min-ram">
              Min RAM (MB){" "}
              <span className="font-normal text-muted">— optional, leave empty to omit -Xms</span>
            </label>
            <input
              id="java-min-ram"
              type="number"
              min={256}
              step={256}
              placeholder="e.g. 512"
              className="w-36 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-foreground placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-accent"
              value={form.minMemoryMb}
              onChange={(e) => setForm((f) => ({ ...f, minMemoryMb: e.target.value }))}
            />
          </div>

          {/* Extra JVM args */}
          <div className="flex flex-col gap-1">
            <label className="text-sm font-medium text-foreground" htmlFor="java-extra-args">
              Extra JVM args
            </label>
            <input
              id="java-extra-args"
              type="text"
              placeholder="-XX:+UseG1GC -XX:MaxGCPauseMillis=50"
              className="rounded-lg border border-border bg-surface-2 px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-accent"
              value={form.argsOverride}
              onChange={(e) => setForm((f) => ({ ...f, argsOverride: e.target.value }))}
            />
          </div>

          {/* Java path */}
          <div className="flex flex-col gap-1">
            <label className="text-sm font-medium text-foreground" htmlFor="java-path">
              Java path
            </label>
            <div className="flex items-center gap-2">
              <input
                id="java-path"
                type="text"
                placeholder="Auto (bundled / auto-detected)"
                className="min-w-0 flex-1 rounded-lg border border-border bg-surface-2 px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-accent"
                value={form.pathOverride}
                onChange={(e) => handlePathChange(e.target.value)}
              />
              <button
                type="button"
                className="flex shrink-0 items-center gap-1.5 rounded-lg border border-border bg-surface px-3 py-1.5 text-sm text-foreground hover:bg-surface-2 active:opacity-80"
                onClick={handleBrowse}
              >
                <FolderOpen className="size-4" />
                Browse
              </button>
            </div>

            {/* Probe status */}
            {probeStatus.kind === "probing" && (
              <div className="flex items-center gap-1.5 text-xs text-muted">
                <Loader2 className="size-3 animate-spin" />
                Validating…
              </div>
            )}
            {probeStatus.kind === "ok" && (
              <div className="flex items-center gap-1.5 text-xs text-accent">
                <CheckCircle2 className="size-3" />
                Java {probeStatus.major} ({probeStatus.version})
              </div>
            )}
            {probeStatus.kind === "error" && (
              <div className="flex items-center gap-1.5 text-xs text-danger">
                <AlertCircle className="size-3" />
                {probeStatus.message}
              </div>
            )}
            {probeStatus.kind === "idle" && !form.pathOverride && (
              <p className="text-xs text-muted">
                Empty path = Auto (bundled / auto-detected).
              </p>
            )}
          </div>
        </div>
      )}

      {/* Save button */}
      <div className="flex items-center gap-3">
        <button
          type="button"
          disabled={saveState === "saving"}
          className="flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90 active:opacity-80 disabled:opacity-50"
          onClick={handleSave}
        >
          {saveState === "saving" && <Loader2 className="size-4 animate-spin" />}
          {saveState === "saving" ? "Saving…" : saveState === "saved" ? "Saved!" : "Save"}
        </button>
        {saveState === "error" && saveError && (
          <div className="flex items-center gap-1.5 text-sm text-danger">
            <AlertCircle className="size-4" />
            {saveError}
          </div>
        )}
      </div>
    </div>
  );
}
