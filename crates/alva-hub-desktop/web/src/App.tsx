// INPUT:  React hooks, bridge workspace/run/timeline/session functions
// OUTPUT: engine selection, fallback authorization, durable branches, composer, timeline UI
// POS:    Primary Hub Desktop product surface for reversible, user-authorized engine sessions.

import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import {
  addSessionMemory,
  chooseWorkspace,
  getEngineReadiness,
  getGlobalFallbackPolicy,
  getHubSession,
  getPermissionEligibility,
  getPermissionGroups,
  respondPermissionGroup,
  setTeamAutonomy,
  addTeamWorker,
  adoptWorkerWork,
  archiveWorker,
  completeTeamRun,
  hibernateWorker,
  restoreWorker,
  discardWorkerWorktree,
  previewWorkerAdoption,
  cancelWorkerTask,
  createTeamRun,
  dispatchWorkerTask,
  interruptWorker,
  updateWorkerTask,
  getRepairPlan,
  openExternal,
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
  type PermissionGroup,
  type PermissionProfile,
  type ReadinessReason,
  type RepairAction,
  type AdoptionPreview,
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

export function describeReadinessReason(reason: ReadinessReason): string {
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

export function describeClaim(claim: ResourceClaim): string {
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
/// "now-relative" label for a session's updated_at (e.g. "2m ago",
/// "3h ago", "2024-08-01"). Pure and testable.
export function formatRelativeTime(updatedAt: number, now: number = Date.now()): string {
  if (!Number.isFinite(updatedAt) || !Number.isFinite(now)) return "";
  const delta = Math.max(0, now - updatedAt);
  const min = Math.floor(delta / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min}m ago`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return new Date(updatedAt).toISOString().slice(0, 10);
}

export function parseClaims(input: string): ResourceClaim[] {
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

export function projectTimeline(detail: HubSessionDetail): ProjectedEvent[] {
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
              : event.kind === "permission_decision"
                ? ("permission_required" as const)
                : event.kind === "worker_outcome" ||
                  event.kind === "resource_wait" ||
                  event.kind === "worktree_adopted" ||
                  event.kind === "worktree_discarded"
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
            event.kind === "permission_decision" ||
            event.kind === "resource_wait" ||
            event.kind === "worktree_adopted" ||
            event.kind === "worktree_discarded"
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
  // Drawer handoff expansion (by handoff_id) for the full cross-engine
  // envelope: memory, skills, MCP pointers and filtered transcript.
  const [openDrawerHandoffs, setOpenDrawerHandoffs] = useState<
    Record<string, boolean>
  >({});
  // Sidebar sections the user can collapse to reclaim space for sessions.
  // Fallback chain and team start collapsed: session work is the primary
  // surface; advanced engine/team config is one chevron away.
  const [collapsedSections, setCollapsedSections] = useState<
    Record<string, boolean>
  >({ fallback: true, team: true });
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
  const [isolateWorker, setIsolateWorker] = useState(false);
  const [showArchived, setShowArchived] = useState(false);
  const [approvalGroups, setApprovalGroups] = useState<PermissionGroup[]>([]);
  const [sessionQuery, setSessionQuery] = useState("");
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [sessionDetail, setSessionDetail] = useState<HubSessionDetail | null>(
    null,
  );
  const [adoption, setAdoption] = useState<
    Record<string, AdoptionPreview | undefined>
  >({});
  const [editingTask, setEditingTask] = useState<string | null>(null);
  const [editingScope, setEditingScope] = useState("");
  const [memoryDraft, setMemoryDraft] = useState("");
  const [memorySaving, setMemorySaving] = useState(false);
  const [repairingEngine, setRepairingEngine] = useState<string | null>(null);
  const [repairLogs, setRepairLogs] = useState<Record<string, string>>({});
  // Logic value: identifies the newest in-flight readiness request so a stale
  // response can never overwrite a fresher one.
  const readinessRequestRef = useRef(0);
  // Logic value: the current lead session for team refreshes triggered from
  // the long-lived timeline listener (state would be a stale closure there).
  const teamLeadRef = useRef<string | null>(null);
  // Logic value: the session the long-lived timeline listener should refresh
  // (reading state there would capture a stale closure).
  const activeSessionRef = useRef<string | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void listenTimeline((event) => {
      if (active) {
        setTimeline((current) => [...current, event]);
        // A new approval request may merge with others: refresh the cards.
        if (event.status === "permission_required") {
          void getPermissionGroups().then((groups) => {
            if (active) setApprovalGroups(groups);
          });
        }
        // Terminal events change what the store says about this session, and
        // both the details drawer and the Stop gate read that.
        if (
          event.product_session_id === activeSessionRef.current &&
          ["completed", "failed", "interrupted", "exhausted"].includes(
            event.status,
          )
        ) {
          void getHubSession(event.product_session_id).then((detail) => {
            if (active && detail) setSessionDetail(detail);
          });
        }
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
    void getPermissionGroups().then(setApprovalGroups).catch(() => {});
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

  const filteredSessions = useMemo(() => {
    const query = sessionQuery.trim().toLowerCase();
    if (!query) return sessions;
    return sessions.filter((session) =>
      [session.workspace, session.last_prompt ?? "", session.engine_id]
        .join(" ")
        .toLowerCase()
        .includes(query),
    );
  }, [sessions, sessionQuery]);

  // A Stop control must be offered exactly when something can be stopped.
  const attemptIsLive = busy || sessionDetail?.session.status === "running";

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

  /// Everything that describes "the session I am looking at". Both the New
  /// session button and picking a workspace go through this, so neither can
  /// leak the previous session's profile, fallback chain or details.
  function resetSessionSurface() {
    setActiveSessionId(null);
    activeSessionRef.current = null;
    setSessionDetail(null);
    setTeamRun(null);
    teamLeadRef.current = null;
    setTimeline([]);
    setPendingEngine(null);
    setEngine("alva");
    setProfile("workspace_write");
    setTranscriptSharingState(false);
    setAnsweredPermissions({});
    // Keyed by timeline index, so a stale entry would re-expand an
    // unrelated card once the timeline refills.
    setOpenHandoffs({});
    setDetailsOpen(false);
    setDispatchWorkerId(null);
    setEditingTask(null);
  }

  async function selectWorkspace() {
    try {
      const selected = await chooseWorkspace();
      if (selected) {
        setWorkspace(selected);
        resetSessionSurface();
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
    activeSessionRef.current = detail.session.product_session_id;
    setWorkspace(detail.session.workspace);
    setEngine(detail.session.engine_id);
    setProfile(detail.session.permission_profile);
    setTranscriptSharingState(detail.session.include_full_transcript);
    setAnsweredPermissions({});
    setPendingEngine(detail.pending_switch?.target.engine_id ?? null);
    setFallbackPolicy(detail.fallback_policy);
    setTimeline(projectTimeline(detail));
    setSessionDetail(detail);
    // Last: a failed team fetch must not leave the session view half-loaded.
    teamLeadRef.current = detail.session.product_session_id;
    // A worker id from the previous run must never survive into this one.
    setDispatchWorkerId(null);
    setEditingTask(null);
    setMemoryDraft("");
    setTeamRun(await getTeamRunForLead(detail.session.product_session_id));
  }

  /// Append a memory entry and reflect the fresh list on the open session.
  /// The backend returns the updated list, so the drawer never guesses.
  async function addMemory(entry: string) {
    if (!activeSessionId || !entry.trim() || memorySaving) return;
    setMemorySaving(true);
    try {
      const memory = await addSessionMemory(activeSessionId, entry);
      setSessionDetail((detail) =>
        detail ? { ...detail, memory } : detail,
      );
      setMemoryDraft("");
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setMemorySaving(false);
    }
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
    const dispatchable = teamRun.workers.filter(
      (worker) => worker.lifecycle !== "archived",
    );
    const selected = dispatchable.find(
      (worker) => worker.worker_id === dispatchWorkerId,
    );
    const workerId = selected?.worker_id ?? dispatchable[0]?.worker_id ?? null;
    if (!workerId) {
      appendLocalFailure(
        new Error("Every worker in this run is archived. Restore one first."),
      );
      return;
    }
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
      setTeamRun(
        await addTeamWorker(teamRun.team_run_id, teamWorkerEngine, isolateWorker),
      );
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setTeamBusy(false);
    }
  }

  async function decideApprovalGroup(group: PermissionGroup, allow: boolean) {
    try {
      // Exactly the members this card displayed — never whatever happens to
      // be pending by the time the click lands.
      await respondPermissionGroup(
        group.group_key,
        allow,
        group.members.map((member) => member.request_id),
      );
      setApprovalGroups(await getPermissionGroups());
      // The individual cards for those members are answered too.
      setAnsweredPermissions((current) => ({
        ...current,
        ...Object.fromEntries(
          group.members.map((member) => [
            member.request_id,
            allow ? ("allowed" as const) : ("denied" as const),
          ]),
        ),
      }));
    } catch (error) {
      appendLocalFailure(error);
      // A partial delivery still changed some engines: resync.
      setApprovalGroups(await getPermissionGroups().catch(() => []));
    }
  }

  async function toggleAutonomy(enabled: boolean) {
    if (!teamRun) return;
    try {
      setTeamRun(await setTeamAutonomy(teamRun.team_run_id, enabled));
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function changeWorkerLifecycle(
    workerId: string,
    action: "hibernate" | "restore" | "archive",
  ) {
    try {
      if (action === "hibernate") await hibernateWorker(workerId);
      else if (action === "restore") await restoreWorker(workerId);
      else await archiveWorker(workerId);
      await refreshTeam();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function endTeamRun() {
    if (!teamRun) return;
    setTeamBusy(true);
    try {
      // Ending a run archives its workers; nothing is deleted.
      setTeamRun(await completeTeamRun(teamRun.team_run_id));
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setTeamBusy(false);
    }
  }

  async function reviewAdoption(workerId: string) {
    try {
      const preview = await previewWorkerAdoption(workerId);
      setAdoption((current) => ({ ...current, [workerId]: preview }));
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function confirmAdoption(workerId: string) {
    if (!teamRun) return;
    try {
      await adoptWorkerWork(teamRun.team_run_id, workerId);
      setAdoption((current) => ({ ...current, [workerId]: undefined }));
      await refreshTeam();
      if (activeSessionId) await loadSession(activeSessionId);
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function discardIsolatedWork(workerId: string) {
    if (!teamRun) return;
    try {
      await discardWorkerWorktree(teamRun.team_run_id, workerId);
      setAdoption((current) => ({ ...current, [workerId]: undefined }));
      await refreshTeam();
      if (activeSessionId) await loadSession(activeSessionId);
    } catch (error) {
      appendLocalFailure(error);
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

  async function refreshApprovalGroups() {
    try {
      setApprovalGroups(await getPermissionGroups());
    } catch (error) {
      appendLocalFailure(error);
    }
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
      // A group card must not keep offering a request that is now answered.
      await refreshApprovalGroups();
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
            <small>
              {sessionQuery.trim()
                ? `${filteredSessions.length}/${sessions.length}`
                : sessions.length}
            </small>
          </div>
          <input
            aria-label="Search sessions"
            className="session-search"
            placeholder="Search workspace or prompt…"
            value={sessionQuery}
            onChange={(event) => setSessionQuery(event.target.value)}
          />
          <button
            className="secondary-button"
            disabled={busy}
            onClick={() => {
              // A new session starts from a clean surface, not the last one —
              // including the fallback chain, which would otherwise be shown
              // as the default and could be saved as the global one.
              resetSessionSurface();
              void loadGlobalFallbackPolicy().catch(appendLocalFailure);
            }}
            type="button"
          >
            New session
          </button>
          <div className="session-list">
            {sessionError ? (
              <small className="session-error">{sessionError}</small>
            ) : filteredSessions.length === 0 ? (
              <small className="session-empty">
                {sessionQuery.trim()
                  ? "No sessions match that search."
                  : "Hub runs will appear here."}
              </small>
            ) : (
              filteredSessions.map((session) => (
                <button
                  className={`session-item ${
                    activeSessionId === session.product_session_id ? "active" : ""
                  }`}
                  aria-current={
                    activeSessionId === session.product_session_id
                      ? "true"
                      : undefined
                  }
                  key={session.product_session_id}
                  type="button"
                  onClick={() => void openSession(session)}
                >
                  <strong>{session.workspace.split("/").at(-1)}</strong>
                  <span>{session.last_prompt ?? "Untitled session"}</span>
                  <small>
                    {session.engine_id} · {session.status} ·{" "}
                    {formatRelativeTime(session.updated_at)}
                  </small>
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
                          <button
                            className="repair-link"
                            onClick={() =>
                              void openExternal(action.url ?? "").catch(
                                appendLocalFailure,
                              )
                            }
                            type="button"
                          >
                            Official guide
                          </button>
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
            <button
              aria-expanded={!collapsedSections.fallback}
              aria-label="Toggle fallback chain"
              className="section-collapse"
              onClick={() =>
                setCollapsedSections((current) => ({
                  ...current,
                  fallback: !current.fallback,
                }))
              }
              type="button"
            >
              {collapsedSections.fallback ? "▸" : "▾"}
            </button>
            <span className="eyebrow">
              {activeSessionId ? "Session fallback chain" : "Default fallback chain"}
            </span>
            <small>{fallbackPolicy.targets.length}</small>
          </div>
          {collapsedSections.fallback ? null : (
          <>
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
            // key must not include editable fields: changing model_id would
            // remount the inputs mid-typing and drop keystrokes.
            <div
              className="policy-target"
              key={`${target.engine_id}-${index}`}
            >
              <div>
                <strong>{index + 1}. {target.engine_id}</strong>
                <input
                  aria-label={`Model for ${target.engine_id}`}
                  className="policy-model-input"
                  placeholder="engine-default"
                  value={target.model_id}
                  onChange={(event) =>
                    updateFallbackTarget(index, {
                      model_id: event.target.value,
                    })
                  }
                />
                <input
                  aria-label={`Provider for ${target.engine_id}`}
                  className="policy-model-input"
                  placeholder="provider id"
                  value={target.provider_id}
                  onChange={(event) =>
                    updateFallbackTarget(index, {
                      provider_id: event.target.value,
                    })
                  }
                />
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
          </>
          )}
        </section>

        <section className="team-panel">
          <div className="session-heading">
            <button
              aria-expanded={!collapsedSections.team}
              aria-label="Toggle team panel"
              className="section-collapse"
              onClick={() =>
                setCollapsedSections((current) => ({
                  ...current,
                  team: !current.team,
                }))
              }
              type="button"
            >
              {collapsedSections.team ? "▸" : "▾"}
            </button>
            <span className="eyebrow">Team</span>
            <small>{teamRun ? `${teamRun.tasks.length} tasks` : ""}</small>
          </div>
          {collapsedSections.team ? null : (
          <>
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
              <small className="team-worker-line">
                Lead: {engine}
                {teamRun.status !== "active" ? ` · run ${teamRun.status}` : ""}
              </small>
              {teamRun.workers
                .filter(
                  (worker) =>
                    showArchived || worker.lifecycle !== "archived",
                )
                .map((worker) => {
                const workerTasks = teamRun.tasks.filter(
                  (task) => task.worker_id === worker.worker_id,
                );
                const workerRunning = workerTasks.some(
                  (task) => task.status === "running",
                );
                return (
                  <div className="team-worker" key={worker.worker_id}>
                    <div className="team-worker-heading">
                      <strong>
                        Worker · {worker.engine_id}
                        {worker.worktree_path ? (
                          <span className="worker-isolated">isolated</span>
                        ) : null}
                        {worker.lifecycle !== "active" ? (
                          <span className="worker-lifecycle">
                            {worker.lifecycle}
                          </span>
                        ) : null}
                      </strong>
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
                          disabled={
                            !workerRunning || worker.lifecycle !== "active"
                          }
                          onClick={() =>
                            void interruptWorkerNow(worker.worker_session_id)
                          }
                          type="button"
                        >
                          Interrupt
                        </button>
                        {worker.lifecycle === "active" ? (
                          <>
                            <button
                              onClick={() =>
                                void changeWorkerLifecycle(
                                  worker.worker_id,
                                  "hibernate",
                                )
                              }
                              type="button"
                            >
                              Hibernate
                            </button>
                            <button
                              onClick={() =>
                                void changeWorkerLifecycle(
                                  worker.worker_id,
                                  "archive",
                                )
                              }
                              type="button"
                            >
                              Archive
                            </button>
                          </>
                        ) : (
                          <button
                            onClick={() =>
                              void changeWorkerLifecycle(
                                worker.worker_id,
                                "restore",
                              )
                            }
                            type="button"
                          >
                            Restore
                          </button>
                        )}
                      </div>
                    </div>
                    {worker.worktree_branch ? (
                      <small className="team-worker-line">
                        Worktree {worker.worktree_branch} — changes stay here
                        until you adopt them.
                      </small>
                    ) : null}
                    {worker.worktree_path ? (
                      <div className="team-worker-controls">
                        <button
                          onClick={() => void reviewAdoption(worker.worker_id)}
                          type="button"
                        >
                          Review changes
                        </button>
                        <button
                          disabled={!adoption[worker.worker_id]}
                          onClick={() => void confirmAdoption(worker.worker_id)}
                          type="button"
                        >
                          Adopt
                        </button>
                        <button
                          onClick={() =>
                            void discardIsolatedWork(worker.worker_id)
                          }
                          type="button"
                        >
                          Discard worktree
                        </button>
                      </div>
                    ) : null}
                    {adoption[worker.worker_id] ? (
                      <div className="adoption-preview">
                        <strong>
                          {adoption[worker.worker_id]!.changes.length} change(s)
                          {adoption[worker.worker_id]!.applies_cleanly
                            ? " · applies cleanly"
                            : " · does NOT apply cleanly"}
                        </strong>
                        {adoption[worker.worker_id]!.changes.map((change) => (
                          <small key={change.path}>
                            {change.status} {change.path}
                          </small>
                        ))}
                        {adoption[worker.worker_id]!.conflicts.length > 0 ? (
                          <small className="adoption-conflict">
                            Conflicts with your workspace edits:{" "}
                            {adoption[worker.worker_id]!.conflicts.join(", ")}
                          </small>
                        ) : null}
                        {adoption[worker.worker_id]!.verification.map(
                          (entry, index) => (
                            <small key={`verify-${index}`}>{entry}</small>
                          ),
                        )}
                      </div>
                    ) : null}
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
                    (worker) =>
                      worker.worker_id === dispatchWorkerId &&
                      worker.lifecycle !== "archived",
                  )
                    ? (dispatchWorkerId as string)
                    : (teamRun.workers.find(
                        (worker) => worker.lifecycle !== "archived",
                      )?.worker_id ?? "")
                }
                onChange={(event) => setDispatchWorkerId(event.target.value)}
              >
                {teamRun.workers
                  .filter((worker) => worker.lifecycle !== "archived")
                  .map((worker) => (
                    <option key={worker.worker_id} value={worker.worker_id}>
                      To worker on {worker.engine_id}
                      {worker.lifecycle === "hibernated" ? " (hibernated)" : ""}
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
                <label className="transcript-toggle">
                  <input
                    checked={isolateWorker}
                    onChange={(event) => setIsolateWorker(event.target.checked)}
                    type="checkbox"
                  />
                  Isolated worktree
                </label>
                <button
                  disabled={teamBusy || teamRun.status !== "active"}
                  onClick={() => void addWorker()}
                  type="button"
                >
                  Add worker
                </button>
                <button
                  disabled={teamBusy || teamRun.status !== "active"}
                  onClick={() => void endTeamRun()}
                  type="button"
                >
                  End run
                </button>
                <label className="transcript-toggle">
                  <input
                    checked={teamRun.autonomous}
                    onChange={(event) =>
                      void toggleAutonomy(event.target.checked)
                    }
                    type="checkbox"
                  />
                  Autonomous run (no per-request approval)
                </label>
                <label className="transcript-toggle">
                  <input
                    checked={showArchived}
                    onChange={(event) => setShowArchived(event.target.checked)}
                    type="checkbox"
                  />
                  Show archived
                </label>
              </div>
            </>
          )}
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
          <button
            className="details-toggle"
            disabled={!sessionDetail}
            onClick={() => setDetailsOpen((open) => !open)}
            type="button"
          >
            {detailsOpen ? "Hide details" : "Details"}
          </button>
          <div
            aria-live="polite"
            className={`status-pill status-${latest?.status ?? "ready"}`}
          >
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
                {entry.permission_request?.resource ? (
                  <small className="timeline-hint">
                    Resource: {entry.permission_request.resource}
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
                      // Either signal counts: this window's own in-flight run,
                      // or the store saying the session is running (which is
                      // what survives a restart). Using only the snapshot
                      // would disable Stop for the whole of our own run,
                      // since it is refreshed after the run, not during.
                      disabled={!activeSessionId || !attemptIsLive}
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
                    {!attemptIsLive ? (
                      <small className="timeline-hint">
                        This attempt has finished — Stop no longer applies.
                      </small>
                    ) : null}
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
                    {entry.fallback.handoff.available_skills.length > 0 ? (
                      <div className="handoff-skills">
                        <strong>Workspace skills</strong>
                        {entry.fallback.handoff.available_skills.map((skill) => (
                          <small key={skill.path}>
                            {skill.name} — {skill.description}{" "}
                            <code>{skill.path}</code>
                          </small>
                        ))}
                      </div>
                    ) : null}
                    {entry.fallback.handoff.available_mcp.length > 0 ? (
                      <div className="handoff-skills">
                        <strong>MCP servers</strong>
                        {entry.fallback.handoff.available_mcp.map((server) => (
                          <small key={server.id}>
                            {server.display_name}{" "}
                            <code>{server.transport}</code>
                          </small>
                        ))}
                      </div>
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

        {approvalGroups
          .filter((group) => group.members.length > 1)
          .map((group) => (
            <div className="approval-group" key={group.group_key} role="status">
              <strong>
                {group.members.length} workers want to run &apos;{group.tool_name}&apos;
                {group.members[0]?.resource
                  ? ` on ${group.members[0].resource}`
                  : ""}
              </strong>
              {/* Aggregation must never hide who is asking for what. */}
              {group.members.map((member) => (
                <small key={member.request_id}>
                  {member.requester} · {member.tool_name}
                  {member.resource ? ` · ${member.resource}` : ""}
                </small>
              ))}
              <div className="team-worker-controls">
                <button
                  onClick={() => void decideApprovalGroup(group, true)}
                  type="button"
                >
                  Allow all {group.members.length}
                </button>
                <button
                  onClick={() => void decideApprovalGroup(group, false)}
                  type="button"
                >
                  Deny all
                </button>
              </div>
            </div>
          ))}

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

        {detailsOpen && sessionDetail ? (
          <aside className="details-drawer" aria-label="Session details">
            <section>
              <span className="eyebrow">Memory</span>
              <small className="timeline-hint">
                Corrections every engine in this session carries across
                fallbacks and handoffs.
              </small>
              {sessionDetail.memory.length === 0 ? (
                <small className="session-empty">
                  No memory yet. Teach the session once; every engine keeps it.
                </small>
              ) : (
                sessionDetail.memory.map((entry, index) => (
                  <div className="details-row" key={`memory-${index}`}>
                    <small>{entry}</small>
                  </div>
                ))
              )}
              <div className="memory-add">
                <input
                  aria-label="Add session memory"
                  placeholder="Always run cargo test before finishing…"
                  value={memoryDraft}
                  onChange={(event) => setMemoryDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      void addMemory(memoryDraft);
                    }
                  }}
                />
                <button
                  className="secondary-button"
                  disabled={memorySaving || !memoryDraft.trim()}
                  onClick={() => void addMemory(memoryDraft)}
                  type="button"
                >
                  {memorySaving ? "Saving…" : "Remember"}
                </button>
              </div>
            </section>
            <section>
              <span className="eyebrow">Branches</span>
              {sessionDetail.branches.length === 0 ? (
                <small className="session-empty">No branches yet.</small>
              ) : null}
              {sessionDetail.branches.map((branch) => (
                <div className="details-row" key={branch.branch_id}>
                  <strong>
                    {branch.engine_id} · {branch.status}
                  </strong>
                  <small>
                    {branch.model_id} · {branch.provider_id}
                    {branch.engine_session_id ? " · native session kept" : ""}
                  </small>
                </div>
              ))}
            </section>
            <section>
              <span className="eyebrow">Attempts</span>
              {sessionDetail.turns.every((turn) => turn.attempts.length === 0) ? (
                <small className="session-empty">No attempts yet.</small>
              ) : null}
              {sessionDetail.turns.flatMap((turn) =>
                turn.attempts.map((attempt) => (
                  <div className="details-row" key={attempt.attempt_id}>
                    <strong>
                      {attempt.engine_id} · {attempt.status}
                    </strong>
                    {attempt.failure_reason ? (
                      <small>{attempt.failure_reason}</small>
                    ) : null}
                    {attempt.usage ? (
                      <small>
                        {attempt.usage.input_tokens}in/
                        {attempt.usage.output_tokens}out ·{" "}
                        {attempt.usage.duration_ms}ms
                      </small>
                    ) : null}
                    {attempt.side_effects.length > 0 ? (
                      <small>
                        {attempt.side_effects.length} tool call(s)
                        {attempt.side_effects
                          .map((evidence) => evidence.path)
                          .filter(Boolean).length > 0
                          ? ` · ${attempt.side_effects
                              .map((evidence) => evidence.path)
                              .filter(Boolean)
                              .join(", ")}`
                          : ""}
                      </small>
                    ) : null}
                  </div>
                )),
              )}
            </section>
            <section>
              <span className="eyebrow">Handoffs</span>
              {sessionDetail.handoffs.length === 0 ? (
                <small className="session-empty">No handoffs yet.</small>
              ) : (
                sessionDetail.handoffs.map((handoff) => (
                  <div className="details-row" key={handoff.handoff_id}>
                    <strong>
                      {handoff.kind} · {handoff.source.engine_id} →{" "}
                      {handoff.target.engine_id}
                      {handoff.used_at ? " · delivered" : " · pending"}
                    </strong>
                    <small>{handoff.reason}</small>
                    {handoff.team_snapshot ? (
                      <small>{handoff.team_snapshot}</small>
                    ) : null}
                    {handoff.session_memory.length > 0 ||
                    handoff.available_skills.length > 0 ||
                    handoff.available_mcp.length > 0 ? (
                      <small>
                        envelope: {handoff.session_memory.length} memory ·{" "}
                        {handoff.available_skills.length} skills ·{" "}
                        {handoff.available_mcp.length} MCP
                      </small>
                    ) : null}
                    <button
                      className="secondary-button"
                      onClick={() =>
                        setOpenDrawerHandoffs((current) => ({
                          ...current,
                          [handoff.handoff_id]: !current[handoff.handoff_id],
                        }))
                      }
                      type="button"
                    >
                      {openDrawerHandoffs[handoff.handoff_id]
                        ? "Hide envelope"
                        : "Show envelope"}
                    </button>
                    {openDrawerHandoffs[handoff.handoff_id] ? (
                      <div className="handoff-view">
                        {handoff.session_memory.length > 0 ? (
                          <>
                            <strong>Memory carried</strong>
                            {handoff.session_memory.map((entry, index) => (
                              <small key={`dm-${index}`}>{entry}</small>
                            ))}
                          </>
                        ) : null}
                        {handoff.available_skills.length > 0 ? (
                          <>
                            <strong>Skills pointed</strong>
                            {handoff.available_skills.map((skill) => (
                              <small key={skill.path}>
                                {skill.name} — {skill.description}{" "}
                                <code>{skill.path}</code>
                              </small>
                            ))}
                          </>
                        ) : null}
                        {handoff.available_mcp.length > 0 ? (
                          <>
                            <strong>MCP servers</strong>
                            {handoff.available_mcp.map((server) => (
                              <small key={server.id}>
                                {server.display_name}{" "}
                                <code>{server.transport}</code>
                              </small>
                            ))}
                          </>
                        ) : null}
                        {handoff.transcript.length > 0 ? (
                          <>
                            <strong>
                              Filtered transcript ({handoff.transcript.length}{" "}
                              turns)
                            </strong>
                            {handoff.transcript.map((turn, index) => (
                              <small key={`dt-${index}`}>
                                » {turn.prompt}
                                {turn.result ? ` — ${turn.result}` : ""}
                              </small>
                            ))}
                          </>
                        ) : (
                          <small>
                            Transcript not shared (default).
                          </small>
                        )}
                      </div>
                    ) : null}
                  </div>
                ))
              )}
            </section>
            <section>
              <span className="eyebrow">Engine diagnostics</span>
              {readiness.map((engineReadiness) => (
                <div className="details-row" key={engineReadiness.engine_id}>
                  <strong>
                    {engineReadiness.display_name} ·{" "}
                    {readinessBadge[engineReadiness.status]}
                  </strong>
                  <small>
                    {engineReadiness.version ?? "version unknown"} · stream{" "}
                    {engineReadiness.stream_health}
                  </small>
                  {engineReadiness.reasons.map((reason, index) => (
                    <small key={`${reason.code}-${index}`}>
                      {describeReadinessReason(reason)}
                    </small>
                  ))}
                </div>
              ))}
            </section>
          </aside>
        ) : null}

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
          {(() => {
            const selectedId = pendingEngine ?? engine;
            // Prefer the edited policy target (user-set model/provider),
            // fall back to the catalog default for an unconfigured engine.
            const target =
              fallbackPolicy.targets.find(
                (candidate) => candidate.engine_id === selectedId,
              ) ??
              engineCatalog.find(
                (candidate) => candidate.engine_id === selectedId,
              );
            if (!target) return null;
            return (
              <small className="composer-model">
                Running {selectedId} on {target.model_id} · {target.provider_id}
              </small>
            );
          })()}
          <div className="composer-footer">
            <span
              className="composer-summary"
              title={`${engine}${
                sessionDetail ? ` · ${sessionDetail.session.model_id}` : ""
              } · ${profileLabel[profile]} · fallback: ${
                fallbackPolicy.targets
                  .map((target) => target.engine_id)
                  .join(" → ") || "none"
              }`}
            >
              {workspace ? "" : "Select a workspace first · "}
              {engine}
              {sessionDetail ? ` · ${sessionDetail.session.model_id}` : ""}
              {` · ${profileLabel[profile]}`}
              {` · fallback: ${
                fallbackPolicy.targets.length === 0
                  ? "none"
                  : fallbackPolicy.targets
                      .map(
                        (target) =>
                          `${target.engine_id}${
                            target.automatic_use_authorized ? "" : " (unauthorized)"
                          }`,
                      )
                      .join(" → ")
              }`}
            </span>
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
