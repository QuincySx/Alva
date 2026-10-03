// INPUT: Session-owned attachment references and read-only paged media/receipt bridge
// OUTPUT: AttachmentMedia inspection dialog with explicit derivative previews
// POS: Message-domain media evidence; inspection never processes media or sends model requests.
import { useEffect, useRef, useState } from "react";
import { getHubMediaArtifact, getHubMediaReceipts, listHubAttachmentMedia,
  type InputAttachmentRef, type MediaArtifactContent, type MediaArtifactPage, type MediaArtifactRef, type MediaReceiptPage } from "../bridge";
import { useI18n } from "../i18n";
import { Button, DialogContent, DialogRoot } from "../ui";
import { AttachmentPreview, attachmentBytes } from "./attachment-preview";

const STATUS = {
  prepared: "media.prepared", submitted: "media.submitted", response_received: "media.response",
  provider_error_acceptance_unknown: "media.unknown", interrupted_unknown: "media.unknown",
  not_submitted_cancelled: "media.cancelled", not_submitted_before_run_end: "media.notSubmitted",
  not_submitted_interrupted: "media.notSubmitted",
} as const;

export function AttachmentMedia({ sessionId, file }: { sessionId: string; file: InputAttachmentRef }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement | null>(null);
  return <><Button ref={trigger} size="small" variant="ghost" aria-label={t("media.inspect", { name: file.name })}
    onClick={() => setOpen(true)}>{t("media.title")}</Button>
    <DialogRoot open={open} onOpenChange={setOpen}>
      <DialogContent title={`${file.name} · ${t("media.title")}`} description={t("media.readOnly")} size="wide"
        onCloseAutoFocus={(event) => { event.preventDefault(); trigger.current?.focus(); }}>
        {open ? <MediaIndex key={`${sessionId}:${file.attachment_id}`} sessionId={sessionId} attachmentId={file.attachment_id} /> : null}
      </DialogContent>
    </DialogRoot></>;
}

