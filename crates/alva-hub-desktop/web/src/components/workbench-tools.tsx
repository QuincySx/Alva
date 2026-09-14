// INPUT:  Current Hub session detail, timeline, and team state
// OUTPUT: Files, text, Git, Review, Agent, Inspector, Tasks, Usage, Chrome, and Terminal views
// POS:    Auxiliary HubSession tools kept separate from conversation and shared workbench chrome

import { useEffect, useMemo, useState } from "react";
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
import type { HubSessionDetail, StoredAttempt, TeamRunDetail, WorkerTask } from "../bridge";
import type { HubTimelineEvent } from "./hub-timeline";
import { EngineInspectorPanel, type EngineInspectorPanelProps } from "./engine-inspector";
import { localizeSystemText, useI18n } from "../i18n";
import {
  Badge,
  Button,
  Disclosure,
  EmptyState,
  ListRow,
  Notice,
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
        {/* R53: nothing reads the file here — claiming a connected preview was a
            fake connection. State the gap instead of inventing content. */}
        <EmptyState icon={<FileCode2 size={18} />} title={t("tool.previewNotWiredTitle")} description={activeFile ? t("tool.previewNotWired", { file: activeFile }) : t("tool.noFileSelected")} />
      </PanelSection>
    </div>
  );
}

