import { useOutletContext } from "react-router-dom";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { Stat, formatDate, formatPlaytime } from "@/routes/InstanceDetail";

export function TechTab() {
  const { instance } = useOutletContext<InstanceTabContext>();

  return (
    <div className="flex flex-col gap-6">
      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <Stat label="Memory" value={`${instance.java.memoryMb} MB`} />
        <Stat
          label="Java"
          value={instance.java.major ? `Java ${instance.java.major}` : "Auto"}
        />
        <Stat label="Managed mods" value={`${instance.mods.length}`} />
        <Stat label="Created" value={formatDate(instance.created)} />
      </dl>

      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-2">
        <Stat
          label="Last played"
          value={instance.lastPlayed ? formatDate(instance.lastPlayed) : "Never"}
        />
        <Stat
          label="Total playtime"
          value={formatPlaytime(instance.totalPlaytimeSec)}
        />
      </dl>
    </div>
  );
}
