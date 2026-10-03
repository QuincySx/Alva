// INPUT: Explicitly selected File bytes or session-owned durable attachment refs
// OUTPUT: InputAttachments picker, MessageAttachments original/derived inspection and truthful processing manifests
// POS: Reference-first uploads; local previews never imply media was sent to or analyzed by AI.
import { useEffect, useRef, useState, type RefObject } from "react";
import { getHubInputAttachment, type InputAttachment, type InputAttachmentRef, type InputAttachmentContent } from "../bridge";
import { useI18n } from "../i18n";
import { Button, DialogContent, DialogRoot } from "../ui";
import { AttachmentPreview, attachmentBytes } from "./attachment-preview";
import { AttachmentMedia } from "./attachment-media";
export { attachmentBytes } from "./attachment-preview";
function toBase64(bytes: Uint8Array): string {
  const pieces: string[] = [];
  for (let index = 0; index < bytes.length; index += 32768) pieces.push(String.fromCharCode(...bytes.subarray(index, index + 32768)));
  return btoa(pieces.join(""));
}
function byteLength(file: InputAttachment): number {
  return file.data_base64 ? file.data_base64.length / 4 * 3 - (file.data_base64.endsWith("==") ? 2 : file.data_base64.endsWith("=") ? 1 : 0)
    : new TextEncoder().encode(file.text ?? "").length;
}
export async function readSelectedFiles(selected: File[], existing: InputAttachment[]): Promise<InputAttachment[]> {
  if (selected.length + existing.length > 8 || selected.reduce((sum, file) => sum + file.size, 0)
    + existing.reduce((sum, file) => sum + byteLength(file), 0) > 40 * 1024 * 1024) throw new Error("attachment limit");
  const files: InputAttachment[] = [];
  for (const file of selected) {
    if (file.size > 20 * 1024 * 1024 || new TextEncoder().encode(file.name).length > 200
      || !file.name.trim() || [".", ".."].includes(file.name) || /[/\\\x00-\x1f\x7f]/.test(file.name)
      || /^(?:\.env(?:\.|$)|id_rsa|id_ed25519|credentials(?:\.|$))|\.(?:key|pem|p12|pfx)$/i.test(file.name)) throw new Error("attachment unavailable");
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (bytes.length > 20 * 1024 * 1024) throw new Error("attachment limit");
    files.push({ name: file.name, data_base64: toBase64(bytes) });
  }
  const combined = [...existing, ...files];
  if (combined.reduce((size, file) => size + byteLength(file), 0) > 40 * 1024 * 1024) throw new Error("attachment limit");
  return combined;
}

