// INPUT:  Team run state, worker/task form state, and explicit Hub action callbacks
// OUTPUT: Worker lifecycle, queued task, worktree adoption, and autonomy controls
// POS:    Team tab business block inside the Hub workbench

import { useState } from "react";
import type {
  AdoptionPreview,
  EngineTarget,
  ResourceClaim,
  TeamRunDetail,
} from "../bridge";
import { Button, Checkbox, SelectField, TextArea, TextField } from "../ui";

export interface TeamPanelProps {
  activeSession: boolean;
  adoption: Record<string, AdoptionPreview | undefined>;
  busy: boolean;
  catalog: EngineTarget[];
  claims: string;
  dispatchWorkerId: string | null;
  engineId: string;
  isolateWorker: boolean;
  onAddWorker: () => void;
  onAdopt: (workerId: string) => void;
  onCancelTask: (taskId: string) => void;
  onClaimsChange: (claims: string) => void;
  onCreate: () => void;
  onDiscard: (workerId: string) => void;
  onDispatch: () => void;
  onDispatchWorkerChange: (workerId: string) => void;
  onEnd: () => void;
  onFocusWorker: (sessionId: string) => void;
  onInterruptWorker: (sessionId: string) => void;
  onIsolateWorkerChange: (isolated: boolean) => void;
  onLifecycleChange: (workerId: string, action: "hibernate" | "restore" | "archive") => void;
  onReviewAdoption: (workerId: string) => void;
  onScopeChange: (scope: string) => void;
  onToggleAutonomy: (enabled: boolean) => void;
  onUpdateTask: (taskId: string, scope: string) => void;
  onWorkerEngineChange: (engineId: string) => void;
  run: TeamRunDetail | null;
  scope: string;
  workerEngine: string;
  describeClaim: (claim: ResourceClaim) => string;
}

