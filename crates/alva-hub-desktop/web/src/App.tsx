// INPUT:  React hooks, bridge workspace/run/timeline/session functions
// OUTPUT: engine selection, fallback authorization, durable branches, composer, timeline UI
// POS:    Primary Hub Desktop product surface for reversible, user-authorized engine sessions.

import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import {
  chooseWorkspace,
  getEngineReadiness,
  getGlobalFallbackPolicy,
  getHubSession,
  getPermissionEligibility,
  addTeamWorker,
  cancelWorkerTask,
  createTeamRun,
  dispatchWorkerTask,
  interruptWorker,
  updateWorkerTask,
  getRepairPlan,
  getTeamRunForLead,
  listHubSessions,
  listenTimeline,
  respondHubPermission,
  runEngineRepair,
  runHubPrompt,
  setGlobalFallbackPolicy,
  setSessionFallbackPolicy,
  setTranscriptSharing,
  stopHubAttempt,
  switchHubEngine,
  type EngineEligibility,
  type EngineReadiness,
  type EngineTarget,
  type FallbackPolicy,
  type HandoffPackage,
  type HubSessionDetail,
  type HubSessionSummary,
  type PermissionProfile,
  type ReadinessReason,
  type RepairAction,
  type ResourceClaim,
  type TeamRunDetail,
  type RepairKind,
  type TimelineEvent,
} from "./bridge";

interface FallbackControls {
  source_engine_id: string;
  handoff: HandoffPackage | null;
}

type ProjectedEvent = TimelineEvent & { fallback?: FallbackControls };

const engineCatalog: EngineTarget[] = [
  {
    engine_id: "alva",
    model_id: "workspace-config",
    provider_id: "workspace-config",
    account_source: "Workspace provider configuration",
    billing_route: "Workspace provider account",
    automatic_use_authorized: false,
    required_capabilities: [],
  },
  {
    engine_id: "claude",
    model_id: "engine-default",
    provider_id: "claude-cli",
    account_source: "Claude Code CLI account",
    billing_route: "Anthropic account via Claude CLI",
    automatic_use_authorized: false,
    required_capabilities: [],
  },
  {
    engine_id: "codex",
    model_id: "engine-default",
    provider_id: "codex-cli",
    account_source: "Codex CLI account",
    billing_route: "OpenAI account via Codex CLI",
    automatic_use_authorized: false,
    required_capabilities: [],
  },
];

function describeReadinessReason(reason: ReadinessReason): string {
  switch (reason.code) {
    case "binary_not_found":
      return `Not installed — searched ${reason.searched.join(", ") || "PATH"}.`;
    case "version_unreadable":
      return `Version check failed: ${reason.detail}`;
    case "version_incompatible":
      return `Version ${reason.found} is older than the required ${reason.minimum}.`;
    case "unauthenticated":
      return reason.remedy;
    case "provider_key_missing":
      return reason.remedy;
    case "stream_failing":
      return `Streaming is failing: ${reason.last_failure}`;
  }
}

const readinessBadge: Record<EngineReadiness["status"], string> = {
  healthy: "Healthy",
  missing: "Not installed",
  incompatible: "Incompatible",
  unauthenticated: "Sign-in needed",
  unhealthy: "Unhealthy",
};

const profileLabel: Record<PermissionProfile, string> = {
  interactive: "Ask before risky actions",
  workspace_write: "Workspace writes allowed",
  unrestricted: "Everything allowed",
};

function describeClaim(claim: ResourceClaim): string {
  switch (claim.kind) {
    case "path":
      return `${claim.mode} ${claim.path}`;
    case "facility":
      return `facility ${claim.name}`;
    case "exclusive_tool":
      return `tool ${claim.tool}`;
  }
}

/// `src/lib.rs, read:docs, facility:target-dir, tool:git` → claims.
function parseClaims(input: string): ResourceClaim[] {
  return input
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry): ResourceClaim => {
      if (entry.startsWith("facility:")) {
        return { kind: "facility", name: entry.slice("facility:".length) };
      }
      if (entry.startsWith("tool:")) {
        return { kind: "exclusive_tool", tool: entry.slice("tool:".length) };
      }
      if (entry.startsWith("read:")) {
        return { kind: "path", path: entry.slice("read:".length), mode: "read" };
      }
      return { kind: "path", path: entry, mode: "write" };
    });
}

