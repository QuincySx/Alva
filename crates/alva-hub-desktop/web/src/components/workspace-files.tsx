// INPUT: Persisted Hub session identity and bounded workspace bridge DTOs
// OUTPUT: WorkspaceFiles — real read-only directory navigation and text preview
// POS: Workbench Files/Text domain surface; no host paths authorized by the UI.

import { useEffect, useState } from "react";
import { FileCode2, FolderTree } from "lucide-react";
import { listHubWorkspaceFiles, readHubWorkspaceFile, type WorkspaceDirectory, type WorkspacePreview } from "../bridge";
import { useI18n } from "../i18n";
import { Button, EmptyState, ListRow, Notice, PanelSection } from "../ui";

export function WorkspaceFiles({ sessionId }: { sessionId: string }) {
  const { t } = useI18n();
  const [path, setPath] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [directory, setDirectory] = useState<{ key: string; value?: WorkspaceDirectory; error?: boolean }>();
  const [preview, setPreview] = useState<{ key: string; value?: WorkspacePreview; error?: boolean }>();
  const directoryKey = JSON.stringify([sessionId, path, refresh]);
  const previewKey = JSON.stringify([sessionId, selected, refresh]);
  useEffect(() => {
    let active = true;
    void listHubWorkspaceFiles(sessionId, path).then((value) => {
      if (active) setDirectory({ key: directoryKey, value });
    }).catch(() => { if (active) setDirectory({ key: directoryKey, error: true }); });
    return () => { active = false; };
  }, [sessionId, path, directoryKey]);
  useEffect(() => {
    if (!selected) return;
    let active = true;
    void readHubWorkspaceFile(sessionId, selected).then((value) => {
      if (active) setPreview({ key: previewKey, value });
    }).catch(() => { if (active) setPreview({ key: previewKey, error: true }); });
    return () => { active = false; };
  }, [sessionId, selected, previewKey]);
  const current = directory?.key === directoryKey ? directory : undefined;
  const file = preview?.key === previewKey ? preview : undefined;
  function navigate(next: string) { setPath(next); setSelected(null); }
  return <div className="hub-text-browser">
    <PanelSection heading={t("files.title")} description={path || "/"}>
      <Button size="small" variant="ghost" disabled={!path} onClick={() => navigate(path.split("/").slice(0, -1).join("/"))}>{t("common.back")}</Button>
      <Button size="small" variant="ghost" onClick={() => setRefresh((value) => value + 1)}>{t("common.refresh")}</Button>
      <small>{t("files.protected")}</small>
      {!current ? <p role="status">{t("files.loading")}</p> : current.error ? <Notice tone="warning" heading={t("files.error")} /> : <>
        {current.value?.entries.length === 0 ? <EmptyState title={t("files.empty")} /> : null}
        {current.value?.entries.map((entry) => <ListRow key={entry.path} heading={entry.name}
          leading={entry.kind === "directory" ? <FolderTree size={16} /> : <FileCode2 size={16} />}
          description={entry.kind === "symlink" || entry.kind === "unavailable" ? t("files.blocked") : undefined}
          onClick={entry.kind === "directory" ? () => navigate(entry.path) : entry.kind === "file" ? () => setSelected(entry.path) : undefined} />)}
        {current.value?.truncated ? <Notice tone="warning" heading={t("files.listLimit")} /> : null}
      </>}
    </PanelSection>
    {selected ? <PanelSection heading={selected}>
      {!file ? <p role="status">{t("files.loading")}</p> : file.error ? <Notice tone="warning" heading={t("files.error")} /> : <>
        <small>{t("files.bytes", { bytes: file.value?.total_bytes ?? 0 })}</small>
        {file.value?.kind === "binary" ? <EmptyState title={t("files.binary")} /> : <pre className="timeline-activity-output">{file.value?.text}</pre>}
        {file.value?.truncated ? <Notice tone="warning" heading={t("files.previewLimit")} /> : null}
      </>}
    </PanelSection> : null}
  </div>;
}
