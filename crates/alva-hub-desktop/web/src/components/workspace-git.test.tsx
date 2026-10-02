// INPUT: WorkspaceGit and deterministic bridge promises
// OUTPUT: Real Git vocabulary, diff loading, refresh and stale-session regressions
// POS: Workbench Git UI contract, independent of tool execution evidence.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getHubGitDiff, getHubGitStatus, type WorkspaceGitStatus } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { WorkspaceGit } from "./workspace-git";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), getHubGitStatus: vi.fn(), getHubGitDiff: vi.fn() }));
const status = vi.mocked(getHubGitStatus);
const diff = vi.mocked(getHubGitDiff);
const snapshot: WorkspaceGitStatus = { is_repository: true, branch: "topic", hidden_paths: false, files: [{ path: "file", original_path: null, index: "M", worktree: " " }] };
const view = (id = "a", revision = 1) => <I18nProvider><UIProvider><WorkspaceGit key={id} sessionId={id} revision={revision} /></UIProvider></I18nProvider>;
beforeEach(() => { status.mockReset(); diff.mockReset(); localStorage.setItem("alva-hub.language", "en-US"); });
afterEach(cleanup);

it("shows a real Git branch and staged diff without claiming tool execution", async () => {
  status.mockResolvedValue(snapshot);
  diff.mockResolvedValue({ path: "file", area: "staged", text: "+<script>inert</script>", truncated: true });
  const rendered = render(view());
  await screen.findByText("Git branch: topic");
  fireEvent.click(screen.getByText("file"));
  await screen.findByText("+<script>inert</script>");
  expect(diff).toHaveBeenCalledWith("a", "file", "staged");
  expect(rendered.container.querySelector("script")).toBeNull();
  expect(screen.getByText(/Diff limited/)).toBeVisible();
  rendered.rerender(view("a", 2));
  await act(async () => {});
  expect(status).toHaveBeenCalledTimes(2);
});

it("does not install a late old-worktree status after switching sessions", async () => {
  let finish!: (value: WorkspaceGitStatus) => void;
  status.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  status.mockResolvedValueOnce({ ...snapshot, branch: "new" });
  const rendered = render(view());
  rendered.rerender(view("b"));
  await screen.findByText("Git branch: new");
  await act(async () => finish(snapshot));
  expect(screen.queryByText("Git branch: topic")).toBeNull();
});

it("distinguishes failure from non-repository and retries", async () => {
  status.mockRejectedValueOnce(new Error("private stderr"));
  status.mockResolvedValueOnce({ is_repository: false, branch: null, files: [], hidden_paths: false });
  render(view());
  await screen.findByText(/Git inspection unavailable/);
  expect(screen.queryByText("private stderr")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByText("This workspace is not a Git worktree");
  expect(screen.queryByText("No visible changes")).toBeNull();
});
