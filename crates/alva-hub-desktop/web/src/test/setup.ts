// INPUT:  @testing-library/jest-dom matchers
// OUTPUT: jest-dom assertions (toBeInTheDocument, toBeDisabled, ...) for vitest
// POS:    Shared setup for every Hub Desktop web test.

import "@testing-library/jest-dom/vitest";

class TestResizeObserver implements ResizeObserver {
  disconnect() {}
  observe() {}
  unobserve() {}
}

globalThis.ResizeObserver = TestResizeObserver;

// Node ≥ 26 ships an experimental localStorage global that is not backed by
// any store in jsdom runs ("--localstorage-file was not provided"), so every
// read/write throws. Back it with a plain in-memory implementation; on older
// runtimes jsdom already provides a working localStorage and this is a no-op.
const backing = globalThis.localStorage;
const working =
  backing &&
  typeof backing.getItem === "function" &&
  (() => {
    try {
      backing.setItem("__alva_probe__", "1");
      backing.removeItem("__alva_probe__");
      return true;
    } catch {
      return false;
    }
  })();
if (!working) {
  const store = new Map<string, string>();
  globalThis.localStorage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
    clear: () => store.clear(),
    key: (index: number) => Array.from(store.keys())[index] ?? null,
    get length() {
      return store.size;
    },
  } as Storage;
}
