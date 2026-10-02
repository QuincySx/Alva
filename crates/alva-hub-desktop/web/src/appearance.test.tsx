// INPUT:  AppearanceProvider, browser preference/storage seams, and CSS token sources
// OUTPUT: Persistent theme/density/motion and accessibility contract regression tests
// POS:    Hub-wide appearance behavior coverage independent of engine accounts

import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppearanceProvider, useAppearance } from "./appearance";
import tokens from "./styles/tokens.css?raw";
import primitives from "./styles/primitives.css?raw";

const key = "alva-hub.appearance.v1";
function Probe() {
  const { effectiveTheme, effectiveReduceMotion, persistenceFailed, update } = useAppearance();
  return <>
    <output>{effectiveTheme}/{String(effectiveReduceMotion)}/{String(persistenceFailed)}</output>
    <button onClick={() => update({ theme: "light", compact: true, reduceMotion: true })}>Change</button>
    <button onClick={() => update({ theme: "system", reduceMotion: false })}>System</button>
  </>;
}

describe("persistent appearance", () => {
  let dark: boolean;
  let reduce: boolean;
  let listeners: Map<string, Set<() => void>>;
  const notify = () => act(() => { for (const callbacks of listeners.values()) for (const callback of callbacks) callback(); });
  beforeEach(() => {
    localStorage.clear();
    dark = false;
    reduce = false;
    listeners = new Map();
    vi.stubGlobal("matchMedia", (query: string) => {
      if (!listeners.has(query)) listeners.set(query, new Set());
      return {
        get matches() { return query.includes("color-scheme") ? dark : reduce; },
        addEventListener: (_: string, callback: () => void) => listeners.get(query)!.add(callback),
        removeEventListener: (_: string, callback: () => void) => listeners.get(query)!.delete(callback),
      };
    });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); localStorage.clear(); });

  it("preserves the dark baseline, applies before paint, and restores on remount", () => {
    const view = render(<AppearanceProvider><Probe /></AppearanceProvider>);
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(document.documentElement).toHaveAttribute("data-density", "default");
    fireEvent.click(screen.getByText("Change"));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
    expect(document.documentElement).toHaveAttribute("data-reduced-motion", "true");
    view.unmount();
    render(<AppearanceProvider><Probe /></AppearanceProvider>);
    expect(screen.getByText("light/true/false")).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
  });

  it("follows live system theme and never overrides OS reduced motion", () => {
    render(<AppearanceProvider><Probe /></AppearanceProvider>);
    fireEvent.click(screen.getByText("System"));
    expect(screen.getByText("light/false/false")).toBeInTheDocument();
    dark = true; reduce = true; notify();
    expect(screen.getByText("dark/true/false")).toBeInTheDocument();
    fireEvent.click(screen.getByText("System"));
    expect(document.documentElement).toHaveAttribute("data-reduced-motion", "true");
    reduce = false; notify();
    expect(document.documentElement).toHaveAttribute("data-reduced-motion", "false");
  });

  it.each(["{", "null", '{"version":2,"theme":"light","compact":true,"reduceMotion":true}', '{"version":1,"theme":"unknown","compact":true,"reduceMotion":true}', '{"version":1,"theme":"light","compact":"yes","reduceMotion":true}', " ".repeat(1025)])("ignores invalid or oversized saved preferences: %s", (raw) => {
    localStorage.setItem(key, raw);
    render(<AppearanceProvider><Probe /></AppearanceProvider>);
    expect(screen.getByText("dark/false/false")).toBeInTheDocument();
  });

  it("keeps changes usable and reports storage failures; another save retries", () => {
    const storage = localStorage;
    const save = vi.fn(storage.setItem.bind(storage)).mockImplementationOnce(() => { throw new Error("quota"); });
    vi.stubGlobal("localStorage", { getItem: storage.getItem.bind(storage), setItem: save });
    render(<AppearanceProvider><Probe /></AppearanceProvider>);
    fireEvent.click(screen.getByText("Change"));
    expect(screen.getByText("light/true/true")).toBeInTheDocument();
    expect(localStorage.getItem(key)).toBeNull();
    fireEvent.click(screen.getByText("System"));
    expect(save).toHaveBeenCalledTimes(2);
    expect(screen.getByText("light/false/false")).toBeInTheDocument();
  });

  it("reports inaccessible storage without preventing startup", () => {
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("blocked"); } });
    render(<AppearanceProvider><Probe /></AppearanceProvider>);
    expect(screen.getByText("dark/false/true")).toBeInTheDocument();
  });

  it("syncs another window and cleans up listeners and document attributes in StrictMode", () => {
    document.documentElement.setAttribute("data-theme", "prior");
    const view = render(<StrictMode><AppearanceProvider><Probe /></AppearanceProvider></StrictMode>);
    act(() => window.dispatchEvent(new StorageEvent("storage", { key, newValue: JSON.stringify({ version: 1, theme: "light", compact: true, reduceMotion: false }) })));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
    act(() => window.dispatchEvent(new StorageEvent("storage", { key: null })));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    view.unmount();
    expect([...listeners.values()].every((callbacks) => callbacks.size === 0)).toBe(true);
    expect(document.documentElement).toHaveAttribute("data-theme", "prior");
    document.documentElement.removeAttribute("data-theme");
  });
});

describe("appearance CSS contract", () => {
  it("keeps body/caption/status light-theme foregrounds at AA contrast", () => {
    const light = tokens.split(':root[data-theme="light"]')[1].split("}")[0];
    const color = (name: string) => light.match(new RegExp(`${name}: (#[a-f0-9]{6});`))![1];
    const luminance = (hex: string) => {
      const channels = [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255).map((v) => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
      return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
    };
    for (const fg of ["text", "text-muted", "text-subtle", "code-text", "accent-badge", "success", "warning", "danger"]) {
      for (const bg of ["canvas", "sidebar", "surface", "surface-hover", "surface-active", "surface-sunken", "code-bg"]) {
        expect((luminance(color(`--color-${bg}`)) + .05) / (luminance(color(`--color-${fg}`)) + .05), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("uses root density tokens and removes displacement without removing dialog centering or spinner", () => {
    expect(tokens).toContain(':root[data-density="compact"]');
    expect(primitives).toContain("min-height: var(--row-surface)");
    expect(primitives).toContain(':root[data-reduced-motion="true"] .ui-dialog { transform: translate(-50%, -50%) !important; }');
    expect(primitives).toContain("@keyframes ui-spin");
  });
});
