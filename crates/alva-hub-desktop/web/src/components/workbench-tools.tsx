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
  const files = useMemo(
    () => Array.from(new Set(detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects.map((effect) => effect.path).filter((path): path is string => Boolean(path)))) ?? [])),
    [detail],
  );
  return files.length === 0 ? (
    <EmptyState icon={<FolderTree size={18} />} title="No changed files" description="Files touched by this HubSession will appear here." />
  ) : (
    <PanelSection heading="Changed files" meta={`${files.length}`}>
      {files.map((file) => <ListRow heading={file.split("/").at(-1) ?? file} description={file} key={file} leading={<FileCode2 size={16} />} />)}
    </PanelSection>
  );
}

function changedFiles(detail: HubSessionDetail | null) {
  return Array.from(new Set(detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects.map((effect) => effect.path).filter((path): path is string => Boolean(path)))) ?? []));
}

export function TextBrowserTool({ detail }: { detail: HubSessionDetail | null }) {
  const files = useMemo(() => changedFiles(detail), [detail]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const activeFile = selectedFile && files.includes(selectedFile) ? selectedFile : files[0] ?? null;
  return (
    <div className="hub-text-browser">
      <PanelSection heading="Project files" meta={`${files.length}`}>
        {files.length === 0 ? <EmptyState icon={<FileCode2 size={18} />} title="No files to preview" description="Files reported by this session appear here." /> : files.map((file) => (
          <ListRow heading={file.split("/").at(-1) ?? file} description={file} key={file} leading={<FileCode2 size={16} />} onClick={() => setSelectedFile(file)} />
        ))}
      </PanelSection>
      <PanelSection heading={activeFile?.split("/").at(-1) ?? "Text preview"} description={activeFile ?? "Choose a file"}>
        <pre className="hub-text-preview">{activeFile ? `File preview is connected to the native workspace adapter.\n\n${activeFile}` : "No file selected."}</pre>
      </PanelSection>
    </div>
  );
}

export function GitTool({ detail }: { detail: HubSessionDetail | null }) {
  const effects = detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects)) ?? [];
  return (
    <PanelSection description="Session-attributed working tree activity. Native history remains owned by the Git adapter." heading="Git" meta={`${effects.length} changes`}>
      {effects.length === 0 ? <EmptyState icon={<GitBranch size={18} />} title="Working tree is clean" description="Changes made by this HubSession will appear here." /> : effects.map((effect) => (
        <ListRow
          description={effect.path ?? effect.tool_name}
          heading={effect.succeeded === false ? "Failed change" : "Modified"}
          key={effect.tool_call_id}
          leading={<GitBranch size={16} />}
          meta={effect.duration_ms ? `${effect.duration_ms} ms` : "Session change"}
        />
      ))}
    </PanelSection>
  );
}

export function ReviewTool({ detail }: { detail: HubSessionDetail | null }) {
  const effects = detail?.turns.flatMap((turn) => turn.attempts.flatMap((attempt) => attempt.side_effects)) ?? [];
  return (
    <PanelSection description="Side effects and verification evidence from the current session." heading="Review">
      {effects.length === 0 ? <EmptyState title="Nothing to review" /> : effects.map((effect) => (
        <ListRow
          heading={effect.tool_name}
          description={effect.path ?? "No file path reported"}
          key={effect.tool_call_id}
          leading={effect.succeeded === false ? <CircleDot size={16} /> : <CheckCircle2 size={16} />}
          meta={effect.duration_ms ? `${effect.duration_ms} ms` : "Recorded"}
        />
      ))}
    </PanelSection>
  );
}

export function AgentTool({ run, onFocus }: { run: TeamRunDetail | null; onFocus: (sessionId: string) => void }) {
  if (!run || run.workers.length === 0) return <EmptyState icon={<Bot size={18} />} title="No child agents" description="Workers linked to this HubSession will appear here." />;
  return (
    <PanelSection description="Child-agent relationships, not execution debug traces." heading="Agents" meta={`${run.workers.length}`}>
      {run.workers.map((worker) => (
        <ListRow
          actions={<Button onClick={() => onFocus(worker.worker_session_id)} size="small" variant="ghost">Open</Button>}
          description={worker.worker_session_id}
          heading={worker.worker_id}
          key={worker.worker_id}
          leading={<Bot size={16} />}
          meta={<Badge tone={worker.lifecycle === "active" ? "success" : "neutral"}>{worker.lifecycle}</Badge>}
        />
      ))}
    </PanelSection>
  );
}

