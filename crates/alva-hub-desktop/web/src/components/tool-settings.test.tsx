// INPUT: Persisted capability/MCP bridge doubles and shared accessible UI
// OUTPUT: Real preference mutation, paused save, host authorization, diagnostics and failure/race coverage
// POS: Functional settings regression suite without external servers or paid models

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getHubToolSettings, hubToolSettingsAction, testHubMcp, type HubToolSettings, type ManagedMcpServer } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { ToolSettings } from "./tool-settings";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), getHubToolSettings: vi.fn(), hubToolSettingsAction: vi.fn(), testHubMcp: vi.fn() }));
const load = vi.mocked(getHubToolSettings);
const mutate = vi.mocked(hubToolSettingsAction);
const probe = vi.mocked(testHubMcp);
const server: ManagedMcpServer = { id: "fixture", display_name: "Fixture", command: "/usr/bin/python3", args: ["/tmp/fixture.py"], connect_timeout_secs: 5, enabled: false };
const settings: HubToolSettings = { revision: 4, components: [{ id: "shell", enabled: true, default_enabled: true }, { id: "browser", enabled: false, default_enabled: false }], mcp_servers: [server] };
const view = (sessionId: string | null = "session") => <I18nProvider><UIProvider><ToolSettings sessionId={sessionId} /></UIProvider></I18nProvider>;
beforeEach(() => { localStorage.setItem("alva-hub.language", "en-US"); load.mockReset().mockResolvedValue(settings); mutate.mockReset().mockResolvedValue({ ...settings, revision: 5 }); probe.mockReset().mockResolvedValue({ server_id: "fixture", revision: 4, ok: true, tools: ["echo"], latency_ms: 10, message: "" }); });
afterEach(cleanup);

it("persists component choices with revision and advertises ALVA-only availability", async () => {
  render(view());
  fireEvent.click(await screen.findByRole("switch", { name: "Terminal / Shell" }));
  await waitFor(() => expect(mutate).toHaveBeenCalledWith({ op: "set_component", revision: 4, id: "shell", enabled: false }));
  expect(screen.getByText(/not a security sandbox/)).toBeInTheDocument();
  expect(screen.getByText(/Claude Code, Codex and Pi/)).toBeInTheDocument();
  expect(screen.queryByRole("switch", { name: /security|approval/i })).toBeNull();
});

it("creates a disabled configuration, uses literal argument lines and never executes on save", async () => {
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Add MCP server" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Server ID" }), { target: { value: "new-server" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Display name" }), { target: { value: "New" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Absolute executable path" }), { target: { value: "npx example" } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Absolute executable path" }), { target: { value: "/usr/bin/node" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Arguments (one per line)" }), { target: { value: "/tmp/program.js\n$(literal)" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(mutate).toHaveBeenCalledWith({ op: "create_mcp", revision: 4, server: { id: "new-server", display_name: "New", command: "/usr/bin/node", args: ["/tmp/program.js", "$(literal)"], connect_timeout_secs: 30, enabled: false } }));
  expect(probe).not.toHaveBeenCalled();
});

it("retains the edit draft and original revision after a failed save", async () => {
  mutate.mockRejectedValue(new Error("settings changed; refresh"));
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Edit MCP server" }));
  expect(screen.getByRole("textbox", { name: "Server ID" })).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Display name" }), { target: { value: "Edited" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("settings changed");
  expect(screen.getByRole("textbox", { name: "Display name" })).toHaveValue("Edited");
  expect(mutate).toHaveBeenCalledWith({ op: "update_mcp", revision: 4, server: { ...server, display_name: "Edited", enabled: false } });
});

it("requires explicit host authorization for enable and connection test", async () => {
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Enable MCP" }));
  const dialog = screen.getByRole("dialog");
  expect(dialog).toHaveTextContent("outside Agent approvals");
  expect(within(dialog).getByRole("button", { name: "Enable MCP" })).toBeDisabled();
  expect(mutate).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("checkbox"));
  fireEvent.click(within(dialog).getByRole("button", { name: "Enable MCP" }));
  await waitFor(() => expect(mutate).toHaveBeenCalledWith({ op: "set_mcp_enabled", revision: 4, id: "fixture", enabled: true, confirmed: true }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
  expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Test connection" })).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Test connection" }));
  await screen.findByText("Connection test passed");
  expect(probe).toHaveBeenCalledWith("session", "fixture", 5, true);
  expect(screen.getByText("echo")).toBeInTheDocument();
  expect(screen.getByText(/one-shot test, not a live connection/)).toBeInTheDocument();
});

it("uses real disable/delete actions, confirms deletion and restores focus", async () => {
  load.mockResolvedValue({ ...settings, mcp_servers: [{ ...server, enabled: true }] });
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Disable MCP" }));
  await waitFor(() => expect(mutate).toHaveBeenCalledWith({ op: "set_mcp_enabled", revision: 4, id: "fixture", enabled: false, confirmed: false }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Delete MCP" })).not.toBeDisabled());
  const trigger = screen.getByRole("button", { name: "Delete MCP" });
  fireEvent.click(trigger);
  expect(screen.getByRole("dialog")).toHaveTextContent("not installed programs, files, history");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(trigger).toHaveFocus());
  fireEvent.click(trigger);
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete MCP" }));
  await waitFor(() => expect(mutate).toHaveBeenCalledWith({ op: "delete_mcp", revision: 5, id: "fixture" }));
});

it("requires a saved session for testing but allows paused configuration CRUD", async () => {
  render(view(null));
  expect(await screen.findByRole("button", { name: "Test connection" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Add MCP server" })).not.toBeDisabled();
  expect(probe).not.toHaveBeenCalled();
});

it("discards a stale refresh after mutation and a reply after unmount", async () => {
  let resolve!: (value: HubToolSettings) => void;
  load.mockResolvedValueOnce(settings).mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
  const next = { ...settings, revision: 5, components: settings.components.map((c) => ({ ...c, enabled: false })) };
  mutate.mockResolvedValue(next);
  const rendered = render(view());
  await screen.findByText("Fixture");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  fireEvent.click(screen.getByRole("switch", { name: "Terminal / Shell" }));
  await waitFor(() => expect(screen.getByRole("switch", { name: "Terminal / Shell" })).toHaveAttribute("aria-checked", "false"));
  await act(async () => resolve(settings));
  expect(screen.getByRole("switch", { name: "Terminal / Shell" })).toHaveAttribute("aria-checked", "false");
  load.mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  rendered.unmount();
  await act(async () => resolve(settings));
  expect(screen.queryByText("Fixture")).toBeNull();
});
