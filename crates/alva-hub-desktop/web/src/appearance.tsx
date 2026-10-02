// INPUT:  React context, localStorage, and operating-system media preferences
// OUTPUT: AppearanceProvider, useAppearance, and typed appearance preferences
// POS:    Persistent Hub-wide theme, density, and accessible motion preferences

import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useState, type ReactNode } from "react";

export type ThemePreference = "dark" | "light" | "system";
export interface AppearancePreferences {
  theme: ThemePreference;
  compact: boolean;
  reduceMotion: boolean;
}

const storageKey = "alva-hub.appearance.v1";
const defaults: AppearancePreferences = { theme: "dark", compact: false, reduceMotion: false };

function parsePreferences(raw: string | null): AppearancePreferences {
  if (!raw || raw.length > 1024) return defaults;
  try {
    const value = JSON.parse(raw);
    if (value?.version === 1 && ["dark", "light", "system"].includes(value.theme)
      && typeof value.compact === "boolean" && typeof value.reduceMotion === "boolean") {
      return { theme: value.theme, compact: value.compact, reduceMotion: value.reduceMotion };
    }
  } catch { /* Invalid local data must not prevent opening the product. */ }
  return defaults;
}

function useMediaPreference(query: string, fallback: boolean) {
  const [matches, setMatches] = useState(() => window.matchMedia?.(query).matches ?? fallback);
  useEffect(() => {
    const media = window.matchMedia?.(query);
    if (!media) return;
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);
  return matches;
}

interface AppearanceContextValue extends AppearancePreferences {
  effectiveTheme: "dark" | "light";
  effectiveReduceMotion: boolean;
  persistenceFailed: boolean;
  update: (patch: Partial<AppearancePreferences>) => void;
}

const AppearanceContext = createContext<AppearanceContextValue | null>(null);

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [initial] = useState(() => {
    try { return { preferences: parsePreferences(localStorage.getItem(storageKey)), failed: false }; }
    catch { return { preferences: defaults, failed: true }; }
  });
  const [preferences, setPreferences] = useState(initial.preferences);
  const [persistenceFailed, setPersistenceFailed] = useState(initial.failed);
  const systemDark = useMediaPreference("(prefers-color-scheme: dark)", true);
  const systemReducedMotion = useMediaPreference("(prefers-reduced-motion: reduce)", false);
  const effectiveTheme = preferences.theme === "system" ? (systemDark ? "dark" : "light") : preferences.theme;
  // An in-app preference may strengthen, never defeat, the OS accessibility setting.
  const effectiveReduceMotion = preferences.reduceMotion || systemReducedMotion;

  const update = useCallback((patch: Partial<AppearancePreferences>) => {
    const next = { ...preferences, ...patch };
    setPreferences(next);
    try {
      localStorage.setItem(storageKey, JSON.stringify({ version: 1, ...next }));
      setPersistenceFailed(false);
    } catch { setPersistenceFailed(true); }
  }, [preferences]);

  useEffect(() => {
    const sync = (event: StorageEvent) => {
      if (event.key !== storageKey && event.key !== null) return;
      setPreferences(parsePreferences(event.key === null ? null : event.newValue));
    };
    window.addEventListener("storage", sync);
    return () => window.removeEventListener("storage", sync);
  }, []);

  useLayoutEffect(() => {
    const root = document.documentElement;
    const values = {
      "data-theme": effectiveTheme,
      "data-density": preferences.compact ? "compact" : "default",
      "data-reduced-motion": String(effectiveReduceMotion),
    };
    const previous = Object.keys(values).map((key) => [key, root.getAttribute(key)] as const);
    for (const [key, value] of Object.entries(values)) root.setAttribute(key, value);
    return () => {
      for (const [key, value] of previous) {
        if (value === null) root.removeAttribute(key);
        else root.setAttribute(key, value);
      }
    };
  }, [effectiveTheme, preferences.compact, effectiveReduceMotion]);

  return <AppearanceContext.Provider value={{ ...preferences, effectiveTheme, effectiveReduceMotion, persistenceFailed, update }}>{children}</AppearanceContext.Provider>;
}

export function useAppearance() {
  const value = useContext(AppearanceContext);
  if (!value) throw new Error("AppearanceProvider is required");
  return value;
}
