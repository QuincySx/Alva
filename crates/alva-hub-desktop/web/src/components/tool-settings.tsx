// INPUT: Persisted ALVA capability/MCP bridge, typed i18n and shared UI controls
// OUTPUT: ToolSettings with real component switches and paused-first stdio MCP CRUD/diagnostics
// POS: Functional capability management; no fake global control over other engines or live connections

import { useEffect, useRef, useState } from "react";
import { getHubToolSettings, hubToolSettingsAction, testHubMcp, type HubToolSettings, type ManagedMcpServer, type McpDiagnostic, type ToolSettingsAction } from "../bridge";
import { useI18n, type MessageKey } from "../i18n";
import { Badge, Button, Checkbox, DialogContent, DialogRoot, EmptyState, FieldFrame, Notice, SettingsGroup, SettingsRow, Switch, TextArea, TextField } from "../ui";

const labels: Record<string, MessageKey> = { core: "pages.settings.toolFile", shell: "pages.settings.toolShell", web: "pages.settings.toolWeb", browser: "pages.settings.toolBrowser", skills: "extension.skills", planning: "tools.planning", utility: "tools.utility", task: "tools.tasks", team: "tools.team", "sub-agents": "tools.subAgents" };
const blank: ManagedMcpServer = { id: "", display_name: "", command: "", args: [], connect_timeout_secs: 30, enabled: false };

