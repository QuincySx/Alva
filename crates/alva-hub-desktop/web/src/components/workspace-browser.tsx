// INPUT: Explicit user navigation and session-owned native browser bridge
// OUTPUT: WorkspaceBrowser controls an isolated temporary native window
// POS: User browser workbench surface, never Agent automation or a privileged iframe.

import { useEffect, useRef, useState } from "react";
import { hubBrowser, NATIVE_WORKBENCH, type BrowserSnapshot } from "../bridge";
import { useI18n } from "../i18n";
import { Button, EmptyState, Notice, PanelSection, TextField } from "../ui";

export function WorkspaceBrowser({ sessionId }: { sessionId: string }) {
  const { t } = useI18n();
  const [id, setId] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [snapshot, setSnapshot] = useState<BrowserSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [pollError, setPollError] = useState(false);
  const owned = useRef<string | null>(null);
  const alive = useRef(true);
  const dirty = useRef(false);
  const observedUrl = useRef("");
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (owned.current) void hubBrowser({ action: "control", operation: "close", session: sessionId, id: owned.current }).catch(() => {});
    };
  }, [sessionId]);
  useEffect(() => {
    if (!id) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function poll() {
      try {
        const next = await hubBrowser<BrowserSnapshot>({ action: "read", session: sessionId, id: id! });
        if (!active) return;
        setPollError(false);
        if (next.closed) { owned.current = null; setId(null); setSnapshot(null); return; }
        setSnapshot(next);
        if (next.url !== observedUrl.current) {
          observedUrl.current = next.url;
          if (!dirty.current) setUrl(next.url);
        }
        timer = setTimeout(() => void poll(), 500);
      } catch {
        if (active) { setPollError(true); timer = setTimeout(() => void poll(), 1000); }
      }
    }
    void poll();
    return () => { active = false; if (timer) clearTimeout(timer); };
  }, [id, sessionId]);
  async function navigate() {
    setBusy(true); setError(false);
    try {
      if (id) {
        await hubBrowser({ action: "control", operation: "navigate", session: sessionId, id, url });
      } else {
        const next = await hubBrowser<string>({ action: "open", session: sessionId, url });
        if (!alive.current) { await hubBrowser({ action: "control", operation: "close", session: sessionId, id: next }); return; }
        owned.current = next; observedUrl.current = ""; setId(next); setSnapshot(null);
      }
      if (alive.current) dirty.current = false;
    } catch { if (alive.current) setError(true); }
    finally { if (alive.current) setBusy(false); }
  }
  async function control(operation: "back" | "forward" | "reload" | "focus" | "close") {
    if (!id) return;
    setBusy(true); setError(false);
    try {
      await hubBrowser({ action: "control", operation, session: sessionId, id });
      if (!alive.current) return;
      dirty.current = false;
      if (operation === "close") { owned.current = null; setId(null); setSnapshot(null); }
    } catch { if (alive.current) setError(true); }
    finally { if (alive.current) setBusy(false); }
  }
  if (!NATIVE_WORKBENCH) return <EmptyState title={t("browser.unsupported")} />;
  return <PanelSection heading={t("workbench.browser")} description={t("browser.boundary")}>
    {error || pollError ? <Notice tone="warning" heading={t("browser.error")} /> : null}
    {snapshot?.blocked_navigation ? <Notice tone="warning" heading={t("browser.blocked")} /> : null}
    {snapshot?.slow ? <Notice tone="warning" heading={t("browser.slow")} /> : null}
    <form className="hub-browser-toolbar" onSubmit={(event) => { event.preventDefault(); if (url.trim() && !busy) void navigate(); }}>
      <TextField aria-label={t("tool.browserAddress")} placeholder="https://example.com" value={url} onChange={(event) => { dirty.current = true; setUrl(event.target.value); }} />
      <Button type="submit" disabled={busy || !url.trim()} size="small">{t("common.open")}</Button>
    </form>
    {id ? <>
      <div className="hub-browser-toolbar">
        {(["back", "forward", "reload", "focus", "close"] as const).map((operation) => <Button key={operation} disabled={busy} size="small" variant="ghost" onClick={() => void control(operation)}>{t(`browser.${operation}`)}</Button>)}
      </div>
      <p>{snapshot?.loading ? t("browser.loading") : t("browser.windowOpen")}</p>
      <p>{snapshot?.url}</p>
    </> : null}
  </PanelSection>;
}
