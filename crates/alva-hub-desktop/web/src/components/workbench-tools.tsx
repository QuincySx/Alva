// INPUT:  Current Hub session detail, timeline, and team state
// OUTPUT: Files, text, Git, Review, Agent, Inspector, Tasks, Usage, Chrome, and Terminal views
// POS:    Auxiliary HubSession tools kept separate from conversation and shared workbench chrome

import { useMemo, useState } from "react";
import {
  Bot,
  CheckCircle2,
  CircleDot,
  FileCode2,
  FolderTree,
  GitBranch,
  Globe2,
  Play,
  TerminalSquare,
} from "lucide-react";
import type { HubSessionDetail, TeamRunDetail, WorkerTask } from "../bridge";
import type { HubTimelineEvent } from "./hub-timeline";
import { EngineInspectorPanel } from "./engine-inspector";
import { localizeSystemText, useI18n } from "../i18n";
import {
  Badge,
  Button,
  Disclosure,
  EmptyState,
  ListRow,
  PanelSection,
  TextField,
} from "../ui";

export function FilesTool({ detail }: { detail: HubSessionDetail | null }) {
  const { t } = useI18n();
  const files = useMemo(
    () => Array.from(new Set(detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects.map((effect) => effect.path).filter((path): path is string => Boolean(path)))) ?? [])),
    [detail],
  );
  return files.length === 0 ? (
    <EmptyState icon={<FolderTree size={18} />} title={t("tool.noChangedFiles")} description={t("tool.noChangedFilesDescription")} />
  ) : (
    <PanelSection heading={t("tool.changedFiles")} meta={`${files.length}`}>
      {files.map((file) => <ListRow heading={file.split("/").at(-1) ?? file} description={file} key={file} leading={<FileCode2 size={16} />} />)}
    </PanelSection>
  );
}

function changedFiles(detail: HubSessionDetail | null) {
  return Array.from(new Set(detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects.map((effect) => effect.path).filter((path): path is string => Boolean(path)))) ?? []));
}

export function TextBrowserTool({ detail }: { detail: HubSessionDetail | null }) {
  const { t } = useI18n();
  const files = useMemo(() => changedFiles(detail), [detail]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const activeFile = selectedFile && files.includes(selectedFile) ? selectedFile : files[0] ?? null;
  return (
    <div className="hub-text-browser">
      <PanelSection heading={t("tool.projectFiles")} meta={`${files.length}`}>
        {files.length === 0 ? <EmptyState icon={<FileCode2 size={18} />} title={t("tool.noPreview")} description={t("tool.noPreviewDescription")} /> : files.map((file) => (
          <ListRow heading={file.split("/").at(-1) ?? file} description={file} key={file} leading={<FileCode2 size={16} />} onClick={() => setSelectedFile(file)} />
        ))}
      </PanelSection>
      <PanelSection heading={activeFile?.split("/").at(-1) ?? t("tool.textPreview")} description={activeFile ?? t("tool.chooseFile")}>
        <pre className="hub-text-preview">{activeFile ? t("tool.previewConnected", { file: activeFile }) : t("tool.noFileSelected")}</pre>
      </PanelSection>
    </div>
  );
}

export function GitTool({ detail }: { detail: HubSessionDetail | null }) {
  const { t } = useI18n();
  const effects = detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects)) ?? [];
  return (
    <PanelSection description={t("tool.gitDescription")} heading={t("workbench.git")} meta={t("common.changeCount", { count: effects.length })}>
      {effects.length === 0 ? <EmptyState icon={<GitBranch size={18} />} title={t("tool.clean")} description={t("tool.cleanDescription")} /> : effects.map((effect) => (
        <ListRow
          description={effect.path ?? effect.tool_name}
          heading={effect.succeeded === false ? t("tool.failedChange") : t("tool.modified")}
          key={effect.tool_call_id}
          leading={<GitBranch size={16} />}
          meta={effect.duration_ms ? `${effect.duration_ms} ms` : t("tool.sessionChange")}
        />
      ))}
    </PanelSection>
  );
}

export function ReviewTool({ detail }: { detail: HubSessionDetail | null }) {
  const { t } = useI18n();
  const effects = detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects)) ?? [];
  return (
    <PanelSection description={t("tool.reviewDescription")} heading={t("workbench.review")}>
      {effects.length === 0 ? <EmptyState title={t("tool.nothingToReview")} /> : effects.map((effect) => (
        <ListRow
          heading={effect.tool_name}
          description={effect.path ?? t("tool.noPath")}
          key={effect.tool_call_id}
          leading={effect.succeeded === false ? <CircleDot size={16} /> : <CheckCircle2 size={16} />}
          meta={effect.duration_ms ? `${effect.duration_ms} ms` : t("common.recorded")}
        />
      ))}
    </PanelSection>
  );
}

