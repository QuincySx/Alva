// INPUT: Explicit original/derived preview bytes and truthful source notice
// OUTPUT: AttachmentPreview, attachmentBytes and shared preview file shape
// POS: Local-only inert text/media preview with revocable Blob lifetime, never model input.
import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { CopyTextButton } from "./copy-text-button";

export interface AttachmentPreviewFile {
  name: string; data_base64?: string | null; text?: string | null;
  path?: string; processing?: Record<string, unknown>;
}
export function attachmentBytes(file: Pick<AttachmentPreviewFile, "data_base64" | "text">): Uint8Array {
  return file.data_base64 ? Uint8Array.from(atob(file.data_base64), (character) => character.charCodeAt(0))
    : new TextEncoder().encode(file.text ?? "");
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
export function AttachmentPreview({ file, notice, errorMessage }: {
  file: AttachmentPreviewFile; notice?: string; errorMessage?: string;
}) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<{ file: typeof file; text?: string; url?: string; mime: string }>();
  const [failedFile, setFailedFile] = useState<typeof file>();
  useEffect(() => {
    let url: string | undefined;
    let text: string | undefined;
    try {
      const bytes = attachmentBytes(file);
      const mime = previewType(bytes);
      if (mime) url = URL.createObjectURL(new Blob([new Uint8Array(bytes).buffer], { type: mime }));
      else try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); if (text.includes("\0")) text = undefined; } catch { /* binary: metadata only */ }
      setPreview({ file, text, url, mime });
    } catch { setFailedFile(file); }
    return () => { if (url) URL.revokeObjectURL(url); };
  }, [file]);
  const current = preview?.file === file ? preview : undefined;
  return <div><p>{notice ?? t("attachment.unprocessed")}</p>
    {file.path ? <code>{file.path}</code> : null}
    {file.processing ? <pre className="attachment-processing">{JSON.stringify(file.processing, null, 2)}</pre> : null}
    {failedFile === file ? <p role="status">{errorMessage ?? t("attachment.mediaPreviewError")}</p>
      : current?.url ? current.mime.startsWith("image/")
      ? <img className="attachment-media" src={current.url} alt={file.name} onError={() => setFailedFile(file)} />
      : <video className="attachment-media" src={current.url} controls preload="metadata" aria-label={file.name} onError={() => setFailedFile(file)} />
      : current === undefined ? <p role="status">{t("attachment.reading")}</p>
      : current.text !== undefined ? <><pre className="attachment-preview">{current.text}</pre><CopyTextButton text={current.text} label={t("chat.copy")} /></>
      : <p>{t("attachment.binarySaved")}</p>}
  </div>;
}