function projectTimeline(detail: HubSessionDetail): ProjectedEvent[] {
  const turns = detail.turns.map((turn) => ({
    status:
      turn.status === "failed"
        ? ("failed" as const)
        : turn.status === "running"
          ? ("running" as const)
          : turn.status === "interrupted"
            ? ("interrupted" as const)
            : ("completed" as const),
    engine_id: turn.attempts.at(-1)?.engine_id ?? detail.session.engine_id,
    product_session_id: detail.session.product_session_id,
    message: [turn.prompt, turn.result ?? turn.error].filter(Boolean).join("\n\n"),
    created_at: turn.created_at,
  }));
  const switches = detail.timeline_events.map((event): ProjectedEvent => ({
    status:
      event.kind === "engine_switch_queued"
        ? ("queued" as const)
        : event.kind === "engine_switch_cancelled" ||
            event.kind === "attempt_cancelled"
          ? ("cancelled" as const)
          : event.kind === "engine_fallback" ||
              event.kind === "engine_fallback_skipped"
            ? ("fallback" as const)
            : event.kind === "fallback_exhausted"
              ? ("exhausted" as const)
              : event.kind === "worker_outcome" ||
                  event.kind === "resource_wait"
                ? ("worker" as const)
                : ("switched" as const),
    engine_id: event.target_engine_id,
    product_session_id: detail.session.product_session_id,
    message:
      event.kind === "engine_fallback_skipped"
        ? `Skipped ${event.target_engine_id}\n\n${event.reason}`
        : event.kind === "fallback_exhausted" ||
            event.kind === "attempt_cancelled" ||
            event.kind === "worker_outcome" ||
            event.kind === "resource_wait"
          ? event.reason
          : `${event.source_engine_id} → ${event.target_engine_id}\n\n${event.reason}`,
    created_at: event.created_at,
    fallback:
      event.kind === "engine_fallback"
        ? {
            source_engine_id: event.source_engine_id,
            handoff:
              detail.handoffs.find(
                (handoff) =>
                  handoff.target_branch_id === event.target_branch_id &&
                  handoff.created_at === event.created_at,
              ) ?? null,
          }
        : undefined,
  }));
  return [...turns, ...switches].sort(
    (left, right) => (left.created_at ?? 0) - (right.created_at ?? 0),
  );
}

