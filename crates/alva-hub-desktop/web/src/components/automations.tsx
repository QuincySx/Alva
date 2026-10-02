// INPUT: Durable interval schedules, saved Hub sessions, typed i18n and shared Controls
// OUTPUT: Real automation CRUD, explicit billing review, pause and session navigation
// POS: Local-app scheduling page; no cron daemon, remote service or hidden execution

import { useEffect, useRef, useState } from "react";
import { CalendarClock, Plus } from "lucide-react";
import { automationAction, listAutomations, type Automation, type AutomationAction, type HubSessionSummary } from "../bridge";
import { useI18n } from "../i18n";
import { Badge, Button, CardGrid, Checkbox, DialogContent, DialogRoot, EmptyState, FieldFrame, Notice, PageHeader, ScrollRegion, SelectField, TextArea, TextField } from "../ui";

export function AutomationsPage({ sessions = [], activeSessionId = null, onOpenSession }: { sessions?: HubSessionSummary[]; activeSessionId?: string | null; onOpenSession?: (id: string) => void }) {
  const { t, locale } = useI18n();
  const [rows, setRows] = useState<Automation[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [editor, setEditor] = useState<{ row: Automation | null } | null>(null);
  const [review, setReview] = useState<{ row: Automation; op: "enable" | "delete" } | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [interval, setInterval] = useState("1440");
  const [sessionId, setSessionId] = useState("");
  const live = useRef(false);
  const mutationBusy = useRef(false);
  const listRequest = useRef(0);
  const trigger = useRef<HTMLElement | null>(null);
  const refreshButton = useRef<HTMLButtonElement | null>(null);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  useEffect(() => {
    let active = true;
    async function load() {
      if (mutationBusy.current) return;
      const request = ++listRequest.current;
      try { const result = await listAutomations(); if (active && request === listRequest.current) { setRows(result); setLoadError(null); } }
      catch (reason) { if (active && request === listRequest.current) setLoadError(String(reason)); }
    }
    void load();
    const timer = window.setInterval(() => void load(), 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, [refresh]);

  function openEditor(row: Automation | null, element: HTMLElement, template?: { title: string; prompt: string; minutes: string }) {
    trigger.current = element; setError(null); setEditor({ row });
    setTitle(row?.title ?? template?.title ?? ""); setPrompt(row?.prompt ?? template?.prompt ?? "");
    setInterval(row ? String(row.interval_minutes) : template?.minutes ?? "1440");
    setSessionId(row?.product_session_id ?? activeSessionId ?? sessions[0]?.product_session_id ?? "");
  }
  async function mutate(action: AutomationAction) {
    mutationBusy.current = true; listRequest.current++;
    setBusy(true); setError(null);
    try {
      const next = await automationAction(action);
      if (!live.current) return;
      setRows((current) => next ? [next, ...(current ?? []).filter((row) => row.automation_id !== next.automation_id)] : (current ?? []).filter((row) => row.automation_id !== ("automation_id" in action ? action.automation_id : "")));
      setEditor(null); setReview(null); setRefresh((value) => value + 1);
    } catch (reason) { if (live.current) setError(String(reason)); }
    finally { mutationBusy.current = false; if (live.current) setBusy(false); }
  }
  const templates = [
    { title: t("automation.templateNightly"), prompt: t("automation.templateNightlyPrompt"), minutes: "1440" },
    { title: t("automation.templateReview"), prompt: t("automation.templateReviewPrompt"), minutes: "1440" },
    { title: t("automation.templateRadar"), prompt: t("automation.templateRadarPrompt"), minutes: "10080" },
  ];
  const restoreFocus = (event: Event) => { event.preventDefault(); (trigger.current?.isConnected ? trigger.current : refreshButton.current)?.focus(); };
  const timestamp = (value: number | null) => value === null ? "—" : new Date(value).toLocaleString(locale);
  const status = (row: Automation) => row.running_turn_id ? t("automation.statusRunning") : row.enabled ? t("automation.statusEnabled") : t("automation.statusPaused");
  return <ScrollRegion className="hub-product-page">
    <PageHeader title={t("nav.automations")} description={t("automation.localDescription")} meta={t("automation.configured", { count: rows?.length ?? 0 })} actions={<>
      <Button ref={refreshButton} disabled={busy} variant="ghost" onClick={() => { setError(null); setLoadError(null); setRefresh((value) => value + 1); }}>{t("common.refresh")}</Button>
      <Button disabled={busy || sessions.length === 0 || rows === null} leadingIcon={<Plus size={15} />} variant="primary" onClick={(event) => openEditor(null, event.currentTarget)}>{t("automation.new")}</Button>
    </>} />
    <Notice tone="info" heading={t("automation.localTitle")} description={t("automation.localScope")} />
    {(error || loadError) && !editor && !review ? <Notice role="alert" tone="warning" heading={t("automation.failedRequest")} description={error ?? loadError} /> : null}
    {!rows && !error && !loadError ? <p role="status">{t("automation.loading")}</p> : null}
    {sessions.length === 0 ? <EmptyState title={t("automation.chooseSession")} description={t("automation.chooseSessionDescription")} /> : null}
    {rows?.length === 0 ? <EmptyState title={t("automation.empty")} description={t("automation.emptyDescription")} /> : null}
    <CardGrid>{rows?.map((row) => <article className="hub-extension-card hub-automation-card" key={row.automation_id}>
      <div className="hub-extension-card__heading"><span><CalendarClock size={18} aria-hidden="true" /></span><div><strong>{row.title}</strong><small>{t("automation.everyMinutes", { minutes: row.interval_minutes })}</small></div><Badge tone={row.running_turn_id ? "warning" : row.enabled ? "success" : "neutral"}>{status(row)}</Badge></div>
      <p>{row.target.engine_id} · {row.target.model_id} · {row.target.provider_id}</p>
      <p>{row.target.account_source} · {row.target.billing_route}</p>
      <p>{t("automation.nextRun", { time: timestamp(row.next_run_at) })}</p>
      <p>{t("automation.lastRun", { time: timestamp(row.last_run_at) })} · {row.last_status === "never" ? t("automation.lastNever") : row.last_status === "skipped_busy" ? t("automation.lastBusy") : row.last_status === "skipped_missed" ? t("automation.lastMissed") : row.last_status === "route_changed" ? t("automation.lastRouteChanged") : row.last_status === "completed" ? t("automation.lastCompleted") : row.last_status === "running" ? t("automation.statusRunning") : row.last_status === "interrupted" ? t("automation.lastInterrupted") : t("automation.lastFailed")}</p>
      {row.last_error ? <Notice tone="warning" heading={t("automation.lastError")} description={row.last_error} /> : null}
      <div className="hub-dialog-actions">
        {onOpenSession ? <Button variant="ghost" size="small" onClick={() => onOpenSession(row.product_session_id)}>{t("automation.openSession")}</Button> : null}
        <Button disabled={busy || !!row.running_turn_id} variant="ghost" size="small" onClick={(event) => openEditor(row, event.currentTarget)}>{t("automation.edit")}</Button>
        {row.enabled ? <Button disabled={busy} size="small" onClick={() => void mutate({ op: "set_enabled", automation_id: row.automation_id, revision: row.revision, enabled: false })}>{t("automation.pause")}</Button> : <Button disabled={busy || !!row.running_turn_id} size="small" onClick={(event) => { trigger.current = event.currentTarget; setError(null); setAccepted(false); setReview({ row, op: "enable" }); }}>{t("automation.enable")}</Button>}
        <Button disabled={busy || !!row.running_turn_id} variant="ghost" size="small" onClick={(event) => { trigger.current = event.currentTarget; setError(null); setReview({ row, op: "delete" }); }}>{t("automation.delete")}</Button>
      </div>
    </article>)}</CardGrid>
    <section className="hub-product-section"><h3>{t("automation.templates")}</h3><CardGrid>{templates.map((template) => <article className="hub-extension-card" key={template.title}><strong>{template.title}</strong><p>{template.prompt}</p><p>{t("automation.everyMinutes", { minutes: Number(template.minutes) })}</p><Button disabled={busy || !rows || sessions.length === 0} variant="ghost" onClick={(event) => openEditor(null, event.currentTarget, template)}>{t("automation.useTemplate")}</Button></article>)}</CardGrid></section>
    <DialogRoot open={!!editor} onOpenChange={(open) => { if (!open && !busy) { setEditor(null); setError(null); } }}>
      <DialogContent title={editor?.row ? t("automation.edit") : t("automation.new")} description={t("automation.savedPaused")} size="wide" onCloseAutoFocus={restoreFocus}>
        <div className="hub-form-stack">
          {error ? <Notice role="alert" tone="warning" heading={t("automation.failedRequest")} description={error} /> : null}
          <FieldFrame label={t("automation.session")}><SelectField ariaLabel={t("automation.session")} disabled={busy || !!editor?.row} value={sessionId} onValueChange={setSessionId} options={sessions.map((session) => ({ value: session.product_session_id, label: `${session.last_prompt?.slice(0, 60) ?? session.product_session_id} · ${session.engine_id} · ${session.model_id}` }))} /></FieldFrame>
          <FieldFrame label={t("automation.title")}><TextField aria-label={t("automation.title")} maxLength={120} value={title} disabled={busy} onChange={(event) => setTitle(event.target.value)} /></FieldFrame>
          <FieldFrame label={t("automation.interval")} description={t("automation.intervalDescription")}><TextField aria-label={t("automation.interval")} type="number" min={5} max={10080} step={1} value={interval} disabled={busy} onChange={(event) => setInterval(event.target.value)} /></FieldFrame>
          <FieldFrame label={t("automation.prompt")}><TextArea aria-label={t("automation.prompt")} value={prompt} rows={7} disabled={busy} onChange={(event) => setPrompt(event.target.value)} /></FieldFrame>
          <div className="hub-dialog-actions"><Button disabled={busy} variant="ghost" onClick={() => { setEditor(null); setError(null); }}>{t("common.cancel")}</Button><Button disabled={busy || !sessionId || !title.trim() || !prompt.trim() || !Number.isInteger(Number(interval)) || Number(interval) < 5 || Number(interval) > 10080} variant="primary" onClick={() => void mutate(editor?.row ? { op: "update", automation_id: editor.row.automation_id, revision: editor.row.revision, title, prompt, interval_minutes: Number(interval) } : { op: "create", product_session_id: sessionId, title, prompt, interval_minutes: Number(interval) })}>{busy ? t("common.saving") : t("common.save")}</Button></div>
        </div>
      </DialogContent>
    </DialogRoot>
    <DialogRoot open={!!review} onOpenChange={(open) => { if (!open && !busy) { setReview(null); setError(null); } }}>
      <DialogContent className="hub-automation-dialog" title={review?.op === "enable" ? t("automation.reviewTitle") : t("automation.deleteTitle")} description={review?.op === "enable" ? t("automation.reviewDescription") : t("automation.deleteDescription")} onCloseAutoFocus={restoreFocus}>
        <div className="hub-form-stack">
          {error ? <Notice role="alert" tone="warning" heading={t("automation.failedRequest")} description={error} /> : null}
          {review?.op === "enable" ? <><p>{review.row.target.engine_id} · {review.row.target.model_id} · {review.row.target.provider_id}</p><p>{review.row.target.account_source}</p><p>{review.row.target.billing_route}</p><p>{t("automation.permission", { profile: sessions.find((session) => session.product_session_id === review.row.product_session_id)?.permission_profile ?? "—" })}</p><Checkbox checked={accepted} disabled={busy} onCheckedChange={(value) => setAccepted(value === true)} label={t("automation.acceptBilling")} /></> : null}
          <div className="hub-dialog-actions"><Button disabled={busy} variant="ghost" onClick={() => { setReview(null); setError(null); }}>{t("common.cancel")}</Button><Button disabled={busy || (review?.op === "enable" && !accepted)} variant={review?.op === "delete" ? "danger" : "primary"} onClick={() => review && void mutate(review.op === "enable" ? { op: "set_enabled", automation_id: review.row.automation_id, revision: review.row.revision, enabled: true } : { op: "delete", automation_id: review.row.automation_id, revision: review.row.revision })}>{review?.op === "enable" ? t("automation.enableConfirmed") : t("automation.delete")}</Button></div>
        </div>
      </DialogContent>
    </DialogRoot>
  </ScrollRegion>;
}