export function TeamPanel({
  activeSession,
  adoption,
  busy,
  catalog,
  claims,
  dispatchWorkerId,
  engineId,
  isolateWorker,
  onAddWorker,
  onAdopt,
  onCancelTask,
  onClaimsChange,
  onCreate,
  onDiscard,
  onDispatch,
  onDispatchWorkerChange,
  onEnd,
  onFocusWorker,
  onInterruptWorker,
  onIsolateWorkerChange,
  onLifecycleChange,
  onReviewAdoption,
  onScopeChange,
  onToggleAutonomy,
  onUpdateTask,
  onWorkerEngineChange,
  run,
  scope,
  workerEngine,
  describeClaim,
}: TeamPanelProps) {
  const [collapsed, setCollapsed] = useState(true);
  const [editingTask, setEditingTask] = useState<string | null>(null);
  const [editingScope, setEditingScope] = useState("");
  const [showArchived, setShowArchived] = useState(false);

  return (
    <section className="team-panel" aria-labelledby="team-panel-title">
      <div className="session-heading">
        <Button
          aria-expanded={!collapsed}
          aria-label="Toggle team panel"
          className="section-collapse"
          onClick={() => setCollapsed((value) => !value)}
          size="icon"
          variant="ghost"
        >
          {collapsed ? "▸" : "▾"}
        </Button>
        <span className="eyebrow" id="team-panel-title">Team</span>
        <small>{run ? `${run.tasks.length} tasks` : ""}</small>
      </div>
      {collapsed ? null : !activeSession ? (
        <small className="session-empty">Open a session to lead a team run.</small>
      ) : !run ? (
        <>
          <SelectField
            ariaLabel="Worker engine"
            onValueChange={onWorkerEngineChange}
            options={catalog.map((target) => ({
              label: `Worker on ${target.engine_id}`,
              value: target.engine_id,
            }))}
            value={workerEngine}
          />
          <Button className="secondary-button" loading={busy} onClick={onCreate}>
            {busy ? "Creating…" : "Create team run"}
          </Button>
        </>
      ) : (
        <>
          <small className="team-worker-line">
            Lead: {engineId}{run.status !== "active" ? ` · run ${run.status}` : ""}
          </small>
          {run.workers
            .filter((worker) => showArchived || worker.lifecycle !== "archived")
            .map((worker) => {
              const workerTasks = run.tasks.filter((task) => task.worker_id === worker.worker_id);
              const workerRunning = workerTasks.some((task) => task.status === "running");
              const preview = adoption[worker.worker_id];
              return (
                <div className="team-worker" key={worker.worker_id}>
                  <div className="team-worker-heading">
                    <strong>
                      Worker · {worker.engine_id}
                      {worker.worktree_path ? <span className="worker-isolated">isolated</span> : null}
                      {worker.lifecycle !== "active" ? (
                        <span className="worker-lifecycle">{worker.lifecycle}</span>
                      ) : null}
                    </strong>
                    <div className="team-worker-controls">
                      <Button onClick={() => onFocusWorker(worker.worker_session_id)} size="small" variant="ghost">Focus</Button>
                      <Button
                        disabled={!workerRunning || worker.lifecycle !== "active"}
                        onClick={() => onInterruptWorker(worker.worker_session_id)}
                        size="small"
                        variant="ghost"
                      >Interrupt</Button>
                      {worker.lifecycle === "active" ? (
                        <>
                          <Button onClick={() => onLifecycleChange(worker.worker_id, "hibernate")} size="small" variant="ghost">Hibernate</Button>
                          <Button onClick={() => onLifecycleChange(worker.worker_id, "archive")} size="small" variant="ghost">Archive</Button>
                        </>
                      ) : (
                        <Button onClick={() => onLifecycleChange(worker.worker_id, "restore")} size="small" variant="ghost">Restore</Button>
                      )}
                    </div>
                  </div>
                  {worker.worktree_branch ? (
                    <small className="team-worker-line">
                      Worktree {worker.worktree_branch} — changes stay here until you adopt them.
                    </small>
                  ) : null}
                  {worker.worktree_path ? (
                    <div className="team-worker-controls">
                      <Button onClick={() => onReviewAdoption(worker.worker_id)} size="small" variant="ghost">Review changes</Button>
                      <Button disabled={!preview} onClick={() => onAdopt(worker.worker_id)} size="small">Adopt</Button>
                      <Button onClick={() => onDiscard(worker.worker_id)} size="small" variant="danger">Discard worktree</Button>
                    </div>
                  ) : null}
                  {preview ? (
                    <div className="adoption-preview">
                      <strong>
                        {preview.changes.length} change(s)
                        {preview.applies_cleanly ? " · applies cleanly" : " · does NOT apply cleanly"}
                      </strong>
                      {preview.changes.map((change) => <small key={change.path}>{change.status} {change.path}</small>)}
                      {preview.conflicts.length > 0 ? (
                        <small className="adoption-conflict">
                          Conflicts with your workspace edits: {preview.conflicts.join(", ")}
                        </small>
                      ) : null}
                      {preview.verification.map((entry, index) => <small key={`verify-${index}`}>{entry}</small>)}
                    </div>
                  ) : null}
                  {workerTasks.map((task) => (
                    <div className="team-task" key={task.task_id}>
                      {editingTask === task.task_id ? (
                        <TextArea className="team-scope" value={editingScope} onChange={(event) => setEditingScope(event.target.value)} />
                      ) : <strong>{task.scope}</strong>}
                      <small>{task.status}{task.result ? ` — ${task.result}` : ""}</small>
                      {task.changed_files.length > 0 ? <small>Files: {task.changed_files.join(", ")}</small> : null}
                      {task.verification.length > 0 ? <small>{task.verification.length} evidence entries</small> : null}
                      {task.claims.length > 0 ? <small className="team-claim-list">Claims: {task.claims.map(describeClaim).join(", ")}</small> : null}
                      {task.status === "queued" ? (
                        <div className="team-worker-controls">
                          {editingTask === task.task_id ? (
                            <Button onClick={() => { onUpdateTask(task.task_id, editingScope); setEditingTask(null); }} size="small">Save</Button>
                          ) : (
                            <Button onClick={() => { setEditingTask(task.task_id); setEditingScope(task.scope); }} size="small" variant="ghost">Edit</Button>
                          )}
                          <Button onClick={() => onCancelTask(task.task_id)} size="small" variant="ghost">Cancel</Button>
                        </div>
                      ) : null}
                    </div>
                  ))}
                </div>
              );
            })}
          <SelectField
            ariaLabel="Dispatch target worker"
            onValueChange={onDispatchWorkerChange}
            options={run.workers.filter((worker) => worker.lifecycle !== "archived").map((worker) => ({
              label: `To worker on ${worker.engine_id}${worker.lifecycle === "hibernated" ? " (hibernated)" : ""}`,
              value: worker.worker_id,
            }))}
            value={run.workers.some((worker) => worker.worker_id === dispatchWorkerId && worker.lifecycle !== "archived")
              ? (dispatchWorkerId as string)
              : (run.workers.find((worker) => worker.lifecycle !== "archived")?.worker_id ?? "")}
          />
          <TextArea aria-label="Worker task scope" className="team-scope" placeholder="Task to queue — or a message to the worker…" value={scope} onChange={(event) => onScopeChange(event.target.value)} />
          <TextField aria-label="Resource claims" className="team-claims" placeholder="Claims: src/lib.rs, read:docs, facility:target-dir" value={claims} onChange={(event) => onClaimsChange(event.target.value)} />
          <Button className="secondary-button" disabled={!scope.trim()} loading={busy} onClick={onDispatch}>
            {busy ? "Queueing…" : "Queue for worker"}
          </Button>
          <div className="team-worker-controls">
            <SelectField
              ariaLabel="Additional worker engine"
              onValueChange={onWorkerEngineChange}
              options={catalog.map((target) => ({ label: target.engine_id, value: target.engine_id }))}
              value={workerEngine}
            />
            <Checkbox checked={isolateWorker} label="Isolated worktree" onCheckedChange={(checked) => onIsolateWorkerChange(checked === true)} />
            <Button disabled={busy || run.status !== "active"} onClick={onAddWorker} size="small">Add worker</Button>
            <Button disabled={busy || run.status !== "active"} onClick={onEnd} size="small" variant="danger">End run</Button>
            <Checkbox checked={run.autonomous} label="Autonomous run (no per-request approval)" onCheckedChange={(checked) => onToggleAutonomy(checked === true)} />
            <Checkbox checked={showArchived} label="Show archived" onCheckedChange={(checked) => setShowArchived(checked === true)} />
          </div>
        </>
      )}
    </section>
  );
}
