// INPUT: Session-scoped Skills bridge, typed translations, and shared UI controls
// OUTPUT: WorkspaceSkills catalog, bounded source editor, and recoverable archive/restore actions
// POS: Real workspace Skills management without remote installation or live Agent mutation

import { useEffect, useRef, useState } from "react";
import { Blocks, Plus } from "lucide-react";
import { listWorkspaceSkills, workspaceSkillAction, type WorkspaceSkill, type WorkspaceSkillCatalog, type WorkspaceSkillDocument } from "../bridge";
import { useI18n } from "../i18n";
import { Badge, Button, CardGrid, DialogContent, DialogRoot, EmptyState, FieldFrame, Notice, TextArea } from "../ui";

export function WorkspaceSkills({ sessionId, query }: { sessionId: string | null; query: string }) {
  const { t } = useI18n();
  return sessionId ? <SessionSkills key={sessionId} sessionId={sessionId} query={query} /> : <EmptyState title={t("skill.chooseSession")} description={t("skill.chooseSessionDescription")} />;
}

function SessionSkills({ sessionId, query }: { sessionId: string; query: string }) {
  const { t } = useI18n();
  const [catalog, setCatalog] = useState<WorkspaceSkillCatalog | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [editor, setEditor] = useState<{ document: WorkspaceSkillDocument | null } | null>(null);
  const [content, setContent] = useState("");
  const [confirmArchive, setConfirmArchive] = useState(false);
  const [saved, setSaved] = useState(false);
  const live = useRef(false);
  const documentRequest = useRef(0);
  const editorTrigger = useRef<HTMLElement | null>(null);
  const refreshTrigger = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    live.current = true;
    return () => { live.current = false; documentRequest.current++; };
  }, []);
  useEffect(() => {
    let active = true;
    setCatalog(null);
    void listWorkspaceSkills(sessionId).then((value) => { if (active) setCatalog(value); })
      .catch((reason) => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [sessionId, refresh]);

  async function read(skill: WorkspaceSkill, trigger: HTMLElement) {
    editorTrigger.current = trigger;
    const request = ++documentRequest.current;
    setBusy(true); setError(null); setSaved(false); setConfirmArchive(false);
    try {
      const document = await workspaceSkillAction(sessionId, { op: "read", path: skill.path });
      if (live.current && request === documentRequest.current) { setEditor({ document }); setContent(document.content); }
    } catch (reason) { if (live.current && request === documentRequest.current) setError(String(reason)); }
    finally { if (live.current && request === documentRequest.current) setBusy(false); }
  }

  async function mutate(op: "save" | "archive" | "restore") {
    if (!editor) return;
    const request = ++documentRequest.current;
    setBusy(true); setError(null); setSaved(false);
    const document = editor.document;
    try {
      const action = !document ? { op: "create" as const, content } : op === "save"
        ? { op, path: document.path, content, revision: document.revision }
        : { op, path: document.path, revision: document.revision };
      const next = await workspaceSkillAction(sessionId, action);
      if (!live.current || request !== documentRequest.current) return;
      if (op === "save") { setEditor({ document: next }); setContent(next.content); setSaved(true); }
      else { setEditor(null); setConfirmArchive(false); }
      setRefresh((value) => value + 1);
    } catch (reason) { if (live.current && request === documentRequest.current) setError(String(reason)); }
    finally { if (live.current && request === documentRequest.current) setBusy(false); }
  }

  const visible = catalog?.skills.filter((skill) => `${skill.name} ${skill.description} ${skill.path}`.toLowerCase().includes(query.trim().toLowerCase())) ?? [];
  const selected = editor?.document;
  return <section className="hub-product-section">
    <Notice tone="info" heading={t("skill.scopeTitle")} description={t("skill.scopeDescription")} />
    <div className="hub-product-toolbar">
      <Button disabled={busy || !catalog || catalog.truncated} leadingIcon={<Plus size={15} />} onClick={(event) => { editorTrigger.current = event.currentTarget; setEditor({ document: null }); setContent(t("skill.template")); setConfirmArchive(false); setError(null); setSaved(false); }}>{t("skill.new")}</Button>
      <Button ref={refreshTrigger} disabled={busy} onClick={() => { setError(null); setRefresh((value) => value + 1); }} variant="ghost">{t("common.refresh")}</Button>
    </div>
    {error && !editor ? <Notice tone="warning" role="alert" heading={t("skill.requestFailed")} description={error} /> : null}
    {!catalog && !error ? <p role="status">{t("skill.loading")}</p> : null}
    {catalog?.warnings ? <Notice tone="warning" heading={t("skill.warnings", { count: catalog.warnings })} /> : null}
    {catalog?.truncated ? <Notice tone="warning" heading={t("skill.limit")} /> : null}
    {catalog && visible.length === 0 ? <EmptyState title={t("skill.empty")} description={t("skill.emptyDescription")} /> : null}
    <CardGrid>
      {visible.map((skill) => <article className="hub-extension-card hub-skill-card" key={skill.path}>
        <div className="hub-extension-card__heading"><span><Blocks size={18} aria-hidden="true" /></span><div><strong>{skill.name}</strong><small>{skill.path}</small></div><Badge tone={skill.archived ? "neutral" : skill.enabled === true ? "success" : "warning"}>{skill.archived ? t("skill.archived") : skill.enabled === true ? t("skill.enabled") : skill.enabled === false ? t("skill.disabled") : t("skill.unknown")}</Badge></div>
        <p>{skill.description}</p>
        <div className="hub-extension-card__actions">
          <Badge>{skill.editable ? t("skill.managed") : skill.archived ? t("skill.archived") : t("skill.readonly")}</Badge>
          <Button disabled={busy} onClick={(event) => void read(skill, event.currentTarget)} size="small" variant="ghost">{t("skill.open", { name: skill.name })}</Button>
        </div>
      </article>)}
    </CardGrid>
    <DialogRoot open={editor !== null} onOpenChange={(open) => { if (!open && !busy) { setEditor(null); setConfirmArchive(false); documentRequest.current++; } }}>
      <DialogContent size="wide" title={selected?.name ?? t("skill.new")} description={selected?.path ?? t("skill.createDescription")} onCloseAutoFocus={(event) => { event.preventDefault(); if (editorTrigger.current?.isConnected) editorTrigger.current.focus(); else if (refreshTrigger.current?.isConnected) refreshTrigger.current.focus(); }}>
        <div className="hub-form-stack">
          {error ? <Notice role="alert" tone="warning" heading={t("skill.requestFailed")} description={error} /> : null}
          {saved ? <Notice role="status" tone="info" heading={t("skill.saved")} /> : null}
          <FieldFrame label={t("skill.source")} description={t("skill.sourceDescription")}>
            <TextArea aria-label={t("skill.source")} value={content} rows={16} readOnly={!!selected && !selected.editable} disabled={busy} onChange={(event) => { setContent(event.target.value); setSaved(false); }} />
          </FieldFrame>
          {confirmArchive ? <Notice tone="warning" heading={t("skill.archiveConfirm")} description={t("skill.archiveDescription")} actions={<><Button disabled={busy} onClick={() => setConfirmArchive(false)} variant="ghost">{t("common.cancel")}</Button><Button disabled={busy} onClick={() => void mutate("archive")} variant="danger">{t("skill.archive")}</Button></>} /> : null}
          <div className="hub-dialog-actions">
            {selected?.editable && selected.can_move && !confirmArchive ? <Button disabled={busy} onClick={() => setConfirmArchive(true)} variant="ghost">{t("skill.archive")}</Button> : null}
            {selected?.archived && selected.can_move ? <Button disabled={busy} onClick={() => void mutate("restore")}>{t("skill.restore")}</Button> : null}
            <Button disabled={busy} onClick={() => { setEditor(null); setError(null); setConfirmArchive(false); }} variant="ghost">{t("common.close")}</Button>
            {!selected || selected.editable ? <Button disabled={busy || !content.trim() || (selected?.content === content)} onClick={() => void mutate("save")} variant="primary">{busy ? t("common.saving") : selected ? t("common.save") : t("common.create")}</Button> : null}
          </div>
        </div>
      </DialogContent>
    </DialogRoot>
  </section>;
}