function MediaIndex({ sessionId, attachmentId }: { sessionId: string; attachmentId: string }) {
  const { t } = useI18n();
  const [pages, setPages] = useState([0]);
  const after = pages[pages.length - 1];
  const [revision, setRevision] = useState(0);
  const [page, setPage] = useState<MediaArtifactPage>();
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<MediaArtifactRef>();
  useEffect(() => {
    let active = true;
    setPage(undefined); setError(false); setSelected(undefined);
    void listHubAttachmentMedia(sessionId, attachmentId, after).then((value) => {
      if (value.artifacts.length > 4 || value.artifacts.some((a) => a.source_attachment_id !== attachmentId)
        || value.next_cursor < after || (value.has_more && value.next_cursor <= after)) throw new Error("Media page mismatch");
      if (active) setPage(value);
    }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, [sessionId, attachmentId, after, revision]);
  function refresh() { setPages([0]); setRevision((v) => v + 1); }
  return <section aria-label={t("media.artifacts")}>
    <Button size="small" onClick={refresh}>{t("media.refresh")}</Button>
    {error ? <p role="alert">{t("media.loadError")} <Button onClick={() => setRevision((v) => v + 1)}>{t("media.retry")}</Button></p>
      : !page ? <p role="status">{t("media.loading")}</p>
      : <><p>{page.artifacts.length ? t("media.localNotice") : t("media.empty")}</p>
        <ul>{page.artifacts.map((artifact) => <li key={artifact.artifact_id}>
          <Button size="small" variant="ghost" onClick={() => setSelected(artifact)} aria-pressed={selected?.artifact_id === artifact.artifact_id}>
            {artifact.name} · {artifact.total_bytes} B · {artifact.artifact_id}
          </Button>
          <pre className="attachment-processing">{JSON.stringify(artifact.processing, null, 2)}</pre>
        </li>)}</ul>
        <Button size="small" disabled={pages.length === 1} onClick={() => setPages((p) => p.slice(0, -1))}>{t("media.previous")}</Button>
        <Button size="small" disabled={!page.has_more} onClick={() => setPages((p) => [...p, page.next_cursor])}>{t("media.next")}</Button>
        {selected ? <MediaDetail key={selected.artifact_id} sessionId={sessionId} attachmentId={attachmentId} artifact={selected} /> : null}
      </>}
  </section>;
}

function MediaDetail({ sessionId, attachmentId, artifact }: { sessionId: string; attachmentId: string; artifact: MediaArtifactRef }) {
  const { t } = useI18n();
  const [content, setContent] = useState<MediaArtifactContent>();
  const [error, setError] = useState(false);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    setContent(undefined); setError(false);
    void getHubMediaArtifact(sessionId, attachmentId, artifact.artifact_id).then((value) => {
      if (value.artifact_id !== artifact.artifact_id || value.source_attachment_id !== attachmentId
        || value.total_bytes !== artifact.total_bytes || value.content_type !== artifact.content_type
        || value.processing.output_sha256 !== artifact.processing.output_sha256 || attachmentBytes(value).length !== artifact.total_bytes) throw new Error("Media mismatch");
      if (active) setContent(value);
    }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, [sessionId, attachmentId, artifact, revision]);
  return <section aria-label={t("media.selected")}>
    {error ? <p role="alert">{t("media.previewError")} <Button onClick={() => setRevision((v) => v + 1)}>{t("media.retry")}</Button></p>
      : !content ? <p role="status">{t("media.loading")}</p>
      : <AttachmentPreview file={content} notice={t("media.localNotice")} errorMessage={t("media.webviewError")} />}
    <MediaReceipts sessionId={sessionId} attachmentId={attachmentId} artifactId={artifact.artifact_id} />
  </section>;
}

function MediaReceipts({ sessionId, attachmentId, artifactId }: { sessionId: string; attachmentId: string; artifactId: string }) {
  const { t } = useI18n();
  const [pages, setPages] = useState([0]);
  const after = pages[pages.length - 1];
  const [through, setThrough] = useState<number>();
  const snapshot = useRef<number>();
  const [revision, setRevision] = useState(0);
  const [page, setPage] = useState<MediaReceiptPage>();
  const [error, setError] = useState(false);
  useEffect(() => {
    let active = true;
    setPage(undefined); setError(false);
    void getHubMediaReceipts(sessionId, attachmentId, artifactId, after, snapshot.current).then((value) => {
      if (value.receipts.length > 5 || value.next_cursor < after || (value.has_more && value.next_cursor <= after)
        || (snapshot.current !== undefined && value.through !== snapshot.current)) throw new Error("Receipt page mismatch");
      if (active) { snapshot.current = value.through; setThrough(value.through); setPage(value); }
    }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, [sessionId, attachmentId, artifactId, after, revision]);
  function refresh() { snapshot.current = undefined; setThrough(undefined); setPages([0]); setRevision((v) => v + 1); }
  return <section aria-label={t("media.receipts")}>
    <h3>{t("media.receipts")}</h3><p>{t("media.receiptNotice")}</p>
    <Button size="small" onClick={refresh}>{t("media.refreshReceipts")}</Button>
    {error ? <p role="alert">{t("media.loadError")} <Button onClick={() => setRevision((v) => v + 1)}>{t("media.retry")}</Button></p>
      : !page ? <p role="status">{t("media.loading")}</p>
      : <><p>{t("media.snapshot", { through: through ?? 0 })}</p>
        {!page.receipts.length ? <p>{t("media.noReceipts")}</p> : <ul>{page.receipts.map((receipt) => <li key={receipt.delivery_id}>
          <p>{t(Object.prototype.hasOwnProperty.call(STATUS, receipt.status) ? STATUS[receipt.status as keyof typeof STATUS] : "media.statusUnknown")} · <code>{receipt.status}</code></p>
          <pre className="attachment-processing">{JSON.stringify(receipt, null, 2)}</pre>
        </li>)}</ul>}
        <Button size="small" disabled={pages.length === 1} onClick={() => setPages((p) => p.slice(0, -1))}>{t("media.previousReceipts")}</Button>
        <Button size="small" disabled={!page.has_more} onClick={() => setPages((p) => [...p, page.next_cursor])}>{t("media.nextReceipts")}</Button>
      </>}
  </section>;
}
