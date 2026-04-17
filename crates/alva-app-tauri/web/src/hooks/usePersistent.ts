import { useState } from "react";

/**
 * `useState` whose value is persisted to `localStorage` on every set.
 * Falls back gracefully in private mode or when storage is unavailable.
 */
export function usePersistent(
  key: string,
  initial: string,
): [string, (v: string) => void] {
  const [value, setValue] = useState<string>(() => {
    try {
      return localStorage.getItem(key) ?? initial;
    } catch {
      return initial;
    }
  });
  const set = (v: string) => {
    setValue(v);
    try {
      localStorage.setItem(key, v);
    } catch {
      // ignore
    }
  };
  return [value, set];
}
