// INPUT: Session-scoped tool content reference, preview, paged bridge API
// OUTPUT: ToolContent — bounded keyboard-reachable pages with optional trusted modules on complete bodies
// POS: Mounted tool rows own only their current page, not an unbounded body cache.

import { useEffect, useState } from "react";
import { getHubToolContent, type ToolContentRef } from "../bridge";
import { useI18n } from "../i18n";
import { Button } from "../ui";
import { MessageModules } from "./message-modules";

export function ToolContent({ sessionId, content, preview, label, renderModules = false }: {
  sessionId: string;
  content?: ToolContentRef | null;
  preview?: string | null;
  label: string;
  renderModules?: boolean;
}) {
  const { t } = useI18n();
  const [page, setPage] = useState(0);
  const [retry, setRetry] = useState(0);
  const [loaded, setLoaded] = useState<{ key: string; text?: string; error?: boolean }>();
  const key = JSON.stringify([sessionId, content?.content_id, page]);
  useEffect(() => {
    if (!content) return;
    let active = true;
    setLoaded({ key });
    void getHubToolContent(sessionId, content.content_id, page).then((chunk) => {
      if (chunk.content_id !== content.content_id || chunk.chunk_index !== page
        || chunk.chunk_count !== content.chunk_count || chunk.total_bytes !== content.total_bytes) {
        throw new Error("tool content response does not match its reference");
      }
      if (active) setLoaded({ key, text: chunk.text });
    }).catch(() => { if (active) setLoaded({ key, error: true }); });
    return () => { active = false; };
  }, [sessionId, content?.content_id, content?.chunk_count, content?.total_bytes, page, retry, key]);

  if (!content && preview == null) return null;
  const current = loaded?.key === key ? loaded : undefined;
  const loading = !!content && !current?.error && current?.text === undefined;
  return <section aria-label={label} aria-busy={loading}>
    <small>{label}</small>
    {renderModules && (!content || content.chunk_count === 1) ? <MessageModules source={content ? current?.text ?? "" : preview ?? ""} /> : null}
    <pre className="timeline-activity-output" aria-label={label}>
      {content ? current?.text ?? (page === 0 ? preview : "") : preview}
    </pre>
    {content ? <div>
      <small>{t("timeline.contentPage", { page: page + 1, pages: content.chunk_count, bytes: content.total_bytes })}</small>
      <Button size="small" variant="ghost" disabled={loading || page === 0}
        aria-label={t("timeline.previousContentPage")} onClick={() => setPage((value) => value - 1)}>
        {t("timeline.previousContentPage")}
      </Button>
      <Button size="small" variant="ghost" disabled={loading || page + 1 >= content.chunk_count}
        aria-label={t("timeline.nextContentPage")} onClick={() => setPage((value) => value + 1)}>
        {t("timeline.nextContentPage")}
      </Button>
      {loading ? <small role="status">{t("timeline.contentLoading")}</small> : null}
      {current?.error ? <div role="status">
        <small>{t("timeline.contentError")}</small>
        <Button size="small" variant="ghost" onClick={() => setRetry((value) => value + 1)}>{t("timeline.contentRetry")}</Button>
      </div> : null}
    </div> : <small>{t("timeline.contentLegacy")}</small>}
  </section>;
}