export function InspectorTool({ detail, events }: { detail: HubSessionDetail | null; events: HubTimelineEvent[] }) {
  return (
    <div className="hub-inspector-tool">
      <PanelSection description="Execution records are grouped by Turn and Attempt. Agent relationships live in the Agent tab." heading="Inspector" meta={detail ? `${detail.turns.length} turns` : "No session"}>
        {detail?.turns.map((turn) => (
          <Disclosure key={turn.turn_id} title={turn.prompt} description={`${turn.status} · ${turn.attempts.length} attempts`}>
            <div className="hub-inspector-records">
              {turn.attempts.map((attempt) => (
                <ListRow
                  description={attempt.failure_reason ?? attempt.branch_id}
                  heading={`${attempt.engine_id} · ${attempt.status}`}
                  key={attempt.attempt_id}
                  meta={attempt.usage ? `${attempt.usage.input_tokens + attempt.usage.output_tokens} tokens` : "No usage"}
                />
              ))}
            </div>
          </Disclosure>
        )) ?? <EmptyState title="Open a session to inspect it" />}
      </PanelSection>
      <PanelSection heading="Live event summary" meta={`${events.length}`}>
        {events.slice(-8).map((event, index) => <ListRow heading={`${event.engine_id} · ${event.status}`} description={event.message ?? "No additional message"} key={`${event.product_session_id}-${index}`} />)}
      </PanelSection>
    </div>
  );
}

export function TasksTool({ run }: { run: TeamRunDetail | null }) {
  const tasks = run?.tasks ?? [];
  return tasks.length === 0 ? <EmptyState title="No background tasks" description="Queued and running worker tasks will appear here." /> : (
    <PanelSection heading="Background tasks" meta={`${tasks.length}`}>
      {tasks.map((task) => <ListRow heading={task.scope} description={task.worker_id} key={task.task_id} meta={<Badge>{task.status}</Badge>} />)}
    </PanelSection>
  );
}

export function UsageTool({ detail }: { detail: HubSessionDetail | null }) {
  const usage = detail?.turns.flatMap((turn) => turn.attempts.map((attempt) => attempt.usage).filter((entry): entry is NonNullable<typeof entry> => Boolean(entry))) ?? [];
  const totals = usage.reduce((sum, entry) => ({ input: sum.input + entry.input_tokens, output: sum.output + entry.output_tokens, duration: sum.duration + entry.duration_ms, cost: sum.cost + (entry.total_cost_usd ?? 0) }), { input: 0, output: 0, duration: 0, cost: 0 });
  return (
    <PanelSection description="Truthful totals from attempts that reported usage." heading="Usage">
      <div className="hub-metric-grid">
        <div><strong>{totals.input.toLocaleString()}</strong><small>Input tokens</small></div>
        <div><strong>{totals.output.toLocaleString()}</strong><small>Output tokens</small></div>
        <div><strong>{Math.round(totals.duration / 1000)}s</strong><small>Runtime</small></div>
        <div><strong>${totals.cost.toFixed(2)}</strong><small>Reported cost</small></div>
      </div>
    </PanelSection>
  );
}

export function BrowserTool() {
  const [url, setUrl] = useState("http://127.0.0.1:1421");
  const [loadedUrl, setLoadedUrl] = useState(url);
  return (
    <PanelSection description="Chrome-compatible WebView sessions open inside this tab." heading="Chrome">
      <div className="hub-browser-toolbar"><TextField aria-label="Browser address" value={url} onChange={(event) => setUrl(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") setLoadedUrl(url); }} /><Button leadingIcon={<Globe2 size={14} />} onClick={() => setLoadedUrl(url)} size="small">Open</Button></div>
      <EmptyState icon={<Globe2 size={18} />} title="Chrome WebView ready" description={loadedUrl} />
    </PanelSection>
  );
}

export function TerminalTool() {
  const [command, setCommand] = useState("");
  const [lines, setLines] = useState<string[]>(["ALVA Hub terminal", "Select a workspace to start a native shell."]);
  function run() {
    if (!command.trim()) return;
    setLines((current) => [...current, `$ ${command}`, "Command queued for the native terminal adapter."]);
    setCommand("");
  }
  return (
    <PanelSection description="The visual shell is complete; native process execution remains behind the terminal adapter boundary." heading="Terminal">
      <pre className="hub-terminal-output">{lines.join("\n")}</pre>
      <div className="hub-terminal-input"><TerminalSquare size={16} /><TextField aria-label="Terminal command" value={command} onChange={(event) => setCommand(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") run(); }} /><Button aria-label="Run terminal command" onClick={run} size="icon"><Play size={14} /></Button></div>
    </PanelSection>
  );
}
