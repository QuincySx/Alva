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
import { Button, Checkbox, PanelSection, SelectField, TextArea, TextField } from "../ui";
import { useI18n } from "../i18n";

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
  const { localize, t } = useI18n();
  const [editingTask, setEditingTask] = useState<string | null>(null);
  const [editingScope, setEditingScope] = useState("");
  const [showArchived, setShowArchived] = useState(false);

  return (
    <PanelSection
      className="team-panel"
      description={t("team.description")}
      heading={t("workbench.team")}
      meta={run ? t("common.taskCount", { count: run.tasks.length }) : undefined}
    >
      {!activeSession ? (
        <small className="session-empty">{t("team.openSession")}</small>
      ) : !run ? (
        <>
          <SelectField
            ariaLabel={t("team.workerEngine")}
            onValueChange={onWorkerEngineChange}
            options={catalog.map((target) => ({
              label: t("team.workerOn", { engine: target.engine_id }),
              value: target.engine_id,
            }))}
            value={workerEngine}
          />
          <Button className="secondary-button" loading={busy} onClick={onCreate}>
            {busy ? t("team.creating") : t("team.create")}
          </Button>
        </>
      ) : (
        <>
          <small className="team-worker-line">
            {t("team.lead", { engine: engineId })}{run.status !== "active" ? ` · ${t("team.runStatus", { status: localize(run.status) })}` : ""}
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
                      {t("team.worker", { engine: worker.engine_id })}
                      {worker.worktree_path ? <span className="worker-isolated">{t("team.isolated")}</span> : null}
                      {worker.lifecycle !== "active" ? (
                        <span className="worker-lifecycle">{localize(worker.lifecycle)}</span>
                      ) : null}
                    </strong>
                    <div className="team-worker-controls">
                      <Button onClick={() => onFocusWorker(worker.worker_session_id)} size="small" variant="ghost">{t("team.focus")}</Button>
                      <Button
                        disabled={!workerRunning || worker.lifecycle !== "active"}
                        onClick={() => onInterruptWorker(worker.worker_session_id)}
                        size="small"
                        variant="ghost"
                      >{t("team.interrupt")}</Button>
                      {worker.lifecycle === "active" ? (
                        <>
                          <Button onClick={() => onLifecycleChange(worker.worker_id, "hibernate")} size="small" variant="ghost">{t("team.hibernate")}</Button>
                          <Button onClick={() => onLifecycleChange(worker.worker_id, "archive")} size="small" variant="ghost">{t("team.archive")}</Button>
                        </>
                      ) : (
                        <Button onClick={() => onLifecycleChange(worker.worker_id, "restore")} size="small" variant="ghost">{t("team.restore")}</Button>
                      )}
                    </div>
                  </div>
                  {worker.worktree_branch ? (
                    <small className="team-worker-line">
                      {t("team.worktree", { branch: worker.worktree_branch })}
                    </small>
                  ) : null}
                  {worker.worktree_path ? (
                    <div className="team-worker-controls">
                      <Button onClick={() => onReviewAdoption(worker.worker_id)} size="small" variant="ghost">{t("team.reviewChanges")}</Button>
                      <Button disabled={!preview} onClick={() => onAdopt(worker.worker_id)} size="small">{t("team.adopt")}</Button>
                      <Button onClick={() => onDiscard(worker.worker_id)} size="small" variant="danger">{t("team.discard")}</Button>
                    </div>
                  ) : null}
                  {preview ? (
                    <div className="adoption-preview">
                      <strong>
                        {t("common.changeCount", { count: preview.changes.length })}
                        {preview.applies_cleanly ? ` · ${t("team.appliesCleanly")}` : ` · ${t("team.notApplyCleanly")}`}
                      </strong>
                      {preview.changes.map((change) => <small key={change.path}>{change.status} {change.path}</small>)}
                      {preview.conflicts.length > 0 ? (
                        <small className="adoption-conflict">
                          {t("team.conflicts", { files: preview.conflicts.join(", ") })}
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
                      <small>{localize(task.status)}{task.result ? ` — ${task.result}` : ""}</small>
                      {task.changed_files.length > 0 ? <small>{t("team.files", { files: task.changed_files.join(", ") })}</small> : null}
                      {task.verification.length > 0 ? <small>{t("common.evidenceCount", { count: task.verification.length })}</small> : null}
                      {task.claims.length > 0 ? <small className="team-claim-list">{t("team.claims", { claims: task.claims.map(describeClaim).join(", ") })}</small> : null}
                      {task.status === "queued" ? (
                        <div className="team-worker-controls">
                          {editingTask === task.task_id ? (
                            <Button onClick={() => { onUpdateTask(task.task_id, editingScope); setEditingTask(null); }} size="small">{t("common.save")}</Button>
                          ) : (
                            <Button onClick={() => { setEditingTask(task.task_id); setEditingScope(task.scope); }} size="small" variant="ghost">{t("common.edit")}</Button>
                          )}
                          <Button onClick={() => onCancelTask(task.task_id)} size="small" variant="ghost">{t("common.cancel")}</Button>
                        </div>
                      ) : null}
                    </div>
                  ))}
                </div>
              );
            })}
          <SelectField
            ariaLabel={t("team.dispatchTarget")}
            onValueChange={onDispatchWorkerChange}
            options={run.workers.filter((worker) => worker.lifecycle !== "archived").map((worker) => ({
              label: `${t("team.toWorker", { engine: worker.engine_id })}${worker.lifecycle === "hibernated" ? ` (${localize(worker.lifecycle)})` : ""}`,
              value: worker.worker_id,
            }))}
            value={run.workers.some((worker) => worker.worker_id === dispatchWorkerId && worker.lifecycle !== "archived")
              ? (dispatchWorkerId as string)
              : (run.workers.find((worker) => worker.lifecycle !== "archived")?.worker_id ?? "")}
          />
          <TextArea aria-label={t("team.taskScope")} className="team-scope" placeholder={t("team.taskPlaceholder")} value={scope} onChange={(event) => onScopeChange(event.target.value)} />
          <TextField aria-label={t("team.resourceClaims")} className="team-claims" placeholder={t("team.claimsPlaceholder")} value={claims} onChange={(event) => onClaimsChange(event.target.value)} />
          <Button className="secondary-button" disabled={!scope.trim()} loading={busy} onClick={onDispatch}>
            {busy ? t("team.queueing") : t("team.queue")}
          </Button>
          <div className="team-worker-controls">
            <SelectField
              ariaLabel={t("team.additionalEngine")}
              onValueChange={onWorkerEngineChange}
              options={catalog.map((target) => ({ label: target.engine_id, value: target.engine_id }))}
              value={workerEngine}
            />
            <Checkbox checked={isolateWorker} label={t("team.isolatedWorktree")} onCheckedChange={(checked) => onIsolateWorkerChange(checked === true)} />
            <Button disabled={busy || run.status !== "active"} onClick={onAddWorker} size="small">{t("team.addWorker")}</Button>
            <Button disabled={busy || run.status !== "active"} onClick={onEnd} size="small" variant="danger">{t("team.endRun")}</Button>
            <Checkbox checked={run.autonomous} label={t("team.autonomous")} onCheckedChange={(checked) => onToggleAutonomy(checked === true)} />
            <Checkbox checked={showArchived} label={t("team.showArchived")} onCheckedChange={(checked) => setShowArchived(checked === true)} />
          </div>
        </>
      )}
    </PanelSection>
  );
}
