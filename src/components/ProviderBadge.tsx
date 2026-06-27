/**
 * Small inline badge indicating which provider a project comes from.
 * Used in Browse (CP3) and the Manage Installs slide-over (CP5).
 */

import type { ProviderKind } from "@/lib/ipc";

/**
 * Map a wire provider string (e.g. from `Instance.source.provider`) to the
 * canonical `ProviderKind` camelCase union used by the frontend.
 * "curseforge" | "curseForge" → "curseForge"; anything else → "modrinth".
 */
export function toProviderKind(wire: string): ProviderKind {
  if (wire === "curseforge" || wire === "curseForge") return "curseForge";
  if (wire === "ftb") return "ftb";
  if (wire === "atlauncher") return "atlauncher";
  return "modrinth";
}

interface ProviderBadgeProps {
  provider: ProviderKind;
  className?: string;
}

const PROVIDER_LABELS: Record<ProviderKind, string> = {
  modrinth: "Modrinth",
  curseForge: "CurseForge",
  ftb: "FTB",
  atlauncher: "ATLauncher",
};

const PROVIDER_COLORS: Record<ProviderKind, string> = {
  modrinth: "bg-emerald-500/15 text-emerald-400 ring-emerald-500/20",
  curseForge: "bg-orange-500/15 text-orange-400 ring-orange-500/20",
  ftb: "bg-sky-500/15 text-sky-400 ring-sky-500/20",
  atlauncher: "bg-indigo-500/15 text-indigo-400 ring-indigo-500/20",
};

export function ProviderBadge({ provider, className = "" }: ProviderBadgeProps) {
  return (
    <span
      className={
        "inline-flex items-center rounded-md px-1.5 py-0.5 text-xs font-medium ring-1 ring-inset " +
        PROVIDER_COLORS[provider] +
        (className ? " " + className : "")
      }
    >
      {PROVIDER_LABELS[provider]}
    </span>
  );
}
