// INPUT: Explicit terminal UI actions and mocked native IPC
// OUTPUT: No-auto-execution, control input, stale-open cleanup and close tests
// POS: Desktop terminal lifecycle contract.

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { hubTerminal } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { WorkspaceTerminal } from "./workspace-terminal";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), NATIVE_WORKBENCH: true, hubTerminal: vi.fn() }));
const invoke = vi.mocked(hubTerminal);
const view = () => <I18nProvider><UIProvider><WorkspaceTerminal sessionId="session" /></UIProvider></I18nProvider>;
beforeEach(() => { invoke.mockReset(); localStorage.setItem("alva-hub.language", "en-US"); });
afterEach(cleanup);

it("starts only on explicit click and sends real input, interrupt and close actions", async () => {
  invoke.mockImplementation(async (request) => request.action === "open" ? "pty" : request.action === "read" ? { output: "native output", exited: false, exit_code: null, dropped_bytes: 0 } : null);
  const rendered = render(view());
  expect(invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Open terminal" }));
  await screen.findByText("native output");
  fireEvent.change(screen.getByRole("textbox", { name: "Command input" }), { target: { value: "pwd" } });
  fireEvent.click(screen.getByRole("button", { name: "Send command" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith(expect.objectContaining({ action: "write", id: "pty", input: "pwd\n" })));
  await waitFor(() => expect(screen.getByRole("button", { name: "Ctrl-C" })).not.toBeDisabled());
  fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith(expect.objectContaining({ action: "write", input: "\u0003" })));
  rendered.unmount();
  expect(invoke).toHaveBeenCalledWith({ action: "close", session: "session", id: "pty" });
});

it("closes an open response arriving after unmount instead of leaking a PTY", async () => {
  let finish!: (id: string) => void;
  invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  invoke.mockResolvedValue(null);
  const rendered = render(view());
  fireEvent.click(screen.getByRole("button", { name: "Open terminal" }));
  rendered.unmount();
  await act(async () => finish("late-pty"));
  expect(invoke).toHaveBeenCalledWith({ action: "close", session: "session", id: "late-pty" });
});
