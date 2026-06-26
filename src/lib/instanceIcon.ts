import { useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getAppPaths, type Instance } from "@/lib/ipc";

/**
 * Resolve an instance's display icon source with precedence:
 * custom user icon (a file under the instance dir, served via the Tauri asset
 * protocol) > pack provider icon (`source.iconUrl`) > `null` (caller shows a
 * placeholder). The custom filename is timestamped (`icon-<ts>.<ext>`), so a
 * replacement yields a new URL and never serves a stale cached image.
 */
export function useInstanceIconSrc(instance: Instance | null | undefined): string | null {
  // useQuery is called unconditionally (hook order stays stable across renders).
  const { data: paths } = useQuery({ queryKey: ["app-paths"], queryFn: getAppPaths });
  if (!instance) return null;
  const custom =
    instance.icon && paths
      ? convertFileSrc(`${paths.dataDir}/instances/${instance.slug}/${instance.icon}`)
      : null;
  return custom ?? instance.source?.iconUrl ?? null;
}