export function AgentTool({ run, onFocus }: { run: TeamRunDetail | null; onFocus: (sessionId: string) => void }) {
  const { localize, t } = useI18n();
  if (!run || run.workers.length === 0) return <EmptyState icon={<Bot size={18} />} title={t("tool.noAgents")} description={t("tool.noAgentsDescription")} />;

  // Child-agent tree: Lead (root) → Workers → per-Worker Tasks.
  // `workers` carry solo identity; `tasks` attach to a worker by worker_id, so a
  // worker without any task still renders as a leaf, and tasks without a live
  // worker (should not happen post-restore) are surfaced under an orphan bucket.
  const workersById = new Map(run.workers.map((w) => [w.worker_id, w]));
  const tasksByWorker = new Map<string, WorkerTask[]>();
  const orphanTasks: WorkerTask[] = [];
  for (const task of run.tasks) {
    if (workersById.has(task.worker_id)) {
      const list = tasksByWorker.get(task.worker_id) ?? [];
      list.push(task);
      tasksByWorker.set(task.worker_id, list);
    } else {
      orphanTasks.push(task);
    }
  }

  return (
    <div className="hub-agent-tree">
      <PanelSection description={t("tool.agentsDescription")} heading={t("tool.agents")} meta={`${run.workers.length}`}>
        <Disclosure title={t("tool.leadAgent")} description={run.lead_session_id} defaultOpen>
          <div className="hub-agent-tree__children">
            {run.workers.map((worker) => {
              const tasks = tasksByWorker.get(worker.worker_id) ?? [];
              return (
                <Disclosure
                  description={`${worker.engine_id} · ${localize(worker.lifecycle)}`}
                  defaultOpen
                  key={worker.worker_id}
                  title={worker.worker_id}
                  actions={
                    <Button onClick={() => onFocus(worker.worker_session_id)} size="small" variant="ghost">
                      {t("common.open")}
                    </Button>
                  }
                >
                  <div className="hub-agent-tree__tasks">
                    {tasks.length === 0 ? (
                      <ListRow description={t("tool.noTasks")} heading={t("tool.noTasksDescription")} />
                    ) : (
                      tasks.map((task) => (
                        <ListRow
                          description={task.worker_id}
                          heading={task.scope}
                          key={task.task_id}
                          meta={<Badge>{localize(task.status)}</Badge>}
                        />
                      ))
                    )}
                  </div>
                </Disclosure>
              );
            })}
            {orphanTasks.length > 0 ? (
              <Disclosure title={t("tool.orphanTasks")} description={`${orphanTasks.length}`}>
                <div className="hub-agent-tree__tasks">
                  {orphanTasks.map((task) => (
                    <ListRow description={task.worker_id} heading={task.scope} key={task.task_id} meta={<Badge>{localize(task.status)}</Badge>} />
                  ))}
                </div>
              </Disclosure>
            ) : null}
          </div>
        </Disclosure>
      </PanelSection>
    </div>
  );
}

