// Theme preference: dark (default, no class) | light (.light on <html>) | system
// (follow prefers-color-scheme). Persisted to localStorage so an inline <head>
// script can apply it before first paint (no FOUC). Mirrors the existing
// `.light` token flip in styles.css — never adds a `.dark` class or `dark:`
// utilities. See docs/design/theme-and-icons.md.

export type ThemePref = "system" | "light" | "dark";

const STORAGE_KEY = "apex-theme";

const LIGHT_MQ = "(prefers-color-scheme: light)";

/** Read the stored preference, defaulting to "system". */
export function getThemePref(): ThemePref {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    // localStorage unavailable — fall through to default.
  }
  return "system";
}

/** Resolve a preference to whether the light theme should be active. */
function isLight(pref: ThemePref): boolean {
  if (pref === "light") return true;
  if (pref === "dark") return false;
  return window.matchMedia(LIGHT_MQ).matches; // system
}

/** Apply the current (or given) preference to the document root. */
export function applyTheme(pref: ThemePref = getThemePref()): void {
  document.documentElement.classList.toggle("light", isLight(pref));
}

/** Persist a preference and apply it immediately. */
export function setThemePref(pref: ThemePref): void {
  try {
    localStorage.setItem(STORAGE_KEY, pref);
  } catch {
    // Non-fatal — the in-memory apply below still themes this session.
  }
  applyTheme(pref);
}

/**
 * Re-apply the theme when the OS scheme changes, but only while the stored
 * preference is "system". Returns an unsubscribe fn. Call once at startup.
 */
export function subscribeSystem(): () => void {
  const mq = window.matchMedia(LIGHT_MQ);
  const onChange = () => {
    if (getThemePref() === "system") applyTheme("system");
  };
  mq.addEventListener("change", onChange);
  return () => mq.removeEventListener("change", onChange);
}
