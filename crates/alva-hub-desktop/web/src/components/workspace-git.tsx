// INPUT: Saved Hub session and bounded real Git status/diff bridge responses
// OUTPUT: WorkspaceGit — staged/unstaged/untracked review, without mutations
// POS: Workbench Git surface, separate from product tool execution records.

import { useEffect, useState } from "react";
import { getHubGitDiff, getHubGitStatus, type GitDiffArea, type WorkspaceGitDiff, type WorkspaceGitStatus } from "../bridge";
import { useI18n } from "../i18n";
import { Button, EmptyState, ListRow, Notice, PanelSection } from "../ui";

export function WorkspaceGit({ sessionId, revision }: { sessionId: string; revision: number }) {
  const { t } = useI18n();
  const [refresh, setRefresh] = useState(0);
  const [selected, setSelected] = useState<{ path: string; area: GitDiffArea } | null>(null);
  const [status, setStatus] = useState<{ key: string; value?: WorkspaceGitStatus; error?: boolean }>();
  const [diff, setDiff] = useState<{ key: string; value?: WorkspaceGitDiff; error?: boolean }>();
  const statusKey = JSON.stringify([sessionId, revision, refresh]);
  const diffKey = JSON.stringify([statusKey, selected]);
  useEffect(() => {
    let active = true;
    void getHubGitStatus(sessionId).then((value) => {
      if (active) setStatus({ key: statusKey, value });
    }).catch(() => { if (active) setStatus({ key: statusKey, error: true }); });
    return () => { active = false; };
  }, [sessionId, statusKey]);
  useEffect(() => {
    if (!selected) return;
    let active = true;
    void getHubGitDiff(sessionId, selected.path, selected.area).then((value) => {
      if (active) setDiff({ key: diffKey, value });
    }).catch(() => { if (active) setDiff({ key: diffKey, error: true }); });
    return () => { active = false; };
  }, [sessionId, selected, diffKey]);
  const current = status?.key === statusKey ? status : undefined;
  const patch = diff?.key === diffKey ? diff : undefined;
  return <div>
    <PanelSection heading={t("git.title")}>
      <Button size="small" variant="ghost" onClick={() => setRefresh((value) => value + 1)}>{t("common.refresh")}</Button>
      {!current ? <p role="status">{t("files.loading")}</p> : current.error ? <Notice tone="warning" heading={t("git.error")} /> : !current.value?.is_repository ? <EmptyState title={t("git.notRepository")} /> : <>
        <p>{t("git.branch", { branch: current.value.branch ?? t("git.detached") })}</p>
        {current.value.hidden_paths ? <Notice heading={t("git.hidden")} /> : null}
        {current.value.files.length === 0 ? <EmptyState title={t("git.noVisibleChanges")} /> : null}
        {(["staged", "unstaged", "untracked"] as const).map((area) => <PanelSection key={area} heading={t(`git.${area}`)}>
          {current.value!.files.filter((file) => area === "untracked" ? file.index === "?" : area === "staged" ? file.index !== " " && file.index !== "?" : file.worktree !== " " && file.worktree !== "?").map((file) =>
            <ListRow key={`${area}:${file.path}`} heading={file.path} description={file.original_path ? `${file.original_path} → ${file.path}` : undefined}
              meta={area === "staged" ? file.index : file.worktree} onClick={() => setSelected({ path: file.path, area })} />)}
        </PanelSection>)}
      </>}
    </PanelSection>
    {selected ? <PanelSection heading={`${t(`git.${selected.area}`)} · ${selected.path}`}>
      {!patch ? <p role="status">{t("files.loading")}</p> : patch.error ? <Notice tone="warning" heading={t("git.error")} /> : <>
        <pre className="timeline-activity-output">{patch.value?.text || t("git.emptyDiff")}</pre>
        {patch.value?.truncated ? <Notice tone="warning" heading={t("git.limit")} /> : null}
      </>}
    </PanelSection> : null}
  </div>;
}
