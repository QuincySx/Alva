// INPUT: WorkspaceSkills, session-scoped bridge doubles, and deterministic translations
// OUTPUT: Creation/edit/archive/restore, errors, safe text and stale-response regression coverage
// POS: Real Skills controls acceptance without downloads, globals or engine calls

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { listWorkspaceSkills, workspaceSkillAction, type WorkspaceSkillCatalog, type WorkspaceSkillDocument } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { WorkspaceSkills } from "./workspace-skills";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), listWorkspaceSkills: vi.fn(), workspaceSkillAction: vi.fn() }));
const list = vi.mocked(listWorkspaceSkills);
const action = vi.mocked(workspaceSkillAction);
const document: WorkspaceSkillDocument = { name: "review", description: "Review changes", path: ".alva/skills/user/review/SKILL.md", enabled: true, editable: true, archived: false, can_move: true, revision: "rev1", content: "---\nname: review\ndescription: Review changes\n---\n<script>inert</script>" };
const catalog = (docs: WorkspaceSkillDocument[] = [document]): WorkspaceSkillCatalog => ({ skills: docs, warnings: 0, truncated: false });
const view = (sessionId: string | null = "a", query = "") => <I18nProvider><UIProvider><WorkspaceSkills sessionId={sessionId} query={query} /></UIProvider></I18nProvider>;
beforeEach(() => { list.mockReset(); action.mockReset(); list.mockResolvedValue(catalog()); action.mockResolvedValue(document); localStorage.setItem("alva-hub.language", "en-US"); });
afterEach(cleanup);

it("requires a saved session and filters actual catalog data, never samples", async () => {
  const rendered = render(view(null));
  expect(screen.getByText("Choose a saved session first")).toBeInTheDocument();
  expect(list).not.toHaveBeenCalled();
  rendered.rerender(view("a"));
  expect(await screen.findByText("review")).toBeInTheDocument();
  expect(list).toHaveBeenCalledWith("a");
  expect(screen.getByText("Enabled in project")).toBeInTheDocument();
  rendered.rerender(view("a", "missing"));
  expect(screen.getByText("No matching workspace Skills")).toBeInTheDocument();
  expect(screen.queryByText("review")).toBeNull();
});

it("creates a real source and retains the returned revision for subsequent edits", async () => {
  list.mockResolvedValue(catalog([]));
  render(view());
  await screen.findByText("No matching workspace Skills");
  fireEvent.click(screen.getByRole("button", { name: "New Skill" }));
  fireEvent.change(screen.getByRole("textbox", { name: "SKILL.md source" }), { target: { value: document.content } });
  fireEvent.click(screen.getByRole("button", { name: "Create" }));
  await screen.findByText(/Saved on disk/);
  expect(action).toHaveBeenCalledWith("a", { op: "create", content: document.content });
  fireEvent.change(screen.getByRole("textbox", { name: "SKILL.md source" }), { target: { value: document.content + "Edit" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(action).toHaveBeenLastCalledWith("a", { op: "save", path: document.path, content: document.content + "Edit", revision: "rev1" }));
});

it("keeps raw source inert and returns keyboard focus after closing", async () => {
  const { container } = render(view());
  const trigger = await screen.findByRole("button", { name: "Open review" });
  fireEvent.click(trigger);
  const editor = await screen.findByRole("textbox", { name: "SKILL.md source" });
  expect(editor).toHaveValue(document.content);
  expect(container.querySelector("script")).toBeNull();
  fireEvent.click(screen.getAllByRole("button", { name: "Close" })[1]);
  await waitFor(() => expect(trigger).toHaveFocus());
});

it("requires explicit archive confirmation and restores using the archived path and revision", async () => {
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Open review" }));
  fireEvent.click(await screen.findByRole("button", { name: "Archive Skill" }));
  expect(screen.getByText("Archive this project Skill?")).toBeInTheDocument();
  expect(action).toHaveBeenCalledTimes(1);
  const archived = { ...document, path: ".alva/skills/archive/uuid/SKILL.md", editable: false, archived: true, enabled: false };
  action.mockResolvedValue(archived);
  list.mockResolvedValue(catalog([archived]));
  fireEvent.click(screen.getByRole("button", { name: "Archive Skill" }));
  await waitFor(() => expect(action).toHaveBeenLastCalledWith("a", { op: "archive", path: document.path, revision: "rev1" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(await screen.findByRole("button", { name: "Open review" }));
  fireEvent.click(await screen.findByRole("button", { name: "Restore Skill" }));
  await waitFor(() => expect(action).toHaveBeenLastCalledWith("a", { op: "restore", path: archived.path, revision: "rev1" }));
});

it("readonly sources cannot be edited or archived and save errors never show success", async () => {
  action.mockResolvedValueOnce({ ...document, editable: false, path: ".agents/skills/review/SKILL.md" });
  render(view());
  fireEvent.click(await screen.findByRole("button", { name: "Open review" }));
  expect(await screen.findByRole("textbox", { name: "SKILL.md source" })).toHaveAttribute("readonly");
  expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Archive Skill" })).toBeNull();
  fireEvent.click(screen.getAllByRole("button", { name: "Close" })[0]);
  fireEvent.click(await screen.findByRole("button", { name: "Open review" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "SKILL.md source" }), { target: { value: "draft" } });
  action.mockRejectedValueOnce(new Error("Skill changed on disk; reload"));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByRole("alert");
  expect(screen.getByRole("textbox", { name: "SKILL.md source" })).toHaveValue("draft");
  expect(screen.queryByText(/Saved on disk/)).toBeNull();
});

it("rejects late catalog and document replies after switching sessions", async () => {
  let finishList!: (value: WorkspaceSkillCatalog) => void;
  list.mockImplementationOnce(() => new Promise((resolve) => { finishList = resolve; }));
  const rendered = render(view());
  rendered.rerender(view("b"));
  await screen.findByText("review");
  await act(async () => finishList(catalog([{ ...document, name: "old" }])));
  expect(screen.queryByText("old")).toBeNull();
  let finishDocument!: (value: WorkspaceSkillDocument) => void;
  action.mockImplementationOnce(() => new Promise((resolve) => { finishDocument = resolve; }));
  fireEvent.click(screen.getByRole("button", { name: "Open review" }));
  rendered.rerender(view("c"));
  await screen.findByText("review");
  await act(async () => finishDocument({ ...document, name: "old" }));
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("retries a failed catalog and reports partial scans without enabling Create", async () => {
  list.mockRejectedValueOnce(new Error("unavailable"));
  list.mockResolvedValue({ ...catalog(), warnings: 2, truncated: true });
  render(view());
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByText("review");
  expect(screen.getByText(/2 entries/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "New Skill" })).toBeDisabled();
});