export function InspectorTool({ detail, events }: { detail: HubSessionDetail | null; events: HubTimelineEvent[] }) {
  const { locale, localize, t } = useI18n();
  const [expandedJson, setExpandedJson] = useState<Record<string, boolean>>({});
  return (
    <div className="hub-inspector-tool">
      <EngineInspectorPanel detail={detail} />
      <PanelSection description={t("tool.inspectorDescription")} heading={t("workbench.inspector")} meta={detail ? t("common.turnCount", { count: detail.turns.length }) : t("tool.noSession")}>
        {detail?.turns.map((turn) => (
          <Disclosure key={turn.turn_id} title={turn.prompt} description={`${localize(turn.status)} · ${t("common.attemptCount", { count: turn.attempts.length })}`}>
            <div className="hub-inspector-records">
              {turn.attempts.map((attempt) => (
                <Disclosure
                  key={attempt.attempt_id}
                  title={`${attempt.engine_id} · ${localize(attempt.status)}`}
                  description={attempt.failure_reason ? localizeSystemText(locale, attempt.failure_reason, t("system.operationFailed")) : attempt.branch_id}
                  meta={attempt.usage ? t("common.tokenCount", { count: attempt.usage.input_tokens + attempt.usage.output_tokens }) : t("tool.noUsage")}
                >
                  {attempt.usage ? (
                    <div className="hub-inspector-usage">
                      <small>In: {attempt.usage.input_tokens} Out: {attempt.usage.output_tokens} Turns: {attempt.usage.num_turns} Dur: {attempt.usage.duration_ms}ms {attempt.usage.total_cost_usd !== null ? `Cost: $${attempt.usage.total_cost_usd}` : ""}</small>
                    </div>
                  ) : null}
                  {attempt.side_effects.map((effect) => (
                    <Disclosure
                      key={effect.tool_call_id}
                      title={`${effect.tool_name} ${effect.succeeded === false ? "✗" : effect.succeeded === true ? "✓" : "…"}`}
                      description={effect.path ?? effect.tool_call_id}
                      meta={effect.duration_ms ? `${effect.duration_ms}ms` : undefined}
                    >
                      <div className="hub-inspector-tool-io">
                        <div><small>tool_call_id: {effect.tool_call_id}</small></div>
                        {effect.tool_input ? (
                          <div>
                            <small>tool_input:</small>
                            <pre style={{ whiteSpace: "pre-wrap", wordBreak: "break-all", fontSize: 11 }}>{expandedJson[effect.tool_call_id + ":in"] ? effect.tool_input : effect.tool_input.slice(0, 240) + (effect.tool_input.length > 240 ? "…" : "")}</pre>
                            {effect.tool_input.length > 240 ? <Button size="small" variant="ghost" onClick={() => setExpandedJson((m) => ({ ...m, [effect.tool_call_id + ":in"]: !m[effect.tool_call_id + ":in"] }))}>{expandedJson[effect.tool_call_id + ":in"] ? "Collapse" : "Expand"}</Button> : null}
                          </div>
                        ) : null}
                        {effect.tool_output ? (
                          <div>
                            <small>tool_output:</small>
                            <pre style={{ whiteSpace: "pre-wrap", wordBreak: "break-all", fontSize: 11 }}>{expandedJson[effect.tool_call_id + ":out"] ? effect.tool_output : effect.tool_output.slice(0, 240) + (effect.tool_output.length > 240 ? "…" : "")}</pre>
                            {effect.tool_output.length > 240 ? <Button size="small" variant="ghost" onClick={() => setExpandedJson((m) => ({ ...m, [effect.tool_call_id + ":out"]: !m[effect.tool_call_id + ":out"] }))}>{expandedJson[effect.tool_call_id + ":out"] ? "Collapse" : "Expand"}</Button> : null}
                          </div>
                        ) : null}
                      </div>
                    </Disclosure>
                  ))}
                  {attempt.side_effects.length === 0 ? <small style={{ opacity: 0.6 }}>No tool calls in this attempt.</small> : null}
                </Disclosure>
              ))}
            </div>
          </Disclosure>
        )) ?? <EmptyState title={t("tool.inspectSession")} />}
      </PanelSection>
      {detail?.handoffs.length ? (
        <PanelSection heading="Handoff envelope" meta={`${detail.handoffs.length}`} description="Full cross-engine envelope: transcript, memory, skills, MCP.">
          {detail.handoffs.map((h) => (
            <Disclosure key={h.handoff_id} title={`${h.kind} · ${h.source.engine_id} → ${h.target.engine_id}`} description={h.reason}>
              <div style={{ display: "grid", gap: 6, padding: 8 }}>
                <div><small>Goal: {h.user_goal ?? "—"}</small></div>
                <div><small>Completed: {h.completed_work.map((w) => `${w.prompt} — ${w.result}`).join("; ") || "—"}</small></div>
                <div><small>Pending: {h.pending_work.map((w) => w.prompt).join("; ") || "—"}</small></div>
                <div><small>Changed files: {h.changed_files.join(", ") || "—"}</small></div>
                <div><small>Skills: {h.available_skills.map((s) => s.path).join(", ") || "—"}</small></div>
                <div><small>MCP: {h.available_mcp.map((s) => `${s.id} (${s.transport})`).join(", ") || "—"}</small></div>
                <div><small>Memory: {h.session_memory.join("; ") || "—"}</small></div>
                <div><small>Transcript: {h.transcript.length} turns {h.transcript.length ? `(${h.transcript.map((tr) => tr.prompt.slice(0, 40)).join(" | ")})` : "(not shared)"}</small></div>
                {h.team_snapshot ? <div><small>Team: {h.team_snapshot.slice(0, 240)}</small></div> : null}
              </div>
            </Disclosure>
          ))}
        </PanelSection>
      ) : null}
      {detail?.branches.length ? (
        <PanelSection heading="Branches" meta={`${detail.branches.length}`} description="Engine branches with resource/worktree identity.">
          {detail.branches.map((b) => (
            <ListRow key={b.branch_id} heading={`${b.engine_id}/${b.model_id} · ${b.status}`} description={`${b.provider_id} · ${b.branch_id.slice(0, 8)}`} meta={b.engine_session_id ? `session ${b.engine_session_id.slice(0, 8)}` : "no session"} />
          ))}
        </PanelSection>
      ) : null}
      {detail?.timeline_events.length ? (
        <PanelSection heading="Timeline (permission / fallback lifecycle)" meta={`${detail.timeline_events.length}`}>
          {detail.timeline_events.map((ev) => (
            <ListRow key={ev.event_id} heading={`${ev.kind} · ${ev.source_engine_id} → ${ev.target_engine_id}`} description={ev.reason} meta={new Date(ev.created_at).toLocaleTimeString()} />
          ))}
        </PanelSection>
      ) : null}
      {detail?.memory.length ? (
        <PanelSection heading="Session memory" meta={`${detail.memory.length}`}>
          {detail.memory.map((m, i) => <ListRow key={i} heading={m} description={`mem-${i}`} />)}
        </PanelSection>
      ) : null}
      <PanelSection heading={t("tool.liveEvents")} meta={`${events.length}`}>
        {events.slice(-8).map((event, index) => <ListRow heading={`${event.engine_id} · ${localize(event.status)}`} description={event.message ? localize(event.message) : t("tool.noMessage")} key={`${event.product_session_id}-${index}`} />)}
        {events.some((e) => e.permission_request) ? <small style={{ opacity: 0.7 }}>Permission lifecycle visible in live events (permission_required → allowed/denied).</small> : null}
      </PanelSection>
    </div>
  );
}

