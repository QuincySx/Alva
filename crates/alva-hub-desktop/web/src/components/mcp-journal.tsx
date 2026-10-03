// INPUT: Session-bound MCP journal query bridge, i18n and shared UI controls
// OUTPUT: Explicit read-only metadata/search pages and bounded journal text
// POS: Inspector domain component; never starts MCP/model work or replays calls.
import { useEffect, useRef, useState } from "react";
import { inspectHubMcpJournal, type McpJournalPage, type McpJournalQuery } from "../bridge";
import { useI18n } from "../i18n";
import { Button, EmptyState, ListRow, Notice, PanelSection, TextField } from "../ui";

type IndexView = { query: string | null; after: number; through?: number; previous: number[] };
type View =
  | { kind: "index"; index: IndexView }
  | { kind: "read"; id: string; offset: number; previous: number[]; index: IndexView };
type Result = { view: View; retry: number } & (
  | { status: "ready"; page: McpJournalPage }
  | { status: "error" }
);
export interface McpJournalProps {
  productSessionId: string;
  nativeSessionId: string;
  load?: typeof inspectHubMcpJournal;
}

export function McpJournal({ productSessionId, nativeSessionId, load = inspectHubMcpJournal }: McpJournalProps) {
  // Keyed by product/native scope, so cursors and pending responses cannot cross sessions.
  return (
    <Journal key={`${productSessionId}:${nativeSessionId}`}
      productSessionId={productSessionId} nativeSessionId={nativeSessionId} load={load} />
  );
}
function Journal({ productSessionId, nativeSessionId, load = inspectHubMcpJournal }: McpJournalProps) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [view, setView] = useState<View>({ kind: "index", index: { query: null, after: 0, previous: [] } });
  const [result, setResult] = useState<Result | null>(null);
  const [retry, setRetry] = useState(0);
  const heading = useRef<HTMLDivElement>(null);
  const openButtons = useRef(new Map<string, HTMLButtonElement>());
  const returnFocus = useRef<string | null>(null);
  // Bind content to the exact navigation/retry, not just the component's scope.
  // A new cursor cannot briefly render the previous page while its effect starts.
  const current = result?.view === view && result.retry === retry ? result : null;
  const loading = current === null;
  const error = current?.status === "error";
  const page = current?.status === "ready" ? current.page : null;
  useEffect(() => {
    let cancelled = false;
    let input: McpJournalQuery;
    if (view.kind === "read") {
      input = { action: "read", record_id: view.id, offset: view.offset };
    } else {
      const cursor = { after_seq: view.index.after, through: view.index.through };
      input = view.index.query === null
        ? { action: "list", ...cursor }
        : { action: "search_history", query: view.index.query, ...cursor };
    }
    void load(productSessionId, nativeSessionId, input)
      .then((page) => {
        if (cancelled) return;
        // Runtime responses must correspond to the active action and record.
        const mismatch = view.kind === "read"
          ? !("text" in page) || page.record_id !== view.id || page.offset !== view.offset
          : !("hits" in page);
        if (mismatch) throw new Error("Journal response mismatch");
        setResult({ view, retry, status: "ready", page });
      })
      .catch(() => {
        if (!cancelled) setResult({ view, retry, status: "error" });
      });
    return () => { cancelled = true; };
  }, [load, productSessionId, nativeSessionId, view, retry]);
  useEffect(() => { if (view.kind === "read") heading.current?.focus(); }, [view.kind]);
  useEffect(() => {
    if (view.kind !== "index" || !page || !returnFocus.current) return;
    const button = openButtons.current.get(returnFocus.current);
    (button ?? heading.current)?.focus();
    returnFocus.current = null;
  }, [view.kind, page]);
  const index = view.index;
  const rows = page && "hits" in page ? page : null;
  const body = page && "text" in page ? page : null;
  const previous = view.kind === "read" ? view.previous : index.previous;
  return <PanelSection heading={t("mcpJournal.title")} description={t("mcpJournal.notice")}>
    <div ref={heading} tabIndex={-1}><strong>{view.kind === "read" ? view.id : t("mcpJournal.title")}</strong></div>
    {view.kind === "index" ? <form onSubmit={(event) => {
      event.preventDefault();
      if (!query.trim() || Array.from(query).length > 200) return;
      setView({ kind: "index", index: { query, after: 0, previous: [] } });
    }}>
      <TextField aria-label={t("mcpJournal.query")} value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && event.nativeEvent.isComposing) event.preventDefault();
        }} />
      <Button type="submit" disabled={loading || !query.trim() || Array.from(query).length > 200}>{t("mcpJournal.search")}</Button>
      <Button disabled={loading} onClick={() => setView({ kind: "index", index: { query: null, after: 0, previous: [] } })}>{t("mcpJournal.refresh")}</Button>
    </form> : <Button onClick={() => {
      returnFocus.current = view.id;
      heading.current?.focus();
      setView({ kind: "index", index });
    }}>{t("mcpJournal.back")}</Button>}
    {loading ? <p role="status">{t("inspector.loading")}</p> : null}
    {error ? <Notice role="alert" tone="danger" heading={t("mcpJournal.error")} actions={<Button onClick={() => setRetry((value) => value + 1)}>{t("mcpJournal.retry")}</Button>} /> : null}
    {rows ? <>
      {rows.hits.length === 0 ? <EmptyState title={t("mcpJournal.empty")} description={rows.has_more ? t("mcpJournal.moreScan") : t("mcpJournal.snapshot")} /> : rows.hits.map((hit) => <ListRow key={hit.record_id}
        heading={`${hit.seq} · ${hit.server_id || hit.record_id.slice(0, 8)}`}
        description={hit.preview ?? `${hit.direction ?? ""} · ${hit.source_bytes ?? "—"} bytes`}
        actions={<Button ref={(element) => {
          if (element) openButtons.current.set(hit.record_id, element);
          else openButtons.current.delete(hit.record_id);
        }} onClick={() => setView({
          kind: "read", id: hit.record_id, offset: hit.match_offset ?? 0,
          previous: [], index: { ...index, through: rows.through },
        })}>{t("common.open")}</Button>} />)}
      <p>{t("mcpJournal.snapshot")} · {rows.through}</p>
    </> : null}
    {body ? <><p>{body.offset}–{body.next_offset} / {body.total_bytes} bytes</p><pre className="hub-engine-inspector__json">{body.text}</pre></> : null}
    <div>
      <Button disabled={loading || error || previous.length === 0} onClick={() => {
        const last = previous.at(-1)!;
        if (view.kind === "read") setView({ ...view, offset: last, previous: previous.slice(0, -1) });
        else setView({ kind: "index", index: { ...index, after: last, through: rows?.through, previous: previous.slice(0, -1) } });
      }}>{t("mcpJournal.previous")}</Button>
      <Button disabled={loading || error || !page?.has_more} onClick={() => {
        if (view.kind === "read" && body) setView({ ...view, offset: body.next_offset, previous: [...previous, body.offset] });
        else if (rows) setView({ kind: "index", index: { ...index, after: rows.next_cursor, through: rows.through, previous: [...previous, index.after] } });
      }}>{t("mcpJournal.next")}</Button>
    </div>
  </PanelSection>;
}