export function ToolSettings({ sessionId, query = "" }: { sessionId: string | null; query?: string }) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<HubToolSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [editor, setEditor] = useState<{ server: ManagedMcpServer; revision: number; create: boolean } | null>(null);
  const [argsText, setArgsText] = useState("");
  const [confirm, setConfirm] = useState<{ server: ManagedMcpServer; revision: number; op: "enable" | "test" | "delete" } | null>(null);
  const [trusted, setTrusted] = useState(false);
  const [diagnostic, setDiagnostic] = useState<McpDiagnostic | null>(null);
  const live = useRef(false);
  const generation = useRef(0);
  const busyRef = useRef(false);
  const trigger = useRef<HTMLElement | null>(null);
  const refreshButton = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    live.current = true;
    void load();
    return () => { live.current = false; generation.current++; };
  }, []);

  async function load() {
    const token = ++generation.current;
    setError(null); setDiagnostic(null);
    try { const next = await getHubToolSettings(); if (live.current && token === generation.current) setSettings(next); }
    catch (reason) { if (live.current && token === generation.current) setError(String(reason)); }
  }

  async function run(action: ToolSettingsAction | { op: "test"; id: string; revision: number }) {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setError(null);
    const token = ++generation.current;
    try {
      if (action.op === "test") {
        if (!sessionId) throw new Error(t("mcp.testSession"));
        const next = await testHubMcp(sessionId, action.id, action.revision, true);
        if (live.current && token === generation.current) { setDiagnostic(next); setConfirm(null); }
      } else {
        const next = await hubToolSettingsAction(action);
        if (live.current && token === generation.current) { setSettings(next); setEditor(null); setConfirm(null); setDiagnostic(null); }
      }
    } catch (reason) { if (live.current && token === generation.current) setError(String(reason)); }
    finally { if (live.current && token === generation.current) { busyRef.current = false; setBusy(false); } }
  }

  function openEditor(server: ManagedMcpServer | null, element: HTMLElement) {
    if (!settings) return;
    trigger.current = element; setError(null);
    setEditor({ server: server ? { ...server } : { ...blank }, revision: settings.revision, create: !server });
    setArgsText(server?.args.join("\n") ?? "");
  }
  function openConfirm(server: ManagedMcpServer, op: "enable" | "test" | "delete", element: HTMLElement) {
    if (!settings) return;
    trigger.current = element; setError(null); setTrusted(false);
    setConfirm({ server, revision: settings.revision, op });
  }
  const failure = error ? <Notice role="alert" tone="warning" heading={t("tools.error")} description={error} /> : null;
  const filter = query.trim().toLowerCase();
  const components = settings?.components.filter((c) => `${c.id} ${labels[c.id] ? t(labels[c.id]) : ""}`.toLowerCase().includes(filter)) ?? [];
  const servers = settings?.mcp_servers.filter((s) => `${s.id} ${s.display_name} ${s.command}`.toLowerCase().includes(filter)) ?? [];
  const restoreFocus = (event: Event) => { event.preventDefault(); const node = trigger.current?.isConnected ? trigger.current : refreshButton.current; node?.focus(); };
  const change = (key: "id" | "display_name" | "command", value: string) => setEditor((current) => current ? { ...current, server: { ...current.server, [key]: value } } : null);

  return <>
    <Notice tone="info" heading={t("tools.scope")} description={t("tools.boundary")} />
    <Button ref={refreshButton} disabled={busy} variant="ghost" onClick={() => void load()}>{t("common.refresh")}</Button>
    {!editor && !confirm ? failure : null}
    {!settings && !error ? <p role="status">{t("files.loading")}</p> : null}
    {settings ? <>
      <SettingsGroup heading={t("pages.settings.toolsGroup")} description={t("tools.notSecurity")}>
        {components.map((component) => <SettingsRow key={component.id} heading={labels[component.id] ? t(labels[component.id]) : component.id} control={<Switch disabled={busy} label={labels[component.id] ? t(labels[component.id]) : component.id} checked={component.enabled} onCheckedChange={(enabled) => void run({ op: "set_component", revision: settings.revision, id: component.id, enabled: enabled === true })} />} />)}
        {!components.length && filter ? <EmptyState title={t("extension.noMatches")} /> : null}
      </SettingsGroup>
      <SettingsGroup heading={t("pages.settings.mcpGroup")} description={t("mcp.boundary")}>
        <Button disabled={busy} onClick={(event) => openEditor(null, event.currentTarget)}>{t("mcp.add")}</Button>
        {!servers.length ? <EmptyState title={settings.mcp_servers.length ? t("extension.noMatches") : t("mcp.empty")} /> : null}
        {servers.map((server) => <SettingsRow key={server.id} heading={server.display_name} description={`${server.id} · ${server.command}`} control={<div className="hub-dialog-actions">
          <Badge tone={server.enabled ? "success" : "neutral"}>{server.enabled ? t("mcp.enabled") : t("mcp.disabled")}</Badge>
          <Button disabled={busy} onClick={(event) => openEditor(server, event.currentTarget)} variant="ghost">{t("mcp.edit")}</Button>
          <Button disabled={busy || !sessionId} title={!sessionId ? t("mcp.testSession") : undefined} onClick={(event) => openConfirm(server, "test", event.currentTarget)}>{t("mcp.test")}</Button>
          <Button disabled={busy} onClick={(event) => server.enabled ? void run({ op: "set_mcp_enabled", revision: settings.revision, id: server.id, enabled: false, confirmed: false }) : openConfirm(server, "enable", event.currentTarget)}>{server.enabled ? t("mcp.disable") : t("mcp.enable")}</Button>
          <Button disabled={busy} variant="ghost" onClick={(event) => openConfirm(server, "delete", event.currentTarget)}>{t("mcp.delete")}</Button>
        </div>} />)}
        {!sessionId ? <p>{t("mcp.testSession")}</p> : null}
      </SettingsGroup>
    </> : null}
    {diagnostic ? <Notice role="status" tone={diagnostic.ok ? "info" : "warning"} heading={diagnostic.ok ? t("mcp.testOk") : t("mcp.testFailed")}>
      <p>{diagnostic.server_id} · {diagnostic.latency_ms} ms · {diagnostic.tools.length} {t("mcp.tools")}</p>
      {!diagnostic.ok ? <p>{diagnostic.message}</p> : null}
      <p>{t("mcp.testClosed")}</p>
      {diagnostic.tools.length ? <pre>{diagnostic.tools.join("\n")}</pre> : null}
    </Notice> : null}
    <DialogRoot open={editor !== null} onOpenChange={(open) => { if (!open && !busy) { setEditor(null); setError(null); } }}>
      <DialogContent title={editor?.create ? t("mcp.add") : t("mcp.edit")} description={t("mcp.savePaused")} onCloseAutoFocus={restoreFocus}>
        {editor ? <div className="hub-form-stack">
          {failure}
          <FieldFrame label={t("mcp.id")}><TextField disabled={busy || !editor.create} aria-label={t("mcp.id")} value={editor.server.id} onChange={(event) => change("id", event.target.value)} /></FieldFrame>
          <FieldFrame label={t("mcp.name")}><TextField disabled={busy} aria-label={t("mcp.name")} value={editor.server.display_name} onChange={(event) => change("display_name", event.target.value)} /></FieldFrame>
          <FieldFrame label={t("mcp.command")}><TextField disabled={busy} aria-label={t("mcp.command")} value={editor.server.command} onChange={(event) => change("command", event.target.value)} /></FieldFrame>
          <FieldFrame label={t("mcp.args")}><TextArea disabled={busy} aria-label={t("mcp.args")} value={argsText} onChange={(event) => setArgsText(event.target.value)} /></FieldFrame>
          <FieldFrame label={t("mcp.timeout")}><TextField type="number" min={1} max={60} disabled={busy} aria-label={t("mcp.timeout")} value={editor.server.connect_timeout_secs} onChange={(event) => setEditor({ ...editor, server: { ...editor.server, connect_timeout_secs: Number(event.target.value) } })} /></FieldFrame>
          <div className="hub-dialog-actions"><Button disabled={busy} variant="ghost" onClick={() => { setEditor(null); setError(null); }}>{t("common.cancel")}</Button><Button disabled={busy || !editor.server.id || !editor.server.display_name.trim() || !editor.server.command.startsWith("/") || editor.server.connect_timeout_secs < 1 || editor.server.connect_timeout_secs > 60} onClick={() => void run({ op: editor.create ? "create_mcp" : "update_mcp", revision: editor.revision, server: { ...editor.server, args: argsText === "" ? [] : argsText.split("\n"), enabled: false } })}>{busy ? t("common.saving") : t("common.save")}</Button></div>
        </div> : null}
      </DialogContent>
    </DialogRoot>
    <DialogRoot open={confirm !== null} onOpenChange={(open) => { if (!open && !busy) { setConfirm(null); setError(null); } }}>
      <DialogContent title={confirm?.op === "delete" ? t("mcp.deleteTitle") : t("mcp.trustTitle")} description={confirm?.op === "delete" ? t("mcp.deleteDescription") : t("mcp.trustDescription")} onCloseAutoFocus={restoreFocus}>
        {confirm ? <div className="hub-form-stack">{failure}<strong>{confirm.server.display_name}</strong><pre>{[confirm.server.command, ...confirm.server.args].join("\n")}</pre>
          {confirm.op !== "delete" ? <Checkbox checked={trusted} disabled={busy} label={t("mcp.trustConfirm")} onCheckedChange={(value) => setTrusted(value === true)} /> : null}
          <div className="hub-dialog-actions"><Button disabled={busy} variant="ghost" onClick={() => { setConfirm(null); setError(null); }}>{t("common.cancel")}</Button><Button disabled={busy || (confirm.op !== "delete" && !trusted)} onClick={() => void run(confirm.op === "test" ? { op: "test", id: confirm.server.id, revision: confirm.revision } : confirm.op === "delete" ? { op: "delete_mcp", id: confirm.server.id, revision: confirm.revision } : { op: "set_mcp_enabled", id: confirm.server.id, revision: confirm.revision, enabled: true, confirmed: trusted })}>{busy ? t("mcp.waiting") : confirm.op === "test" ? t("mcp.test") : confirm.op === "delete" ? t("mcp.delete") : t("mcp.enable")}</Button></div>
        </div> : null}
      </DialogContent>
    </DialogRoot>
  </>;
}