export function GitTool({ detail }: { detail: HubSessionDetail | null }) {
  const { t } = useI18n();
  const effects = detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects)) ?? [];
  return (
    // R54: these rows are recorded TOOL executions, not a Git status. A
    // read-only tool call is not a modification, an empty session is not a
    // clean tree, and a running tool is pending — never inferred.
    <PanelSection description={t("tool.recordedPathsDescription")} heading={t("tool.recordedPaths")} meta={t("common.changeCount", { count: effects.length })}>
      {effects.length === 0 ? <EmptyState icon={<GitBranch size={18} />} title={t("tool.noToolRecords")} description={t("tool.noToolRecordsDescription")} /> : effects.map((effect) => (
        <ListRow
          description={effect.path ?? effect.tool_name}
          heading={effect.succeeded === false ? t("tool.failedChange") : effect.succeeded === true ? t("tool.executedRow") : t("tool.pendingRow")}
          key={effect.tool_call_id}
          leading={<GitBranch size={16} />}
          meta={effect.duration_ms ? `${effect.duration_ms} ms` : effect.succeeded === null || effect.succeeded === undefined ? t("tool.pendingRow") : t("tool.sessionChange")}
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
          leading={effect.succeeded === true ? <CheckCircle2 size={16} /> : effect.succeeded === false ? <CircleDot size={16} /> : <CircleDot size={16} />}
          meta={effect.succeeded === true ? t("tool.executedRow") : effect.succeeded === false ? t("tool.failedChange") : t("tool.pendingRow")}
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

export function InspectorTool({ detail, events, loadInspection }: { detail: HubSessionDetail | null; events: HubTimelineEvent[]; loadInspection?: EngineInspectorPanelProps["loadInspection"] }) {
  const { locale, localize, t } = useI18n();
  const [expandedJson, setExpandedJson] = useState<Record<string, boolean>>({});
  // R33: selecting a product attempt locates its native session in the
  // Inspector above; engines without native records surface an explicit
  // unavailable state instead of a silent no-op.
  const [focusSession, setFocusSession] = useState<string | null>(null);
  const [unavailableEngine, setUnavailableEngine] = useState<string | null>(null);
  // R42: the execution overview lives HERE (Inspector surface), not as a
  // swim lane in the chat column. A summary row opens and locates the
  // matching persisted tool record below.
  const [focusToolCallId, setFocusToolCallId] = useState<string | null>(null);
  const activityEvents = useMemo(
    () => events.filter((event) => Boolean(event.activity)),
    [events],
  );
  const toolRecorded = (toolCallId: string | null | undefined): boolean => {
    if (!toolCallId || !detail) return false;
    return detail.turns.some((turn) =>
      turn.attempts.some((attempt) =>
        attempt.side_effects.some((effect) => effect.tool_call_id === toolCallId),
      ),
    );
  };
  const locateToolEvent = (event: HubTimelineEvent) => {
    const id = event.item_id;
    if (!id) return;
    setFocusToolCallId(id);
    const attempt = detail?.turns
      .flatMap((turn) => turn.attempts)
      .find((candidate) => candidate.side_effects.some((effect) => effect.tool_call_id === id));
    if (attempt) {
      const branch = detail?.branches.find((candidate) => candidate.branch_id === attempt.branch_id) ?? null;
      if (branch?.engine_id === "alva" && branch.engine_session_id) {
        setUnavailableEngine(null);
        setFocusSession(branch.engine_session_id);
      } else {
        setUnavailableEngine(branch?.engine_id ?? attempt.engine_id);
      }
    }
  };
  useEffect(() => {
    if (!focusToolCallId) return;
    // jsdom has no layout; guard the scroll call.
    document
      .querySelector(`[data-inspector-tool="${focusToolCallId}"]`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [focusToolCallId]);
  const locateAttempt = (attempt: StoredAttempt) => {
    const branch = detail?.branches.find((candidate) => candidate.branch_id === attempt.branch_id) ?? null;
    if (branch?.engine_id === "alva" && branch.engine_session_id) {
      setUnavailableEngine(null);
      setFocusSession(branch.engine_session_id);
    } else {
      setUnavailableEngine(branch?.engine_id ?? attempt.engine_id);
    }
  };
  return (
    <div className="hub-inspector-tool">
      <EngineInspectorPanel
        detail={detail}
        loadInspection={loadInspection}
        focusSession={focusSession}
        onFocusConsumed={() => setFocusSession(null)}
      />
      {unavailableEngine ? (
        <Notice
          description={t("workbench.attemptNativeUnavailable", { engine: unavailableEngine })}
          heading={t("workbench.locateInInspector")}
          tone="warning"
        />
      ) : null}
      <PanelSection description={t("workbench.executionOverviewDescription")} heading={t("workbench.executionOverview")} meta={`${activityEvents.length}`}>
        {activityEvents.length === 0 ? (
          <EmptyState icon={<CircleDot size={18} />} title={t("workbench.executionOverviewEmpty")} description={t("workbench.executionOverviewEmptyDescription")} />
        ) : (
          activityEvents.map((event, index) => {
            const activity = event.activity!;
            const recorded = toolRecorded(event.item_id);
            const running = activity.succeeded === null || activity.succeeded === undefined;
            return (
              <ListRow
                description={running ? t("workbench.toolNotRecordedYet") : event.message ?? undefined}
                heading={`${activity.tool_name} · ${running ? t("tool.pendingRow") : activity.succeeded ? t("tool.executedRow") : t("tool.failedChange")}`}
                key={`${event.product_session_id}-${event.item_id ?? index}`}
                leading={running ? <CircleDot size={16} /> : activity.succeeded ? <CheckCircle2 size={16} /> : <CircleDot size={16} />}
                meta={activity.duration_ms ? `${activity.duration_ms} ms` : undefined}
                actions={
                  <Button
                    aria-label={t("workbench.locateRecord")}
                    disabled={!recorded}
                    onClick={() => locateToolEvent(event)}
                    size="small"
                    variant="ghost"
                  >
                    {t("workbench.locateRecord")}
                  </Button>
                }
              />
            );
          })
        )}
      </PanelSection>
      <Disclosure defaultOpen={false} description={t("workbench.productAttemptsDescription")} title={t("workbench.productAttempts")} meta={detail ? t("common.turnCount", { count: detail.turns.length }) : t("tool.noSession")}>
        {detail?.turns.map((turn) => (
          <Disclosure key={turn.turn_id} title={turn.prompt} description={`${localize(turn.status)} · ${t("common.attemptCount", { count: turn.attempts.length })}`}>
            <div className="hub-inspector-records">
              {turn.attempts.map((attempt) => (
                <Disclosure
                  key={attempt.attempt_id}
                  title={`${attempt.engine_id} · ${localize(attempt.status)}`}
                  description={attempt.failure_reason ? localizeSystemText(locale, attempt.failure_reason, t("system.operationFailed")) : attempt.branch_id}
                  meta={attempt.usage ? t("common.tokenCount", { count: attempt.usage.input_tokens + attempt.usage.output_tokens }) : t("tool.noUsage")}
                  actions={
                    <Button onClick={() => locateAttempt(attempt)} size="small" variant="ghost">
                      {t("workbench.locateInInspector")}
                    </Button>
                  }
                >
                  {attempt.usage ? (
                    <div className="hub-inspector-usage">
                      <small>In: {attempt.usage.input_tokens} Out: {attempt.usage.output_tokens} Turns: {attempt.usage.num_turns} Dur: {attempt.usage.duration_ms}ms {attempt.usage.total_cost_usd !== null ? `Cost: $${attempt.usage.total_cost_usd}` : ""}</small>
                    </div>
                  ) : null}
                  {attempt.side_effects.map((effect) => (
                    <Disclosure
                      data-inspector-tool={effect.tool_call_id}
                      key={effect.tool_call_id}
                      title={`${effect.tool_name} ${effect.succeeded === false ? "✗" : effect.succeeded === true ? "✓" : "…"}`}
                      description={effect.path ?? effect.tool_call_id}
                      meta={effect.duration_ms ? `${effect.duration_ms}ms` : undefined}
                      open={focusToolCallId === effect.tool_call_id ? true : undefined}
                      onOpenChange={focusToolCallId === effect.tool_call_id ? () => setFocusToolCallId(null) : undefined}
                      className={focusToolCallId === effect.tool_call_id ? "hub-inspector-record--focused" : undefined}
                    >
                      <div className="hub-inspector-tool-io">
                        <div><small>tool_call_id: {effect.tool_call_id}</small></div>
                        {effect.tool_input ? (
                          <div>
                            <small>tool_input:</small>
                            <pre style={{ whiteSpace: "pre-wrap", wordBreak: "break-all", fontSize: "var(--font-size-micro)" }}>{expandedJson[effect.tool_call_id + ":in"] ? effect.tool_input : effect.tool_input.slice(0, 240) + (effect.tool_input.length > 240 ? "…" : "")}</pre>
                            {effect.tool_input.length > 240 ? <Button size="small" variant="ghost" onClick={() => setExpandedJson((m) => ({ ...m, [effect.tool_call_id + ":in"]: !m[effect.tool_call_id + ":in"] }))}>{expandedJson[effect.tool_call_id + ":in"] ? "Collapse" : "Expand"}</Button> : null}
                          </div>
                        ) : null}
                        {effect.tool_output ? (
                          <div>
                            <small>tool_output:</small>
                            <pre style={{ whiteSpace: "pre-wrap", wordBreak: "break-all", fontSize: "var(--font-size-micro)" }}>{expandedJson[effect.tool_call_id + ":out"] ? effect.tool_output : effect.tool_output.slice(0, 240) + (effect.tool_output.length > 240 ? "…" : "")}</pre>
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
      </Disclosure>
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
  // R55: an unreported cost is not a free run. Distinguish "no attempt
  // reported anything", "genuinely $0", and "partial — some attempts did
  // not report"; never render null as $0.00.
  const reportedCosts = usage.filter((entry) => entry.total_cost_usd !== null && entry.total_cost_usd !== undefined);
  const allUnknown = usage.length > 0 && reportedCosts.length === 0;
  const partial = reportedCosts.length > 0 && reportedCosts.length < usage.length;
  const costValue = usage.length === 0
    ? "—"
    : allUnknown
      ? t("tool.costNotReported")
      : `$${totals.cost.toFixed(2)}${partial ? ` ${t("tool.costPartial")}` : ""}`;
  return (
    <PanelSection description={t("tool.usageDescription")} heading={t("workbench.usage")}>
      <div className="hub-metric-grid">
        <div><strong>{totals.input.toLocaleString()}</strong><small>{t("tool.inputTokens")}</small></div>
        <div><strong>{totals.output.toLocaleString()}</strong><small>{t("tool.outputTokens")}</small></div>
        <div><strong>{Math.round(totals.duration / 1000)}s</strong><small>{t("tool.runtime")}</small></div>
        <div><strong>{costValue}</strong><small>{t("tool.reportedCost")}</small></div>
      </div>
    </PanelSection>
  );
}

export function BrowserTool() {
  const { t } = useI18n();
  // R53: nothing loads here — no WebView bridge exists behind this panel.
  // Present the disabled capability honestly instead of a fake "ready" state.
  const [url, setUrl] = useState("");
  return (
    <PanelSection description={t("tool.browserDescription")} heading={t("workbench.browser")}>
      <EmptyState icon={<Globe2 size={18} />} title={t("tool.browserNotWiredTitle")} description={t("tool.browserNotWired")} />
      <div className="hub-browser-toolbar">
        <TextField aria-label={t("tool.browserAddress")} disabled value={url} onChange={(event) => setUrl(event.target.value)} />
        <Button disabled leadingIcon={<Globe2 size={14} />} size="small">{t("common.open")}</Button>
      </div>
    </PanelSection>
  );
}

export function TerminalTool() {
  const { t } = useI18n();
  // R53: the native terminal adapter is not wired in — typed commands were
  // echoed back as "queued" without ever running. Keep the surface disabled
  // and say so.
  const [command, setCommand] = useState("");
  return (
    <PanelSection description={t("tool.terminalDescription")} heading={t("workbench.terminal")}>
      <pre className="hub-terminal-output">{[t("tool.terminalWelcome"), t("tool.terminalNotWired")].join("\n")}</pre>
      <div className="hub-terminal-input"><TerminalSquare size={16} /><TextField aria-label={t("tool.terminalCommand")} disabled value={command} onChange={(event) => setCommand(event.target.value)} /><Button aria-label={t("tool.runCommand")} disabled size="icon"><Play size={14} /></Button></div>
    </PanelSection>
  );
}
