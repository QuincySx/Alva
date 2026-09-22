// INPUT:  persisted projects (H04), read-only worktree discovery states (H05),
//         mode/entry callbacks
// OUTPUT: StartupGate — the two FIRST-CLASS entry modes (conversation /
//         project) with project aggregation before any session exists
// POS:    H08. Conversation mode never requires a repository (it creates a
//         managed independent session directory); project mode aggregates a
//         working directory, its worktrees (read-only discovery states, not
//         worker registrations) and its sessions. Switching modes never
//         migrates sessions or widens permissions — it only changes which
//         sessions are listed and which directory the next session binds to.

import { FolderKanban, FolderOpen, MessageSquare, Plus, RefreshCw, Trash2 } from "lucide-react";
import type { ProjectSummary, WorktreeDiscoveryState, WorktreeInfo } from "../bridge";
import { useI18n } from "../i18n";
import { Badge, Button, EmptyState, Notice, Surface } from "../ui";

export type EntryMode = "conversation" | "project";

export interface ProjectWorktreeState {
  worktrees: WorktreeInfo[];
  state: WorktreeDiscoveryState | null;
}

export interface StartupGateProps {
  busy: boolean;
  entryMode: EntryMode;
  projects: ProjectSummary[];
  projectWorktrees: Record<string, ProjectWorktreeState>;
  projectsSupported: boolean;
  onEntryModeChange: (mode: EntryMode) => void;
  onOpenProject: (project: ProjectSummary, worktreePath?: string) => void;
  onAddProject: () => void;
  onDeleteProject: (project: ProjectSummary) => void;
  onRefreshProjects: () => void;
  onCreateConversation: () => void;
}

function describeWorktreeState(
  state: ProjectWorktreeState | undefined,
  locale: string,
): string | null {
  if (!state) return null;
  if (state.state) {
    if (state.state.code === "not_git_repository") {
      return locale === "zh-CN" ? "非 Git 目录（仍可作为项目）" : "Not a Git repository (still usable as a project)";
    }
    if (state.state.code === "missing_directory") {
      return locale === "zh-CN" ? "目录缺失，需要修复" : "Directory missing — repair needed";
    }
    return locale === "zh-CN" ? `Git 发现失败：${state.state.detail}` : `Git discovery failed: ${state.state.detail}`;
  }
  const user = state.worktrees.filter((worktree) => !worktree.managed_by_hub);
  const managed = state.worktrees.length - user.length;
  const missing = state.worktrees.filter((worktree) => !worktree.exists_on_disk).length;
  if (locale === "zh-CN") {
    const parts = [`${user.length} 个工作树`];
    if (managed > 0) parts.push(`${managed} 个 Hub 受管`);
    if (missing > 0) parts.push(`${missing} 个目录缺失`);
    return parts.join(" · ");
  }
  const parts = [`${user.length} worktree${user.length === 1 ? "" : "s"}`];
  if (managed > 0) parts.push(`${managed} Hub-managed`);
  if (missing > 0) parts.push(`${missing} missing dir${missing === 1 ? "" : "s"}`);
  return parts.join(" · ");
}

