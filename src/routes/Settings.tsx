export function Settings() {
  return (
    <div className="px-8 py-7">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold">Settings</h1>
        <p className="text-sm text-muted">Launcher preferences</p>
      </header>

      <div className="space-y-4">
        <SettingRow
          title="Default memory allocation"
          desc="RAM given to new instances (configurable per-instance later)."
        >
          <span className="text-sm text-muted">4096 MB</span>
        </SettingRow>
        <SettingRow
          title="CurseForge API key"
          desc="Required for CurseForge browsing & pack import (Phase 5/6)."
        >
          <span className="text-sm text-muted">Not set</span>
        </SettingRow>
        <SettingRow
          title="Java"
          desc="Per-instance JREs are downloaded automatically (Phase 2)."
        >
          <span className="text-sm text-muted">Auto</span>
        </SettingRow>
      </div>
    </div>
  );
}

function SettingRow({
  title,
  desc,
  children,
}: {
  title: string;
  desc: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between rounded-xl border border-border bg-surface px-5 py-4">
      <div>
        <div className="text-sm font-medium">{title}</div>
        <div className="text-xs text-muted">{desc}</div>
      </div>
      {children}
    </div>
  );
}