export function TasksTool({ run }: { run: TeamRunDetail | null }) {
  const { localize, t } = useI18n();
  const tasks = run?.tasks ?? [];
  return tasks.length === 0 ? <EmptyState title={t("tool.noTasks")} description={t("tool.noTasksDescription")} /> : (
    <PanelSection heading={t("tool.backgroundTasks")} meta={`${tasks.length}`}>
      {tasks.map((task) => <ListRow heading={task.scope} description={task.worker_id} key={task.task_id} meta={<Badge>{localize(task.status)}</Badge>} />)}
    </PanelSection>
  );
}

export function UsageTool({ detail }: { detail: HubSessionDetail | null }) {
  const { t } = useI18n();
  const usage = detail?.turns.flatMap((turn) => turn.attempts.map((attempt) => attempt.usage).filter((entry): entry is NonNullable<typeof entry> => Boolean(entry))) ?? [];
  const totals = usage.reduce((sum, entry) => ({ input: sum.input + entry.input_tokens, output: sum.output + entry.output_tokens, duration: sum.duration + entry.duration_ms, cost: sum.cost + (entry.total_cost_usd ?? 0) }), { input: 0, output: 0, duration: 0, cost: 0 });
  return (
    <PanelSection description={t("tool.usageDescription")} heading={t("workbench.usage")}>
      <div className="hub-metric-grid">
        <div><strong>{totals.input.toLocaleString()}</strong><small>{t("tool.inputTokens")}</small></div>
        <div><strong>{totals.output.toLocaleString()}</strong><small>{t("tool.outputTokens")}</small></div>
        <div><strong>{Math.round(totals.duration / 1000)}s</strong><small>{t("tool.runtime")}</small></div>
        <div><strong>${totals.cost.toFixed(2)}</strong><small>{t("tool.reportedCost")}</small></div>
      </div>
    </PanelSection>
  );
}

export function BrowserTool() {
  const { t } = useI18n();
  const [url, setUrl] = useState("http://127.0.0.1:1421");
  const [loadedUrl, setLoadedUrl] = useState(url);
  return (
    <PanelSection description={t("tool.browserDescription")} heading={t("workbench.browser")}>
      <div className="hub-browser-toolbar"><TextField aria-label={t("tool.browserAddress")} value={url} onChange={(event) => setUrl(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") setLoadedUrl(url); }} /><Button leadingIcon={<Globe2 size={14} />} onClick={() => setLoadedUrl(url)} size="small">{t("common.open")}</Button></div>
      <EmptyState icon={<Globe2 size={18} />} title={t("tool.browserReady")} description={loadedUrl} />
    </PanelSection>
  );
}

export function TerminalTool() {
  const { t } = useI18n();
  const [command, setCommand] = useState("");
  const [lines, setLines] = useState<string[]>([t("tool.terminalWelcome"), t("tool.terminalChooseWorkspace")]);
  function run() {
    if (!command.trim()) return;
    setLines((current) => [...current, `$ ${command}`, t("tool.commandQueued")]);
    setCommand("");
  }
  return (
    <PanelSection description={t("tool.terminalDescription")} heading={t("workbench.terminal")}>
      <pre className="hub-terminal-output">{lines.join("\n")}</pre>
      <div className="hub-terminal-input"><TerminalSquare size={16} /><TextField aria-label={t("tool.terminalCommand")} value={command} onChange={(event) => setCommand(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") run(); }} /><Button aria-label={t("tool.runCommand")} onClick={run} size="icon"><Play size={14} /></Button></div>
    </PanelSection>
  );
}