export function InputAttachments({ files, onChange, inputRef, disabled, onReadingChange }: {
  files: InputAttachment[];
  onChange: (files: InputAttachment[]) => void;
  inputRef: RefObject<HTMLInputElement>;
  disabled: boolean;
  onReadingChange?: (reading: boolean) => void;
}) {
  const { t } = useI18n();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const [preview, setPreview] = useState<InputAttachment | null>(null);
  const previewTrigger = useRef<HTMLButtonElement | null>(null);
  const generation = useRef(0);
  useEffect(() => () => { generation.current += 1; }, []);
  useEffect(() => { if (disabled) { generation.current += 1; setLoading(false); } }, [disabled]);
  useEffect(() => { onReadingChange?.(loading); return () => onReadingChange?.(false); }, [loading, onReadingChange]);
  async function add(selected: File[]) {
    if (disabled || loading || !selected.length) return;
    const current = ++generation.current;
    setLoading(true); setError(false);
    try { const next = await readSelectedFiles(selected, files); if (generation.current === current) onChange(next); }
    catch { if (generation.current === current) setError(true); }
    finally { if (generation.current === current) setLoading(false); }
  }
  return <section className="input-attachments" aria-label={t("attachment.label")}
    onDragOver={(event) => { if (!disabled) event.preventDefault(); }}
    onDrop={(event) => { event.preventDefault(); void add(Array.from(event.dataTransfer.files)); }}>
    <input ref={inputRef} type="file" multiple hidden disabled={disabled || loading} aria-label={t("attachment.choose")}
      onChange={(event) => { const selected = Array.from(event.target.files ?? []); event.target.value = ""; void add(selected); }} />
    <small>{t("attachment.help")}</small>
    {loading ? <small role="status">{t("attachment.reading")}</small> : null}
    {error ? <small role="alert">{t("attachment.error")}</small> : null}
    <ul>{files.map((file, index) => <li key={`${index}:${file.name}`}>
      <Button size="small" variant="ghost" onClick={(event) => { previewTrigger.current = event.currentTarget; setPreview(file); }}>{file.name} · {byteLength(file)} B</Button>
      <Button size="small" variant="ghost" disabled={disabled || loading} aria-label={t("attachment.remove", { name: file.name })}
        onClick={() => { onChange(files.filter((_, i) => i !== index)); setError(false); }}>{t("attachment.removeAction")}</Button>
    </li>)}</ul>
    <DialogRoot open={preview !== null} onOpenChange={(open) => { if (!open) setPreview(null); }}>
      <DialogContent title={preview?.name ?? t("attachment.label")} description={t("attachment.preview")} size="wide"
        onCloseAutoFocus={(event) => { event.preventDefault(); previewTrigger.current?.focus(); }}>
        {preview ? <AttachmentPreview file={preview} /> : null}
      </DialogContent>
    </DialogRoot>
  </section>;
}

export function MessageAttachments({ sessionId, files }: { sessionId: string; files: InputAttachmentRef[] }) {
  const { t } = useI18n();
  const [selected, setSelected] = useState<InputAttachmentRef | null>(null);
  const previewTrigger = useRef<HTMLButtonElement | null>(null);
  const [body, setBody] = useState<{ key: string; file: InputAttachmentContent }>();
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const key = JSON.stringify([sessionId, selected?.attachment_id]);
  useEffect(() => {
    if (!selected) return;
    let active = true;
    setBody(undefined); setErrorKey(null);
    void getHubInputAttachment(sessionId, selected.attachment_id).then((file) => {
      if (file.attachment_id !== selected.attachment_id || file.name !== selected.name
        || file.total_bytes !== selected.total_bytes || attachmentBytes(file).length !== selected.total_bytes) throw new Error("Attachment mismatch");
      if (active) setBody({ key, file });
    }).catch(() => { if (active) setErrorKey(key); });
    return () => { active = false; };
  }, [sessionId, selected, retry, key]);
  const currentFile = body?.key === key ? body.file : undefined;
  return <div className="message-attachments"><ul>{files.map((file) => <li key={file.attachment_id}>
    <Button size="small" variant="ghost" onClick={(event) => { previewTrigger.current = event.currentTarget; setSelected(file); }}>{file.name} · {file.total_bytes} B</Button>
    {file.content_type?.match(/^(image|video)\//) ? <AttachmentMedia key={`${sessionId}:${file.attachment_id}`} sessionId={sessionId} file={file} /> : null}
  </li>)}</ul><DialogRoot open={selected !== null} onOpenChange={(open) => { if (!open) setSelected(null); }}>
    <DialogContent title={selected?.name ?? t("attachment.label")} description={t("attachment.preview")} size="wide"
      onCloseAutoFocus={(event) => { event.preventDefault(); previewTrigger.current?.focus(); }}>
      {errorKey === key ? <div role="alert">{t("attachment.previewError")}<Button onClick={() => setRetry((value) => value + 1)}>{t("timeline.contentRetry")}</Button></div>
        : currentFile === undefined ? <p role="status">{t("attachment.reading")}</p> : <AttachmentPreview file={currentFile} />}
    </DialogContent>
  </DialogRoot></div>;
}
