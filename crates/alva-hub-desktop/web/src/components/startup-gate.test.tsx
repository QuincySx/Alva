// INPUT: StartupGate, persisted project and discovered worktree fixtures
// OUTPUT: Worktree selection and unavailable-directory regression tests
// POS: Project startup must bind the chosen worktree, never silently the main root
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import type { ProjectSummary, WorktreeInfo } from "../bridge";
import { StartupGate } from "./startup-gate";

afterEach(cleanup);

it("opens the selected valid worktree and disables unavailable worktrees", () => {
  const project: ProjectSummary = { project_id: "p1", display_name: "Project",
    workspace_path: "/repo", workspace_canonical: "/repo", created_at: 1, updated_at: 1, session_count: 0 };
  const tree: WorktreeInfo = { path: "/linked", head: "abc123", branch: "refs/heads/feature",
    detached: false, bare: false, is_main_worktree: false, locked: null, prunable: null,
    managed_by_hub: false, exists_on_disk: true };
  const open = vi.fn();
  render(<UIProvider><I18nProvider><StartupGate busy={false} entryMode="project"
    projects={[project]} projectsSupported
    projectWorktrees={{ p1: { state: null, worktrees: [tree,
      { ...tree, path: "/missing", branch: "refs/heads/missing", exists_on_disk: false },
      { ...tree, path: "/bare", branch: "refs/heads/bare", bare: true },
      { ...tree, path: "/prunable", branch: "refs/heads/prunable", prunable: "missing" },
    ] } }}
    onOpenProject={open} onEntryModeChange={() => {}} onAddProject={() => {}}
    onDeleteProject={() => {}} onRefreshProjects={() => {}} onCreateConversation={() => {}}
  /></I18nProvider></UIProvider>);
  fireEvent.click(screen.getByRole("button", { name: "feature" }));
  expect(open).toHaveBeenCalledWith(project, "/linked");
  for (const name of ["missing", "bare", "prunable"]) {
    const button = screen.getByRole("button", { name });
    expect(button).toBeDisabled();
    fireEvent.click(button);
  }
  expect(open).toHaveBeenCalledTimes(1);
});
