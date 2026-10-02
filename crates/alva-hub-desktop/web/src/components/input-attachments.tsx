// INPUT: Explicitly selected File bytes or session-owned durable attachment refs
// OUTPUT: InputAttachments picker, MessageAttachments preview and truthful processing manifest
// POS: Reference-first uploads; local previews never imply media was sent to or analyzed by AI.
import { useEffect, useRef, useState, type RefObject } from "react";
import { getHubInputAttachment, type InputAttachment, type InputAttachmentRef, type InputAttachmentContent } from "../bridge";
import { useI18n } from "../i18n";
import { Button, DialogContent, DialogRoot } from "../ui";
import { CopyTextButton } from "./copy-text-button";

export function attachmentBytes(file: InputAttachment | InputAttachmentContent): Uint8Array {
  return file.data_base64 ? Uint8Array.from(atob(file.data_base64), (character) => character.charCodeAt(0))
    : new TextEncoder().encode(file.text ?? "");
}
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
  </li>)}</ul><DialogRoot open={selected !== null} onOpenChange={(open) => { if (!open) setSelected(null); }}>
    <DialogContent title={selected?.name ?? t("attachment.label")} description={t("attachment.preview")} size="wide"
      onCloseAutoFocus={(event) => { event.preventDefault(); previewTrigger.current?.focus(); }}>
      {errorKey === key ? <div role="alert">{t("attachment.previewError")}<Button onClick={() => setRetry((value) => value + 1)}>{t("timeline.contentRetry")}</Button></div>
        : currentFile === undefined ? <p role="status">{t("attachment.reading")}</p> : <AttachmentPreview file={currentFile} />}
    </DialogContent>
  </DialogRoot></div>;
}

function previewType(bytes: Uint8Array): string {
  const prefix = String.fromCharCode(...bytes.subarray(0, 12));
  if (prefix.startsWith("\x89PNG\r\n\x1a\n")) return "image/png";
  if (prefix.startsWith("\xff\xd8\xff")) return "image/jpeg";
  if (/^GIF8[79]a/.test(prefix)) return "image/gif";
  if (prefix.startsWith("RIFF") && prefix.slice(8) === "WEBP") return "image/webp";
  if (prefix.slice(4, 8) === "ftyp" && !["avif", "avis", "heic", "heix", "mif1"].includes(prefix.slice(8, 12))) return "video/mp4";
  if (prefix.startsWith("\x1a\x45\xdf\xa3") && new TextDecoder().decode(bytes.subarray(0, 4096)).includes("webm")) return "video/webm";
  return "";
}
function AttachmentPreview({ file }: { file: InputAttachment | InputAttachmentContent }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<{ file: typeof file; text?: string; url?: string; mime: string }>();
  const [failedFile, setFailedFile] = useState<typeof file>();
  useEffect(() => {
    const bytes = attachmentBytes(file);
    const mime = previewType(bytes);
    let url: string | undefined;
    let text: string | undefined;
    if (mime) {
      try { url = URL.createObjectURL(new Blob([new Uint8Array(bytes).buffer], { type: mime })); }
      catch { setFailedFile(file); }
    }
    else try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); if (text.includes("\0")) text = undefined; } catch { /* binary: metadata only */ }
    setPreview({ file, text, url, mime });
    return () => { if (url) URL.revokeObjectURL(url); };
  }, [file]);
  const current = preview?.file === file ? preview : undefined;
  return <div><p>{t("attachment.unprocessed")}</p>
    {"path" in file && file.path ? <code>{file.path}</code> : null}
    {"processing" in file && file.processing ? <pre className="attachment-processing">{JSON.stringify(file.processing, null, 2)}</pre> : null}
    {failedFile === file ? <p role="status">{t("attachment.mediaPreviewError")}</p>
      : current?.url ? current.mime.startsWith("image/")
      ? <img className="attachment-media" src={current.url} alt={file.name} onError={() => setFailedFile(file)} />
      : <video className="attachment-media" src={current.url} controls preload="metadata" aria-label={file.name} onError={() => setFailedFile(file)} />
      : current === undefined ? <p role="status">{t("attachment.reading")}</p>
      : current.text !== undefined ? <><pre className="attachment-preview">{current.text}</pre><CopyTextButton text={current.text} label={t("chat.copy")} /></>
      : <p>{t("attachment.binarySaved")}</p>}
  </div>;
}
