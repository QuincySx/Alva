// INPUT: Explicit user actions and session-scoped native PTY bridge
// OUTPUT: WorkspaceTerminal — bounded line-mode terminal with owned lifecycle
// POS: Desktop workbench terminal, never simulated HTTP or Agent execution.

import { useEffect, useRef, useState } from "react";
import { hubTerminal, NATIVE_WORKBENCH, type TerminalSnapshot } from "../bridge";
import { useI18n } from "../i18n";
import { Button, EmptyState, Notice, PanelSection, TextField } from "../ui";

export function WorkspaceTerminal({ sessionId }: { sessionId: string }) {
  const { t } = useI18n();
  const [id, setId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<TerminalSnapshot | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [pollError, setPollError] = useState(false);
  const [rows, setRows] = useState("24");
  const [columns, setColumns] = useState("80");
  const owned = useRef<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (owned.current) void hubTerminal({ action: "close", session: sessionId, id: owned.current }).catch(() => {});
    };
  }, [sessionId]);
  useEffect(() => {
    if (!id) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function poll() {
      try {
        const next = await hubTerminal<TerminalSnapshot>({ action: "read", session: sessionId, id: id! });
        if (active) { setSnapshot(next); setPollError(false); }
        if (active && !next.exited) timer = setTimeout(() => void poll(), 250);
      } catch {
        if (active) { setPollError(true); timer = setTimeout(() => void poll(), 1000); }
      }
    }
    void poll();
    return () => { active = false; if (timer) clearTimeout(timer); };
  }, [sessionId, id]);
  async function open() {
    setBusy(true); setError(false); setPollError(false);
    try {
      const next = await hubTerminal<string>({ action: "open", session: sessionId, rows: Number(rows), columns: Number(columns) });
      if (!alive.current) { await hubTerminal({ action: "close", session: sessionId, id: next }); return; }
      owned.current = next; setId(next); setSnapshot(null);
    } catch { if (alive.current) setError(true); }
    finally { if (alive.current) setBusy(false); }
  }
  async function action(kind: "write" | "resize" | "close", text?: string) {
    if (!id) return;
    setBusy(true); setError(false);
    try {
      await hubTerminal({ action: kind, session: sessionId, id, input: text, rows: Number(rows), columns: Number(columns) });
      if (!alive.current) return;
      if (kind === "close") { owned.current = null; setId(null); setSnapshot(null); }
      if (kind === "write" && text !== "\u0003") setInput("");
    } catch { if (alive.current) setError(true); }
    finally { if (alive.current) setBusy(false); }
  }
  if (!NATIVE_WORKBENCH) return <EmptyState title={t("terminal.unsupported")} />;
  return <PanelSection heading={t("terminal.title")} description={t("terminal.boundary")}>
    {error || pollError ? <Notice tone="warning" heading={t("terminal.error")} /> : null}
    {!id ? <Button disabled={busy} onClick={() => void open()}>{t("terminal.open")}</Button> : <>
      <Button disabled={busy} size="small" variant="ghost" onClick={() => void action("close")}>{t("terminal.close")}</Button>
      <Button disabled={busy || !!snapshot?.exited} size="small" variant="ghost" onClick={() => void action("write", "\u0003")}>Ctrl-C</Button>
      <TextField type="number" min={2} max={200} aria-label={t("terminal.rows")} value={rows} onChange={(event) => setRows(event.target.value)} />
      <TextField type="number" min={10} max={500} aria-label={t("terminal.columns")} value={columns} onChange={(event) => setColumns(event.target.value)} />
      <Button disabled={busy || !!snapshot?.exited} size="small" variant="ghost" onClick={() => void action("resize")}>{t("terminal.resize")}</Button>
      <pre className="timeline-activity-output" aria-label={t("terminal.output")}>{snapshot?.output ?? ""}</pre>
      {snapshot?.dropped_bytes ? <Notice heading={t("terminal.limit")} /> : null}
      {snapshot?.exited ? <Notice heading={t("terminal.exited", { code: snapshot.exit_code ?? -1 })} /> : null}
      <form onSubmit={(event) => { event.preventDefault(); if (!busy && input && !snapshot?.exited) void action("write", `${input}\n`); }}>
        <TextField aria-label={t("terminal.input")} value={input} disabled={busy || !!snapshot?.exited} onChange={(event) => setInput(event.target.value)} />
        <Button type="submit" disabled={busy || !input || !!snapshot?.exited}>{t("terminal.send")}</Button>
      </form>
    </>}
  </PanelSection>;
}
