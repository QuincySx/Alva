// INPUT: Durable automation DTO doubles and saved session fixtures
// OUTPUT: Paused creation/edit, billing confirmation, real CRUD, focus and stale-response regression coverage
// POS: Local schedule UI contract without timers launching paid engines

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { automationAction, listAutomations, type Automation, type HubSessionSummary } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { AutomationsPage } from "./automations";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), listAutomations: vi.fn(), automationAction: vi.fn() }));
const list = vi.mocked(listAutomations);
const action = vi.mocked(automationAction);
const session: HubSessionSummary = { product_session_id: "s", workspace: "/tmp/test", workspace_kind: "project", active_branch_id: "b", engine_id: "codex", model_id: "test-model", provider_id: "codex-cli", engine_session_id: null, status: "completed", updated_at: 1, last_prompt: "Review project", last_result: "done", permission_profile: "workspace_write", include_full_transcript: false, project_id: null };
const row: Automation = { automation_id: "a", product_session_id: "s", title: "Audit", prompt: "inspect only", interval_minutes: 10, enabled: false, revision: 4, branch_id: "b", target: { engine_id: "codex", model_id: "test-model", provider_id: "codex-cli", account_source: "Synthetic CLI account", billing_route: "Synthetic billing", automatic_use_authorized: false, required_capabilities: [], reasoning_effort: null, fast_mode: false }, next_run_at: null, last_run_at: null, last_status: "never", last_error: null, running_turn_id: null };
const view = (sessions: HubSessionSummary[] = [session], onOpenSession?: (id: string) => void) => <I18nProvider><UIProvider><AutomationsPage sessions={sessions} activeSessionId="s" onOpenSession={onOpenSession} /></UIProvider></I18nProvider>;
beforeEach(() => { localStorage.setItem("alva-hub.language", "en-US"); list.mockReset(); action.mockReset(); list.mockResolvedValue([row]); action.mockResolvedValue(row); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

it("requires saved sessions and advertises local interval limits, not a daemon", async () => {
  render(view([]));
  await screen.findByText("Audit");
  expect(screen.getByRole("button", { name: "New automation" })).toBeDisabled();
  expect(screen.getByText(/Only runs while Desktop/)).toBeInTheDocument();
  expect(screen.getByText("Choose a saved ordinary session first")).toBeInTheDocument();
  expect(action).not.toHaveBeenCalled();
});

it("creates a paused schedule and templates only prefill a real form", async () => {
  list.mockResolvedValue([]);
  render(view());
  await screen.findByText("No local automations yet");
  fireEvent.click(screen.getAllByRole("button", { name: "Use template" })[0]);
  expect(action).not.toHaveBeenCalled();
  expect(screen.getByRole("textbox", { name: "Prompt" })).toHaveValue("Run the project's trusted tests and report failures and verification. Do not change source files.");
  fireEvent.change(screen.getByRole("textbox", { name: "Title" }), { target: { value: "Tests" } });
  fireEvent.change(screen.getByRole("spinbutton", { name: "Interval (minutes)" }), { target: { value: "4" } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  fireEvent.change(screen.getByRole("spinbutton", { name: "Interval (minutes)" }), { target: { value: "10" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(action).toHaveBeenCalledWith(expect.objectContaining({ op: "create", title: "Tests", product_session_id: "s", interval_minutes: 10 })));
  expect(action.mock.calls[0][0]).not.toHaveProperty("enabled");
});

it("requires explicit account and billing review before enabling", async () => {
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Enable…" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("Synthetic CLI account");
  expect(screen.getByRole("dialog")).toHaveTextContent("Synthetic billing");
  expect(screen.getByRole("dialog")).toHaveTextContent("workspace_write");
  expect(screen.getByRole("button", { name: "Enable scheduled runs" })).toBeDisabled();
  expect(action).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Enable scheduled runs" }));
  await waitFor(() => expect(action).toHaveBeenCalledWith({ op: "set_enabled", automation_id: "a", revision: 4, enabled: true }));
});

it("edits with revision, retains failed drafts and does not pretend save succeeded", async () => {
  action.mockRejectedValue(new Error("changed; refresh"));
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Edit automation" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), { target: { value: "edited" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("changed; refresh");
  expect(screen.getByRole("textbox", { name: "Prompt" })).toHaveValue("edited");
  expect(action).toHaveBeenCalledWith({ op: "update", automation_id: "a", revision: 4, title: "Audit", prompt: "edited", interval_minutes: 10 });
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});

it("pauses even a running task but blocks edit/delete and opens its own session", async () => {
  list.mockResolvedValue([{ ...row, enabled: true, running_turn_id: "turn", last_status: "running" }]);
  const open = vi.fn();
  render(view([session], open));
  fireEvent.click(await screen.findByRole("button", { name: "Open session" }));
  expect(open).toHaveBeenCalledWith("s");
  expect(screen.getByRole("button", { name: "Edit automation" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Delete automation" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Pause" }));
  await waitFor(() => expect(action).toHaveBeenCalledWith({ op: "set_enabled", automation_id: "a", revision: 4, enabled: false }));
});

it("deletes only after confirmation and restores focus when cancelled", async () => {
  render(view());
  const trigger = await screen.findByRole("button", { name: "Delete automation" });
  fireEvent.click(trigger);
  expect(screen.getByRole("dialog")).toHaveTextContent("Conversation history and workspace files are kept");
  expect(action).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(trigger).toHaveFocus());
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("button", { name: "Delete automation" }));
  await waitFor(() => expect(action).toHaveBeenCalledWith({ op: "delete", automation_id: "a", revision: 4 }));
});

it("cleans up polling and ignores a late reply after unmount", async () => {
  let resolve!: (rows: Automation[]) => void;
  list.mockReturnValue(new Promise((done) => { resolve = done; }));
  const rendered = render(view());
  rendered.unmount();
  await act(async () => resolve([row]));
  expect(screen.queryByText("Audit")).toBeNull();
  expect(action).not.toHaveBeenCalled();
});

it("a stale catalog reply cannot overwrite a newly authorized schedule", async () => {
  let resolve!: (rows: Automation[]) => void;
  const enabled = { ...row, enabled: true, revision: 5, next_run_at: 500_000 };
  list.mockResolvedValue([enabled]).mockResolvedValueOnce([row]).mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
  action.mockResolvedValue(enabled);
  render(view());
  await screen.findByText("Audit");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("button", { name: "Enable…" }));
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Enable scheduled runs" }));
  await screen.findByText("Enabled");
  await act(async () => resolve([row]));
  expect(screen.getByText("Enabled")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
});