export default function App() {
  const [workspace, setWorkspace] = useState("");
  const [prompt, setPrompt] = useState("");
  const [timeline, setTimeline] = useState<ProjectedEvent[]>([]);
  const [openHandoffs, setOpenHandoffs] = useState<Record<number, boolean>>({});
  const [sessions, setSessions] = useState<HubSessionSummary[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [engine, setEngine] = useState("alva");
  const [pendingEngine, setPendingEngine] = useState<string | null>(null);
  const [profile, setProfile] = useState<PermissionProfile>("workspace_write");
  const [transcriptSharing, setTranscriptSharingState] = useState(false);
  const [eligibility, setEligibility] = useState<EngineEligibility[]>([]);
  // request_id → outcome, so an answered approval card cannot be re-sent.
  const [answeredPermissions, setAnsweredPermissions] = useState<
    Record<string, "allowed" | "denied">
  >({});
  const [fallbackPolicy, setFallbackPolicy] = useState<FallbackPolicy>({
    targets: [],
    updated_at: 0,
  });
  const [policySaving, setPolicySaving] = useState(false);
  const [sessionError, setSessionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [readiness, setReadiness] = useState<EngineReadiness[]>([]);
  const [readinessBusy, setReadinessBusy] = useState(false);
  const [repairPlan, setRepairPlan] = useState<RepairAction[]>([]);
  const [teamRun, setTeamRun] = useState<TeamRunDetail | null>(null);
  const [teamScope, setTeamScope] = useState("");
  const [teamWorkerEngine, setTeamWorkerEngine] = useState("codex");
  const [teamBusy, setTeamBusy] = useState(false);
  const [dispatchWorkerId, setDispatchWorkerId] = useState<string | null>(null);
  const [teamClaims, setTeamClaims] = useState("");
  const [editingTask, setEditingTask] = useState<string | null>(null);
  const [editingScope, setEditingScope] = useState("");
  const [repairingEngine, setRepairingEngine] = useState<string | null>(null);
  const [repairLogs, setRepairLogs] = useState<Record<string, string>>({});
  // Logic value: identifies the newest in-flight readiness request so a stale
  // response can never overwrite a fresher one.
  const readinessRequestRef = useRef(0);
  // Logic value: the current lead session for team refreshes triggered from
  // the long-lived timeline listener (state would be a stale closure there).
  const teamLeadRef = useRef<string | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void listenTimeline((event) => {
      if (active) {
        setTimeline((current) => [...current, event]);
        // Worker pumps finish in the background — pull the fresh queue state.
        if (event.status === "worker" && teamLeadRef.current) {
          void getTeamRunForLead(teamLeadRef.current).then((run) => {
            if (active) setTeamRun(run);
          });
        }
      }
    }).then((stop) => {
      if (active) {
        unsubscribe = stop;
      } else {
        stop();
      }
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    void refreshSessions();
    void loadGlobalFallbackPolicy().catch(appendLocalFailure);
  }, []);

  useEffect(() => {
    void refreshReadiness(workspace || null);
  }, [workspace]);

  useEffect(() => {
    let active = true;
    // On a failed fetch the previous report is kept: an empty report would
    // silently re-enable ineligible engines (the backend still fails closed,
    // but the selector would mislead).
    void getPermissionEligibility(profile)
      .then((report) => {
        if (active) setEligibility(report);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [profile]);

  async function refreshReadiness(workspacePath: string | null) {
    const requestId = readinessRequestRef.current + 1;
    readinessRequestRef.current = requestId;
    setReadinessBusy(true);
    try {
      const [report, plan] = await Promise.all([
        getEngineReadiness(workspacePath),
        getRepairPlan(workspacePath),
      ]);
      if (readinessRequestRef.current === requestId) {
        setReadiness(report);
        setRepairPlan(plan);
      }
    } catch (error) {
      if (readinessRequestRef.current === requestId) {
        appendLocalFailure(error);
      }
    } finally {
      if (readinessRequestRef.current === requestId) {
        setReadinessBusy(false);
      }
    }
  }

  async function repairEngine(engineId: string, kind: RepairKind) {
    setRepairingEngine(engineId);
    try {
      const result = await runEngineRepair(engineId, kind, workspace || null);
      setRepairLogs((current) => ({
        ...current,
        [engineId]: result.outcome.success
          ? `Repair finished. ${result.outcome.log.trim()}`.trim()
          : `Repair failed. ${result.outcome.log.trim()}`.trim(),
      }));
    } catch (error) {
      // A refused repair (e.g. stale plan) lands on the same card as any
      // other outcome — never attributed to the selected engine.
      setRepairLogs((current) => ({
        ...current,
        [engineId]: `Repair failed. ${error instanceof Error ? error.message : String(error)}`,
      }));
    } finally {
      setRepairingEngine(null);
      // Re-probe on every outcome so the plan never goes stale.
      await refreshReadiness(workspace || null);
    }
  }

  const latest = timeline.at(-1);
  const statusLabel = useMemo(() => {
    switch (latest?.status) {
      case "running":
        return `${latest.engine_id} is working`;
      case "completed":
        return "Completed";
      case "failed":
        return "Failed";
      case "interrupted":
        return "Interrupted · ready to continue";
      case "queued":
        return "Engine switch queued";
      case "switched":
        return "Engine switched";
      case "cancelled":
        return "Engine switch cancelled";
      case "fallback":
        return "Automatic fallback";
      case "exhausted":
        return "Fallback exhausted";
      case "permission_required":
        return `${latest.engine_id} is waiting for your approval`;
      case "worker":
        return "Worker outcome";
      default:
        return "Ready";
    }
  }, [latest]);

  async function selectWorkspace() {
    try {
      const selected = await chooseWorkspace();
      if (selected) {
        setWorkspace(selected);
        setActiveSessionId(null);
        setEngine("alva");
        setPendingEngine(null);
        setProfile("workspace_write");
        setAnsweredPermissions({});
        setTimeline([]);
        await loadGlobalFallbackPolicy();
      }
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function refreshSessions() {
    try {
      setSessions(await listHubSessions());
      setSessionError(null);
    } catch (error) {
      setSessionError(error instanceof Error ? error.message : String(error));
    }
  }

  async function openSession(session: HubSessionSummary) {
    if (busy) return;
    try {
      await loadSession(session.product_session_id);
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function loadSession(productSessionId: string) {
    const detail = await getHubSession(productSessionId);
    if (!detail) throw new Error("This Hub session no longer exists.");
    setActiveSessionId(detail.session.product_session_id);
    setWorkspace(detail.session.workspace);
    setEngine(detail.session.engine_id);
    setProfile(detail.session.permission_profile);
    setTranscriptSharingState(detail.session.include_full_transcript);
    setAnsweredPermissions({});
    setPendingEngine(detail.pending_switch?.target.engine_id ?? null);
    setFallbackPolicy(detail.fallback_policy);
    setTimeline(projectTimeline(detail));
    // Last: a failed team fetch must not leave the session view half-loaded.
    teamLeadRef.current = detail.session.product_session_id;
    // A worker id from the previous run must never survive into this one.
    setDispatchWorkerId(null);
    setEditingTask(null);
    setTeamRun(await getTeamRunForLead(detail.session.product_session_id));
  }

  async function loadGlobalFallbackPolicy() {
    setFallbackPolicy(await getGlobalFallbackPolicy());
  }

  function updateFallbackTargets(
    update: (targets: EngineTarget[]) => EngineTarget[],
  ) {
    setFallbackPolicy((current) => ({
      ...current,
      targets: update(current.targets),
    }));
  }

  function toggleFallbackTarget(engineId: string) {
    updateFallbackTargets((targets) => {
      const exists = targets.some((target) => target.engine_id === engineId);
      if (exists) return targets.filter((target) => target.engine_id !== engineId);
      const catalogTarget = engineCatalog.find((target) => target.engine_id === engineId);
      return catalogTarget ? [...targets, { ...catalogTarget }] : targets;
    });
  }

  function updateFallbackTarget(index: number, update: Partial<EngineTarget>) {
    updateFallbackTargets((targets) =>
      targets.map((target, targetIndex) =>
        targetIndex === index ? { ...target, ...update } : target,
      ),
    );
  }

  function moveFallbackTarget(index: number, direction: -1 | 1) {
    updateFallbackTargets((targets) => {
      const targetIndex = index + direction;
      if (targetIndex < 0 || targetIndex >= targets.length) return targets;
      const next = [...targets];
      [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
      return next;
    });
  }

  async function saveFallbackPolicy() {
    setPolicySaving(true);
    try {
      const saved = activeSessionId
        ? await setSessionFallbackPolicy(activeSessionId, fallbackPolicy.targets)
        : await setGlobalFallbackPolicy(fallbackPolicy.targets);
      setFallbackPolicy(saved);
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setPolicySaving(false);
    }
  }

  async function selectEngine(engineId: string) {
    if (!activeSessionId) {
      setEngine(engineId);
      setPendingEngine(null);
      return;
    }
    try {
      const outcome = await switchHubEngine(
        activeSessionId,
        engineId,
        `User explicitly selected ${engineId}`,
      );
      setEngine(outcome.active_engine_id);
      setPendingEngine(outcome.disposition === "queued" ? engineId : null);
      await loadSession(activeSessionId);
      await refreshSessions();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  function appendLocalFailure(error: unknown) {
    setTimeline((current) => [
      ...current,
      {
        status: "failed",
        engine_id: engine,
        product_session_id: "setup",
        message: error instanceof Error ? error.message : String(error),
      },
    ]);
  }

  async function setTranscriptSharing_backend(enabled: boolean) {
    if (!activeSessionId) return;
    try {
      await setTranscriptSharing(activeSessionId, enabled);
    } catch (error) {
      // The store still holds the previous value — the display must follow
      // it, or the next handoff would contradict the shown consent.
      setTranscriptSharingState(!enabled);
      appendLocalFailure(error);
    }
  }

  async function createTeam() {
    if (!activeSessionId) return;
    setTeamBusy(true);
    try {
      setTeamRun(await createTeamRun(activeSessionId, teamWorkerEngine));
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setTeamBusy(false);
    }
  }

  async function dispatchTask() {
    const scope = teamScope.trim();
    if (!teamRun || !scope) return;
    // Only a worker of THIS run is a valid target; a stale selection falls
    // back to the first worker rather than being sent and rejected.
    const selected = teamRun.workers.find(
      (worker) => worker.worker_id === dispatchWorkerId,
    );
    const workerId = selected?.worker_id ?? teamRun.workers[0]?.worker_id ?? null;
    if (!workerId) return;
    setTeamBusy(true);
    try {
      await dispatchWorkerTask(
        teamRun.team_run_id,
        workerId,
        scope,
        parseClaims(teamClaims),
      );
      // Keep the typed scope on failure; clear only once queued.
      setTeamScope("");
      setTeamClaims("");
      setTeamRun(await getTeamRunForLead(teamRun.lead_session_id));
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setTeamBusy(false);
    }
  }

  async function refreshTeam() {
    if (!teamRun) return;
    try {
      setTeamRun(await getTeamRunForLead(teamRun.lead_session_id));
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function addWorker() {
    if (!teamRun) return;
    setTeamBusy(true);
    try {
      setTeamRun(await addTeamWorker(teamRun.team_run_id, teamWorkerEngine));
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setTeamBusy(false);
    }
  }

  async function saveTaskEdit(taskId: string) {
    try {
      await updateWorkerTask(taskId, editingScope);
      setEditingTask(null);
      await refreshTeam();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function cancelTask(taskId: string) {
    try {
      await cancelWorkerTask(taskId);
      await refreshTeam();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function openSessionById(productSessionId: string) {
    try {
      await loadSession(productSessionId);
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function interruptWorkerNow(workerSessionId: string) {
    try {
      await interruptWorker(workerSessionId);
      await refreshTeam();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function stopActiveAttempt() {
    if (!activeSessionId) return;
    try {
      await stopHubAttempt(activeSessionId);
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function switchBack(sourceEngineId: string) {
    await selectEngine(sourceEngineId);
  }

  async function answerPermission(
    productSessionId: string,
    requestId: string,
    allow: boolean,
  ) {
    try {
      await respondHubPermission(productSessionId, requestId, allow);
      setAnsweredPermissions((current) => ({
        ...current,
        [requestId]: allow ? "allowed" : "denied",
      }));
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = prompt.trim();
    if (!workspace) {
      appendLocalFailure(new Error("Choose a workspace before running an engine."));
      return;
    }
    if (!text || busy) return;

    setBusy(true);
    setPrompt("");
    try {
      const outcome = await runHubPrompt({
        prompt: text,
        workspace,
        engine_id: engine,
        product_session_id: activeSessionId,
        // The profile is fixed at session creation; resumes never restate it.
        permission_profile: activeSessionId ? null : profile,
      });
      setActiveSessionId(outcome.product_session_id);
      await refreshSessions();
      await loadSession(outcome.product_session_id);
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div>
          <div className="brand-mark">A</div>
          <h1>ALVA Hub</h1>
          <p>One project. Every engine.</p>
        </div>

        <section className="workspace-panel">
          <span className="eyebrow">Workspace</span>
          <strong>{workspace ? workspace.split("/").at(-1) : "Not selected"}</strong>
          <small>{workspace || "Choose a local project to begin."}</small>
          <button className="secondary-button" type="button" onClick={selectWorkspace}>
            Choose workspace
          </button>
        </section>

        <section className="session-panel">
          <div className="session-heading">
            <span className="eyebrow">Sessions</span>
            <small>{sessions.length}</small>
          </div>
          <div className="session-list">
            {sessionError ? (
              <small className="session-error">{sessionError}</small>
            ) : sessions.length === 0 ? (
              <small className="session-empty">Hub runs will appear here.</small>
            ) : (
              sessions.map((session) => (
                <button
                  className={`session-item ${
                    activeSessionId === session.product_session_id ? "active" : ""
                  }`}
                  key={session.product_session_id}
                  type="button"
                  onClick={() => void openSession(session)}
                >
                  <strong>{session.workspace.split("/").at(-1)}</strong>
                  <span>{session.last_prompt ?? "Untitled session"}</span>
                  <small>{session.engine_id} · {session.status}</small>
                </button>
              ))
            )}
          </div>
        </section>

        <section className="readiness-panel">
          <div className="session-heading">
            <span className="eyebrow">Engines</span>
            <button
              className="readiness-refresh"
              disabled={readinessBusy}
              onClick={() => void refreshReadiness(workspace || null)}
              type="button"
            >
              {readinessBusy ? "Checking…" : "Refresh"}
            </button>
          </div>
          <div className="readiness-list">
            {readiness.length === 0 ? (
              <small className="session-empty">
                Engine health will appear here.
              </small>
            ) : (
              readiness.map((engine) => (
                <div className="readiness-card" key={engine.engine_id}>
                  <div className="readiness-heading">
                    <strong>{engine.display_name}</strong>
                    <span className={`readiness-badge readiness-${engine.status}`}>
                      {readinessBadge[engine.status]}
                    </span>
                  </div>
                  <small>
                    {engine.version
                      ? `v${engine.version} · requires ≥ ${engine.minimum_version}`
                      : `requires ≥ ${engine.minimum_version}`}
                    {" · "}
                    {engine.authenticated === true
                      ? "signed in"
                      : engine.authenticated === false
                        ? "not signed in"
                        : "sign-in unknown"}
                    {" · stream "}
                    {engine.stream_health}
                  </small>
                  {engine.capabilities.length > 0 ? (
                    <small className="readiness-capabilities">
                      {engine.capabilities.join(" · ")}
                    </small>
                  ) : null}
                  {engine.reasons.map((reason, index) => (
                    <small className="readiness-reason" key={`${reason.code}-${index}`}>
                      {describeReadinessReason(reason)}
                    </small>
                  ))}
                  {repairPlan
                    .filter((action) => action.engine_id === engine.engine_id)
                    .map((action) => (
                      <div className="repair-action" key={`${action.engine_id}-${action.kind}`}>
                        {action.one_click ? (
                          <button
                            disabled={repairingEngine !== null}
                            onClick={() =>
                              void repairEngine(action.engine_id, action.kind)
                            }
                            type="button"
                          >
                            {repairingEngine === action.engine_id
                              ? "Working…"
                              : action.title}
                          </button>
                        ) : (
                          <>
                            <small>{action.title}</small>
                            {action.command ? (
                              <code>{action.command}</code>
                            ) : null}
                          </>
                        )}
                        {action.url ? (
                          <small>
                            <a href={action.url} rel="noreferrer" target="_blank">
                              Official guide
                            </a>
                          </small>
                        ) : null}
                      </div>
                    ))}
                  {repairLogs[engine.engine_id] ? (
                    <small className="repair-log">
                      {repairLogs[engine.engine_id]}
                    </small>
                  ) : null}
                </div>
              ))
            )}
          </div>
        </section>

        <section className="fallback-panel">
          <div className="session-heading">
            <span className="eyebrow">
              {activeSessionId ? "Session fallback chain" : "Default fallback chain"}
            </span>
            <small>{fallbackPolicy.targets.length}</small>
          </div>
          <p className="policy-note">
            Only checked targets may run automatically. Manual engine switching is always explicit.
          </p>
          <div className="policy-catalog">
            {engineCatalog.map((target) => {
              const selected = fallbackPolicy.targets.some(
                (candidate) => candidate.engine_id === target.engine_id,
              );
              return (
                <button
                  className={`policy-engine ${selected ? "selected" : ""}`}
                  key={target.engine_id}
                  onClick={() => toggleFallbackTarget(target.engine_id)}
                  type="button"
                >
                  {selected ? "✓ " : "+ "}{target.engine_id}
                </button>
              );
            })}
          </div>
          {fallbackPolicy.targets.map((target, index) => (
            <div className="policy-target" key={`${target.engine_id}-${target.model_id}`}>
              <div>
                <strong>{index + 1}. {target.engine_id}</strong>
                <small>{target.model_id} · {target.provider_id}</small>
              </div>
              <small>{target.account_source}</small>
              <small>{target.billing_route}</small>
              <label className="policy-authorize">
                <input
                  checked={target.automatic_use_authorized}
                  onChange={(event) =>
                    updateFallbackTarget(index, {
                      automatic_use_authorized: event.target.checked,
                    })
                  }
                  type="checkbox"
                />
                Authorize automatic use
              </label>
              <div className="policy-order">
                <button
                  aria-label={`Move ${target.engine_id} earlier`}
                  disabled={index === 0}
                  onClick={() => moveFallbackTarget(index, -1)}
                  type="button"
                >
                  ↑
                </button>
                <button
                  aria-label={`Move ${target.engine_id} later`}
                  disabled={index === fallbackPolicy.targets.length - 1}
                  onClick={() => moveFallbackTarget(index, 1)}
                  type="button"
                >
                  ↓
                </button>
              </div>
            </div>
          ))}
          <button
            className="secondary-button policy-save"
            disabled={policySaving}
            onClick={() => void saveFallbackPolicy()}
            type="button"
          >
            {policySaving ? "Saving…" : activeSessionId ? "Save session chain" : "Save default chain"}
          </button>
        </section>

        <section className="team-panel">
          <div className="session-heading">
            <span className="eyebrow">Team</span>
            <small>{teamRun ? `${teamRun.tasks.length} tasks` : ""}</small>
          </div>
          {!activeSessionId ? (
            <small className="session-empty">
              Open a session to lead a team run.
            </small>
          ) : !teamRun ? (
            <>
              <select
                aria-label="Worker engine"
                value={teamWorkerEngine}
                onChange={(event) => setTeamWorkerEngine(event.target.value)}
              >
                {engineCatalog.map((target) => (
                  <option key={target.engine_id} value={target.engine_id}>
                    Worker on {target.engine_id}
                  </option>
                ))}
              </select>
              <button
                className="secondary-button"
                disabled={teamBusy}
                onClick={() => void createTeam()}
                type="button"
              >
                {teamBusy ? "Creating…" : "Create team run"}
              </button>
            </>
          ) : (
            <>
              <small className="team-worker-line">Lead: {engine}</small>
              {teamRun.workers.map((worker) => {
                const workerTasks = teamRun.tasks.filter(
                  (task) => task.worker_id === worker.worker_id,
                );
                const workerRunning = workerTasks.some(
                  (task) => task.status === "running",
                );
                return (
                  <div className="team-worker" key={worker.worker_id}>
                    <div className="team-worker-heading">
                      <strong>Worker · {worker.engine_id}</strong>
                      <div className="team-worker-controls">
                        <button
                          onClick={() =>
                            void openSessionById(worker.worker_session_id)
                          }
                          type="button"
                        >
                          Focus
                        </button>
                        <button
                          disabled={!workerRunning}
                          onClick={() =>
                            void interruptWorkerNow(worker.worker_session_id)
                          }
                          type="button"
                        >
                          Interrupt
                        </button>
                      </div>
                    </div>
                    {workerTasks.map((task) => (
                      <div className="team-task" key={task.task_id}>
                        {editingTask === task.task_id ? (
                          <textarea
                            className="team-scope"
                            value={editingScope}
                            onChange={(event) =>
                              setEditingScope(event.target.value)
                            }
                          />
                        ) : (
                          <strong>{task.scope}</strong>
                        )}
                        <small>
                          {task.status}
                          {task.result ? ` — ${task.result}` : ""}
                        </small>
                        {task.changed_files.length > 0 ? (
                          <small>Files: {task.changed_files.join(", ")}</small>
                        ) : null}
                        {task.verification.length > 0 ? (
                          <small>
                            {task.verification.length} evidence entries
                          </small>
                        ) : null}
                        {task.claims.length > 0 ? (
                          <small className="team-claim-list">
                            Claims: {task.claims.map(describeClaim).join(", ")}
                          </small>
                        ) : null}
                        {task.status === "queued" ? (
                          <div className="team-worker-controls">
                            {editingTask === task.task_id ? (
                              <button
                                onClick={() => void saveTaskEdit(task.task_id)}
                                type="button"
                              >
                                Save
                              </button>
                            ) : (
                              <button
                                onClick={() => {
                                  setEditingTask(task.task_id);
                                  setEditingScope(task.scope);
                                }}
                                type="button"
                              >
                                Edit
                              </button>
                            )}
                            <button
                              onClick={() => void cancelTask(task.task_id)}
                              type="button"
                            >
                              Cancel
                            </button>
                          </div>
                        ) : null}
                      </div>
                    ))}
                  </div>
                );
              })}
              <select
                aria-label="Dispatch target worker"
                value={
                  teamRun.workers.some(
                    (worker) => worker.worker_id === dispatchWorkerId,
                  )
                    ? (dispatchWorkerId as string)
                    : (teamRun.workers[0]?.worker_id ?? "")
                }
                onChange={(event) => setDispatchWorkerId(event.target.value)}
              >
                {teamRun.workers.map((worker) => (
                  <option key={worker.worker_id} value={worker.worker_id}>
                    To worker on {worker.engine_id}
                  </option>
                ))}
              </select>
              <textarea
                aria-label="Worker task scope"
                className="team-scope"
                placeholder="Task to queue — or a message to the worker…"
                value={teamScope}
                onChange={(event) => setTeamScope(event.target.value)}
              />
              <input
                aria-label="Resource claims"
                className="team-claims"
                placeholder="Claims: src/lib.rs, read:docs, facility:target-dir"
                value={teamClaims}
                onChange={(event) => setTeamClaims(event.target.value)}
              />
              <button
                className="secondary-button"
                disabled={teamBusy || !teamScope.trim()}
                onClick={() => void dispatchTask()}
                type="button"
              >
                {teamBusy ? "Queueing…" : "Queue for worker"}
              </button>
              <div className="team-worker-controls">
                <select
                  aria-label="Additional worker engine"
                  value={teamWorkerEngine}
                  onChange={(event) => setTeamWorkerEngine(event.target.value)}
                >
                  {engineCatalog.map((target) => (
                    <option key={target.engine_id} value={target.engine_id}>
                      {target.engine_id}
                    </option>
                  ))}
                </select>
                <button
                  disabled={teamBusy}
                  onClick={() => void addWorker()}
                  type="button"
                >
                  Add worker
                </button>
              </div>
            </>
          )}
        </section>

        <div className="ticket-note">
          <span>Durable engine branches</span>
          <small>Alva · Claude · Codex · reversible handoff</small>
        </div>
      </aside>

      <main className="main-column">
        <header className="topbar">
          <div>
            <span className="eyebrow">Engine</span>
            <select
              aria-label="Engine"
              value={pendingEngine ?? engine}
              onChange={(event) => void selectEngine(event.target.value)}
            >
              {engineCatalog.map((target) => {
                const health = readiness.find(
                  (candidate) => candidate.engine_id === target.engine_id,
                );
                const engineEligibility = eligibility.find(
                  (candidate) => candidate.engine_id === target.engine_id,
                );
                const ineligible = engineEligibility?.eligible === false;
                const suffix = ineligible
                  ? " — ineligible for this permission profile"
                  : health && health.status !== "healthy"
                    ? ` — ${readinessBadge[health.status]}`
                    : "";
                const label =
                  health?.display_name ??
                  `${target.engine_id.charAt(0).toUpperCase()}${target.engine_id.slice(1)}`;
                return (
                  <option
                    disabled={ineligible && target.engine_id !== engine}
                    key={target.engine_id}
                    value={target.engine_id}
                  >
                    {`${label}${suffix}`}
                  </option>
                );
              })}
            </select>
            {pendingEngine ? <small>Queued after the current run</small> : null}
          </div>
          <div>
            <span className="eyebrow">Permissions</span>
            <select
              aria-label="Permission profile"
              disabled={activeSessionId !== null}
              value={profile}
              onChange={(event) =>
                setProfile(event.target.value as PermissionProfile)
              }
            >
              {(Object.keys(profileLabel) as PermissionProfile[]).map(
                (candidate) => (
                  <option key={candidate} value={candidate}>
                    {profileLabel[candidate]}
                  </option>
                ),
              )}
            </select>
            {activeSessionId ? (
              <label className="transcript-toggle">
                <input
                  checked={transcriptSharing}
                  onChange={(event) => {
                    const enabled = event.target.checked;
                    setTranscriptSharingState(enabled);
                    void setTranscriptSharing_backend(enabled);
                  }}
                  type="checkbox"
                />
                Attach filtered full transcript to future handoffs
              </label>
            ) : null}
            {activeSessionId ? (
              <small>Fixed for this session</small>
            ) : (
              (() => {
                const selected = eligibility.find(
                  (candidate) => candidate.engine_id === engine,
                );
                if (!selected) return null;
                return (
                  <small
                    className={
                      selected.eligible ? undefined : "eligibility-warning"
                    }
                  >
                    {selected.detail}
                  </small>
                );
              })()
            )}
          </div>
          <div className={`status-pill status-${latest?.status ?? "ready"}`}>
            <span />
            {statusLabel}
          </div>
        </header>

        <section className="timeline" aria-live="polite">
          {timeline.length === 0 ? (
            <div className="empty-state">
              <span className="empty-icon">↗</span>
              <h2>Start one continuous project session</h2>
              <p>
                Hub keeps each engine branch in private local storage, so you can
                switch engines and return without losing its native session.
              </p>
            </div>
          ) : (
            timeline.map((entry, index) => (
              <article
                className={`timeline-card timeline-${entry.status}`}
                key={`${entry.product_session_id}-${index}`}
              >
                <div className="timeline-meta">
                  <span>{entry.engine_id}</span>
                  <span>{entry.status}</span>
                </div>
                <p>
                  {entry.message ??
                    (entry.status === "running"
                      ? "Executing in the selected workspace…"
                      : "Run finished.")}
                </p>
                {entry.status === "interrupted" ? (
                  <small className="timeline-hint">
                    The previous runtime is gone. Continue safely from the composer below.
                  </small>
                ) : null}
                {entry.permission_request ? (
                  <div className="permission-card">
                    {entry.permission_request.description ? (
                      <small>{entry.permission_request.description}</small>
                    ) : null}
                    {answeredPermissions[entry.permission_request.request_id] ? (
                      <small className="permission-answered">
                        {answeredPermissions[
                          entry.permission_request.request_id
                        ] === "allowed"
                          ? "You allowed this request."
                          : "You denied this request."}
                      </small>
                    ) : (
                      <div className="timeline-controls">
                        <button
                          onClick={() =>
                            void answerPermission(
                              entry.product_session_id,
                              entry.permission_request?.request_id ?? "",
                              true,
                            )
                          }
                          type="button"
                        >
                          Allow
                        </button>
                        <button
                          onClick={() =>
                            void answerPermission(
                              entry.product_session_id,
                              entry.permission_request?.request_id ?? "",
                              false,
                            )
                          }
                          type="button"
                        >
                          Deny
                        </button>
                      </div>
                    )}
                  </div>
                ) : null}
                {entry.fallback ? (
                  <div className="timeline-controls">
                    <button
                      disabled={!busy || !activeSessionId}
                      onClick={() => void stopActiveAttempt()}
                      type="button"
                    >
                      Stop
                    </button>
                    <button
                      disabled={engine === entry.fallback.source_engine_id}
                      onClick={() =>
                        void switchBack(entry.fallback?.source_engine_id ?? "")
                      }
                      type="button"
                    >
                      Switch back to {entry.fallback.source_engine_id}
                    </button>
                    <button
                      disabled={!entry.fallback.handoff}
                      onClick={() =>
                        setOpenHandoffs((current) => ({
                          ...current,
                          [index]: !current[index],
                        }))
                      }
                      type="button"
                    >
                      {openHandoffs[index] ? "Hide handoff" : "View handoff"}
                    </button>
                  </div>
                ) : null}
                {entry.fallback?.handoff && openHandoffs[index] ? (
                  <div className="handoff-view">
                    <strong>
                      {entry.fallback.handoff.kind} handoff ·{" "}
                      {entry.fallback.handoff.source.engine_id} →{" "}
                      {entry.fallback.handoff.target.engine_id}
                      {entry.fallback.handoff.used_at
                        ? " · delivered"
                        : " · pending"}
                    </strong>
                    <small>Reason: {entry.fallback.handoff.reason}</small>
                    {entry.fallback.handoff.user_goal ? (
                      <small>Goal: {entry.fallback.handoff.user_goal}</small>
                    ) : null}
                    {entry.fallback.handoff.latest_instruction ? (
                      <small>
                        Latest instruction:{" "}
                        {entry.fallback.handoff.latest_instruction}
                      </small>
                    ) : null}
                    {entry.fallback.handoff.completed_work.map((work, workIndex) => (
                      <small key={`completed-${workIndex}`}>
                        ✓ {work.prompt} — {work.result}
                      </small>
                    ))}
                    {entry.fallback.handoff.pending_work.map((work, workIndex) => (
                      <small key={`pending-${workIndex}`}>
                        ⏳ {work.prompt} — {work.reason}
                      </small>
                    ))}
                    {entry.fallback.handoff.changed_files.length > 0 ? (
                      <small>
                        Files: {entry.fallback.handoff.changed_files.join(", ")}
                      </small>
                    ) : null}
                    {entry.fallback.handoff.workspace_summary ? (
                      <small>{entry.fallback.handoff.workspace_summary}</small>
                    ) : null}
                    {entry.fallback.handoff.transcript.length > 0 ? (
                      <>
                        <strong>
                          Filtered transcript (
                          {entry.fallback.handoff.transcript.length} turns)
                        </strong>
                        {entry.fallback.handoff.transcript.map(
                          (turn, turnIndex) => (
                            <small key={`transcript-${turnIndex}`}>
                              » {turn.prompt}
                              {turn.result ? ` — ${turn.result}` : ""}
                            </small>
                          ),
                        )}
                      </>
                    ) : (
                      <small>Transcript not shared (default).</small>
                    )}
                  </div>
                ) : null}
              </article>
            ))
          )}
        </section>

        {(() => {
          const selectedId = pendingEngine ?? engine;
          const health = readiness.find(
            (candidate) => candidate.engine_id === selectedId,
          );
          if (!health || health.status === "healthy") return null;
          return (
            <div className="composer-readiness" role="status">
              <strong>
                {health.display_name} · {readinessBadge[health.status]}
              </strong>
              {health.reasons.map((reason, index) => (
                <small key={`${reason.code}-${index}`}>
                  {describeReadinessReason(reason)}
                </small>
              ))}
            </div>
          );
        })()}

        <form className="composer" onSubmit={submit}>
          <textarea
            aria-label="Message"
            placeholder={`Ask ${pendingEngine ?? engine} to inspect, change, or explain this project…`}
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <div className="composer-footer">
            <span>{workspace ? "Workspace ready" : "Select a workspace first"}</span>
            <button
              className="primary-button"
              type="submit"
              disabled={busy || !prompt.trim() || !workspace}
            >
              {busy ? "Running…" : `Run with ${engine}`}
            </button>
          </div>
        </form>
      </main>
    </div>
  );
}