export function StartupGate({
  busy,
  entryMode,
  projects,
  projectWorktrees,
  projectsSupported,
  onEntryModeChange,
  onOpenProject,
  onAddProject,
  onDeleteProject,
  onRefreshProjects,
  onCreateConversation,
}: StartupGateProps) {
  const { locale, t } = useI18n();
  return (
    <section className="hub-workspace-gate" aria-labelledby="workspace-gate-title">
      <header className="hub-workspace-gate__header">
        <h2 id="workspace-gate-title">{t("startup.title")}</h2>
        <p>{t("startup.description")}</p>
      </header>

      <div className="hub-workspace-gate__options" role="tablist" aria-label={t("startup.modesLabel")}>
        <Surface
          className="hub-workspace-gate__card"
          tone={entryMode === "conversation" ? "raised" : "base"}
        >
          <div className="hub-workspace-gate__icon">
            <MessageSquare aria-hidden="true" size={20} />
          </div>
          <div className="hub-workspace-gate__title">
            <h3>{t("startup.conversationMode")}</h3>
          </div>
          <p>{t("startup.conversationModeDescription")}</p>
          <Button
            disabled={busy}
            onClick={() => {
              onEntryModeChange("conversation");
              onCreateConversation();
            }}
            size="large"
            variant={entryMode === "conversation" ? "primary" : "secondary"}
          >
            {busy ? t("workspace.creatingSession") : t("startup.startConversation")}
          </Button>
        </Surface>
        <Surface
          className="hub-workspace-gate__card"
          tone={entryMode === "project" ? "raised" : "base"}
        >
          <div className="hub-workspace-gate__icon">
            <FolderKanban aria-hidden="true" size={20} />
          </div>
          <div className="hub-workspace-gate__title">
            <h3>{t("startup.projectMode")}</h3>
            <Badge tone="accent">{t("workspace.recommended")}</Badge>
          </div>
          <p>{t("startup.projectModeDescription")}</p>
          <Button
            disabled={busy || !projectsSupported}
            onClick={() => onEntryModeChange("project")}
            size="large"
            variant={entryMode === "project" ? "primary" : "secondary"}
          >
            {t("startup.browseProjects")}
          </Button>
        </Surface>
      </div>

      {entryMode === "project" ? (
        <div className="hub-startup-projects">
          <div className="hub-startup-projects__toolbar">
            <h3>{t("startup.projectsHeading")}</h3>
            <div style={{ display: "flex", gap: "8px" }}>
              <Button
                aria-label={t("startup.refreshProjects")}
                disabled={busy || !projectsSupported}
                onClick={onRefreshProjects}
                size="small"
                variant="ghost"
              >
                <RefreshCw aria-hidden="true" size={14} />
                {t("startup.refreshProjects")}
              </Button>
              <Button
                disabled={busy || !projectsSupported}
                onClick={onAddProject}
                size="small"
                variant="primary"
              >
                <Plus aria-hidden="true" size={14} />
                {t("startup.addProject")}
              </Button>
            </div>
          </div>
          {!projectsSupported ? (
            <Notice tone="info" heading={t("startup.unsupportedTitle")}>
              {t("startup.unsupportedBody")}
            </Notice>
          ) : projects.length === 0 ? (
            <EmptyState
              description={t("startup.emptyProjects")}
              icon={<FolderOpen aria-hidden="true" size={18} />}
              title={t("startup.emptyProjectsTitle")}
            />
          ) : (
            <ul className="hub-startup-projects__list">
              {projects.map((project) => {
                const worktreeLine = describeWorktreeState(projectWorktrees[project.project_id], locale);
                return (
                  <li className="hub-startup-projects__row" key={project.project_id}>
                    <div className="hub-startup-projects__info">
                      <strong>{project.display_name}</strong>
                      <code>{project.workspace_path}</code>
                      <small>
                        {t("startup.sessionCount", { count: project.session_count })}
                        {worktreeLine ? ` · ${worktreeLine}` : ""}
                      </small>
                      <ul aria-label={t("startup.worktrees")}>
                        {(projectWorktrees[project.project_id]?.worktrees ?? []).map((tree) => (
                          <li key={tree.path}>
                            <Button size="small" variant="ghost"
                              disabled={busy || !tree.exists_on_disk || tree.bare || tree.prunable !== null}
                              onClick={() => onOpenProject(project, tree.path)}>
                              {tree.branch?.replace(/^refs\/heads\//, "") ?? tree.head?.slice(0, 8) ?? t("startup.worktree")}
                              {tree.managed_by_hub ? " · Hub" : ""}
                            </Button>
                            <code>{tree.path}</code>
                            {!tree.exists_on_disk ? <small>{t("startup.missingWorktree")}</small> : null}
                          </li>
                        ))}
                      </ul>
                    </div>
                    <div className="hub-startup-projects__actions">
                      <Button
                        disabled={busy}
                        onClick={() => onOpenProject(project)}
                        size="small"
                        variant="primary"
                      >
                        <FolderOpen aria-hidden="true" size={14} />
                        {t("startup.openProject")}
                      </Button>
                      <Button
                        aria-label={t("startup.deleteProject", { name: project.display_name })}
                        disabled={busy}
                        onClick={() => onDeleteProject(project)}
                        size="small"
                        variant="ghost"
                      >
                        <Trash2 aria-hidden="true" size={14} />
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      ) : null}
    </section>
  );
}
