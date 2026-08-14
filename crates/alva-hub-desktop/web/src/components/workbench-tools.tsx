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
import type { HubSessionDetail, TeamRunDetail } from "../bridge";
import type { HubTimelineEvent } from "./hub-timeline";
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
  return (
    <PanelSection description={t("tool.agentsDescription")} heading={t("tool.agents")} meta={`${run.workers.length}`}>
      {run.workers.map((worker) => (
        <ListRow
          actions={<Button onClick={() => onFocus(worker.worker_session_id)} size="small" variant="ghost">{t("common.open")}</Button>}
          description={worker.worker_session_id}
          heading={worker.worker_id}
          key={worker.worker_id}
          leading={<Bot size={16} />}
          meta={<Badge tone={worker.lifecycle === "active" ? "success" : "neutral"}>{localize(worker.lifecycle)}</Badge>}
        />
      ))}
    </PanelSection>
  );
}

export function InspectorTool({ detail, events }: { detail: HubSessionDetail | null; events: HubTimelineEvent[] }) {
  const { locale, localize, t } = useI18n();
  return (
    <div className="hub-inspector-tool">
      <PanelSection description={t("tool.inspectorDescription")} heading={t("workbench.inspector")} meta={detail ? t("common.turnCount", { count: detail.turns.length }) : t("tool.noSession")}>
        {detail?.turns.map((turn) => (
          <Disclosure key={turn.turn_id} title={turn.prompt} description={`${localize(turn.status)} · ${t("common.attemptCount", { count: turn.attempts.length })}`}>
            <div className="hub-inspector-records">
              {turn.attempts.map((attempt) => (
                <ListRow
                  description={attempt.failure_reason ? localizeSystemText(locale, attempt.failure_reason, t("system.operationFailed")) : attempt.branch_id}
                  heading={`${attempt.engine_id} · ${localize(attempt.status)}`}
                  key={attempt.attempt_id}
                  meta={attempt.usage ? t("common.tokenCount", { count: attempt.usage.input_tokens + attempt.usage.output_tokens }) : t("tool.noUsage")}
                />
              ))}
            </div>
          </Disclosure>
        )) ?? <EmptyState title={t("tool.inspectSession")} />}
      </PanelSection>
      <PanelSection heading={t("tool.liveEvents")} meta={`${events.length}`}>
        {events.slice(-8).map((event, index) => <ListRow heading={`${event.engine_id} · ${localize(event.status)}`} description={event.message ? localize(event.message) : t("tool.noMessage")} key={`${event.product_session_id}-${index}`} />)}
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
