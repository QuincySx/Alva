// INPUT: Explicit browser UI actions and mocked native IPC
// OUTPUT: Browser navigation, manual close/reopen and stale-open cleanup tests
// POS: Session-owned user browser lifecycle regression contract.

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { hubBrowser } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { WorkspaceBrowser } from "./workspace-browser";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), NATIVE_WORKBENCH: true, hubBrowser: vi.fn() }));
const invoke = vi.mocked(hubBrowser);
const snapshot = { url: "https://example.com/", loading: false, slow: false, blocked_navigation: false, closed: false };
const view = () => <I18nProvider><UIProvider><WorkspaceBrowser sessionId="session" /></UIProvider></I18nProvider>;
beforeEach(() => { invoke.mockReset(); localStorage.setItem("alva-hub.language", "en-US"); });
afterEach(cleanup);
function open() {
  fireEvent.change(screen.getByRole("textbox", { name: "Browser address" }), { target: { value: "https://example.com" } });
  fireEvent.click(screen.getByRole("button", { name: "Open" }));
}

it("does not load automatically and connects every control to the owned native window", async () => {
  invoke.mockImplementation(async (request) => request.action === "open" ? "browser" : request.action === "read" ? snapshot : null);
  const rendered = render(view());
  expect(invoke).not.toHaveBeenCalled();
  open();
  await screen.findByText("Native window open. Page errors, if any, appear in that window.");
  for (const [label, operation] of [["Back", "back"], ["Forward", "forward"], ["Reload", "reload"], ["Show window", "focus"]]) {
    fireEvent.click(screen.getByRole("button", { name: label }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith({ action: "control", session: "session", id: "browser", operation }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Back" })).not.toBeDisabled());
  }
  fireEvent.change(screen.getByRole("textbox", { name: "Browser address" }), { target: { value: "https://example.com/next" } });
  fireEvent.click(screen.getByRole("button", { name: "Open" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith({ action: "control", operation: "navigate", session: "session", id: "browser", url: "https://example.com/next" }));
  rendered.unmount();
  expect(invoke).toHaveBeenCalledWith({ action: "control", operation: "close", session: "session", id: "browser" });
});

it("closes an open response arriving after the session surface was unmounted", async () => {
  let finish!: (id: string) => void;
  invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  invoke.mockResolvedValue(null);
  const rendered = render(view());
  open();
  rendered.unmount();
  await act(async () => finish("late-browser"));
  expect(invoke).toHaveBeenCalledWith({ action: "control", operation: "close", session: "session", id: "late-browser" });
});

it("reports failed navigation and allows reopening after a native manual close", async () => {
  let closed = false;
  invoke.mockImplementation(async (request) => {
    if (request.action === "open") return "browser";
    if (request.action === "read") return { ...snapshot, closed };
    throw new Error("native failure");
  });
  render(view());
  open();
  await screen.findByRole("button", { name: "Reload" });
  fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  await screen.findByText("Browser operation failed. Use an HTTP(S) address and retry.");
  closed = true;
  await waitFor(() => expect(screen.queryByRole("button", { name: "Reload" })).not.toBeInTheDocument());
  closed = false;
  fireEvent.click(screen.getByRole("button", { name: "Open" }));
  await screen.findByRole("button", { name: "Reload" });
  expect(invoke.mock.calls.filter(([request]) => request.action === "open")).toHaveLength(2);
});
