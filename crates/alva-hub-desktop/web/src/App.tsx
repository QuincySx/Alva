// INPUT:  React hooks, bridge workspace/run/timeline/session functions
// OUTPUT: engine selection, fallback authorization, durable branches, composer, timeline UI
// POS:    Primary Hub Desktop product surface for reversible, user-authorized engine sessions.

import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { PanelRight } from "lucide-react";
import { HubComposer } from "./components/hub-composer";
import { EngineReadinessPanel } from "./components/engine-readiness-panel";
import { FallbackPolicyPanel } from "./components/fallback-policy-panel";
import { ModelPicker, type HubModelRoute } from "./components/model-picker";
import { HubSidebar } from "./components/hub-sidebar";
import {
  HubTimeline,
  type HubTimelineEvent,
} from "./components/hub-timeline";
import { HubTopbar } from "./components/hub-topbar";
import { SessionDetails } from "./components/session-details";
import { TeamPanel } from "./components/team-panel";
import {
  AutomationsPage,
  ExtensionsPage,
  SearchPage,
  SettingsPage,
  type HubPage,
} from "./components/product-pages";
import {
  AgentTool,
  BrowserTool,
  FilesTool,
  InspectorTool,
  ReviewTool,
  TasksTool,
  TerminalTool,
  UsageTool,
} from "./components/workbench-tools";
import {
  AppFrame,
  Button,
  Checkbox,
  ContentFrame,
  DockRegion,
  IconButton,
  Notice,
  ScrollRegion,
  SelectField,
  TabsContent,
  TabsList,
  TabsRoot,
  TabsTrigger,
  UIProvider,
  Workbench,
  WorkbenchPage,
} from "./ui";
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
} from "./bridge";

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

type WorkbenchTool =
  | "files"
  | "review"
  | "agent"
  | "inspector"
  | "tasks"
  | "usage"
  | "browser"
  | "terminal"
  | "engines"
  | "fallback"
  | "team";

const hubPages: HubPage[] = ["conversation", "automations", "extensions", "settings", "search"];
const workbenchTools: WorkbenchTool[] = ["files", "review", "agent", "inspector", "tasks", "usage", "browser", "terminal", "engines", "fallback", "team"];

function queryValue(name: string): string | null {
  return typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get(name);
}

function initialPage(): HubPage {
  const page = queryValue("page") as HubPage | null;
  return page && hubPages.includes(page) ? page : "conversation";
}

function initialWorkbenchTool(): WorkbenchTool {
  const tool = queryValue("tool") as WorkbenchTool | null;
  return tool && workbenchTools.includes(tool) ? tool : "agent";
}

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

function displayWorkspace(workspace: string): string {
  const segments = workspace.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? workspace;
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

export function projectTimeline(detail: HubSessionDetail): HubTimelineEvent[] {
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
  const switches = detail.timeline_events.map((event): HubTimelineEvent => ({
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
  const [timeline, setTimeline] = useState<HubTimelineEvent[]>([]);
  // Drawer handoff expansion (by handoff_id) for the full cross-engine
  // envelope: memory, skills, MCP pointers and filtered transcript.
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
  const [approvalGroups, setApprovalGroups] = useState<PermissionGroup[]>([]);
  const [sessionQuery, setSessionQuery] = useState("");
  const [workbenchOpen, setWorkbenchOpen] = useState(() => queryValue("workbench") === "1");
  const [activeWorkbenchTool, setActiveWorkbenchTool] = useState<WorkbenchTool>(initialWorkbenchTool);
  const [activePage, setActivePage] = useState<HubPage>(initialPage);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [sessionDetail, setSessionDetail] = useState<HubSessionDetail | null>(
    null,
  );
  const [adoption, setAdoption] = useState<
    Record<string, AdoptionPreview | undefined>
  >({});
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
    const requestedSession = queryValue("session");
    if (requestedSession) void loadSession(requestedSession).catch(appendLocalFailure);
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

  const modelRoutes = useMemo<HubModelRoute[]>(
    () =>
      engineCatalog.map((catalogTarget) => {
        const target =
          fallbackPolicy.targets.find(
            (candidate) => candidate.engine_id === catalogTarget.engine_id,
          ) ?? catalogTarget;
        const health = readiness.find(
          (candidate) => candidate.engine_id === target.engine_id,
        );
        const engineEligibility = eligibility.find(
          (candidate) => candidate.engine_id === target.engine_id,
        );
        return {
          engineId: target.engine_id,
          engineLabel:
            health?.display_name ??
            `${target.engine_id.charAt(0).toUpperCase()}${target.engine_id.slice(1)}`,
          modelId: target.model_id,
          modelLabel:
            target.model_id === "engine-default"
              ? "Default model"
              : target.model_id === "workspace-config"
                ? "Workspace model"
                : target.model_id,
          providerId: target.provider_id,
          accountSource: target.account_source,
          billingRoute: target.billing_route,
          description:
            target.model_id === "engine-default"
              ? "Uses the model selected by this engine account."
              : "Configured for this Hub route.",
          capabilities: health?.capabilities ?? [],
          status:
            health && health.status !== "healthy"
              ? readinessBadge[health.status]
              : undefined,
          disabled:
            engineEligibility?.eligible === false && target.engine_id !== engine,
          disabledReason:
            engineEligibility?.eligible === false
              ? engineEligibility.detail
              : undefined,
        };
      }),
    [eligibility, engine, fallbackPolicy.targets, readiness],
  );

  const selectedModelRoute =
    modelRoutes.find((route) => route.engineId === (pendingEngine ?? engine)) ??
    modelRoutes[0];

  /// Everything that describes "the session I am looking at". Both the New
  /// session button and picking a workspace go through this, so neither can
  /// leak the previous session's profile, fallback chain or details.
  function resetSessionSurface() {
    setActivePage("conversation");
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
    setDetailsOpen(false);
    setDispatchWorkerId(null);
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
      setActivePage("conversation");
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

  async function saveTaskEdit(taskId: string, scope: string) {
    try {
      await updateWorkerTask(taskId, scope);
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
      setActivePage("conversation");
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
    <UIProvider>
    <AppFrame
      onWorkbenchDismiss={() => setWorkbenchOpen(false)}
      sidebar={
        <HubSidebar
          activePage={activePage}
          activeSessionId={activeSessionId}
          busy={busy}
          formatUpdatedAt={formatRelativeTime}
          onChooseWorkspace={selectWorkspace}
          onNewSession={() => {
            resetSessionSurface();
            void loadGlobalFallbackPolicy().catch(appendLocalFailure);
          }}
          onNavigate={setActivePage}
          onOpenSession={(session) => void openSession(session)}
          onSessionQueryChange={setSessionQuery}
          sessionError={sessionError}
          sessionQuery={sessionQuery}
          sessions={filteredSessions}
          totalSessionCount={sessions.length}
          workspace={workspace}
        />
      }
      workbench={
        <Workbench
          actions={
            <IconButton
              icon={<PanelRight aria-hidden="true" size={16} />}
              label="Close workbench"
              onClick={() => setWorkbenchOpen(false)}
              variant="ghost"
            />
          }
          className="hub-workbench"
          title="Workspace"
        >
          <TabsRoot className="hub-workbench__tabs" onValueChange={(value) => setActiveWorkbenchTool(value as WorkbenchTool)} value={activeWorkbenchTool}>
            <TabsList aria-label="Workbench panels" className="hub-workbench__tab-list">
              <TabsTrigger value="files">Files</TabsTrigger>
              <TabsTrigger value="review">Review</TabsTrigger>
              <TabsTrigger value="agent">Agent</TabsTrigger>
              <TabsTrigger value="inspector">Inspector</TabsTrigger>
              <TabsTrigger value="tasks">Tasks</TabsTrigger>
              <TabsTrigger value="usage">Usage</TabsTrigger>
              <TabsTrigger value="browser">Browser</TabsTrigger>
              <TabsTrigger value="terminal">Terminal</TabsTrigger>
              <TabsTrigger value="engines">Engines</TabsTrigger>
              <TabsTrigger value="fallback">Fallback</TabsTrigger>
              <TabsTrigger value="team">Team</TabsTrigger>
            </TabsList>
            <TabsContent className="hub-workbench__content" value="files"><WorkbenchPage><FilesTool detail={sessionDetail} /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="review"><WorkbenchPage><ReviewTool detail={sessionDetail} /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="agent"><WorkbenchPage><AgentTool onFocus={(sessionId) => void openSessionById(sessionId)} run={teamRun} /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="inspector"><WorkbenchPage><InspectorTool detail={sessionDetail} events={timeline} /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="tasks"><WorkbenchPage><TasksTool run={teamRun} /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="usage"><WorkbenchPage><UsageTool detail={sessionDetail} /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="browser"><WorkbenchPage><BrowserTool /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="terminal"><WorkbenchPage><TerminalTool /></WorkbenchPage></TabsContent>
            <TabsContent className="hub-workbench__content" value="engines">
              <WorkbenchPage>
                <EngineReadinessPanel
                busy={readinessBusy}
                describeReason={describeReadinessReason}
                label={readinessBadge}
                logs={repairLogs}
                onOpenGuide={(url) =>
                  void Promise.resolve(openExternal(url)).catch(appendLocalFailure)
                }
                onRefresh={() => void refreshReadiness(workspace || null)}
                onRepair={(engineId, kind) => void repairEngine(engineId, kind)}
                readiness={readiness}
                repairingEngine={repairingEngine}
                repairPlan={repairPlan}
                />
              </WorkbenchPage>
            </TabsContent>
            <TabsContent className="hub-workbench__content" value="fallback">
              <WorkbenchPage>
                <FallbackPolicyPanel
                activeSession={activeSessionId !== null}
                catalog={engineCatalog}
                onMoveTarget={moveFallbackTarget}
                onSave={() => void saveFallbackPolicy()}
                onToggleTarget={toggleFallbackTarget}
                onUpdateTarget={updateFallbackTarget}
                policy={fallbackPolicy}
                saving={policySaving}
                />
              </WorkbenchPage>
            </TabsContent>
            <TabsContent className="hub-workbench__content" value="team">
              <WorkbenchPage>
                <TeamPanel
                activeSession={activeSessionId !== null}
                adoption={adoption}
                busy={teamBusy}
                catalog={engineCatalog}
                claims={teamClaims}
                describeClaim={describeClaim}
                dispatchWorkerId={dispatchWorkerId}
                engineId={engine}
                isolateWorker={isolateWorker}
                onAddWorker={() => void addWorker()}
                onAdopt={(workerId) => void confirmAdoption(workerId)}
                onCancelTask={(taskId) => void cancelTask(taskId)}
                onClaimsChange={setTeamClaims}
                onCreate={() => void createTeam()}
                onDiscard={(workerId) => void discardIsolatedWork(workerId)}
                onDispatch={() => void dispatchTask()}
                onDispatchWorkerChange={setDispatchWorkerId}
                onEnd={() => void endTeamRun()}
                onFocusWorker={(sessionId) => void openSessionById(sessionId)}
                onInterruptWorker={(sessionId) => void interruptWorkerNow(sessionId)}
                onIsolateWorkerChange={setIsolateWorker}
                onLifecycleChange={(workerId, action) =>
                  void changeWorkerLifecycle(workerId, action)
                }
                onReviewAdoption={(workerId) => void reviewAdoption(workerId)}
                onScopeChange={setTeamScope}
                onToggleAutonomy={(enabled) => void toggleAutonomy(enabled)}
                onUpdateTask={(taskId, scope) => void saveTaskEdit(taskId, scope)}
                onWorkerEngineChange={setTeamWorkerEngine}
                run={teamRun}
                scope={teamScope}
                workerEngine={teamWorkerEngine}
                />
              </WorkbenchPage>
            </TabsContent>
          </TabsRoot>
        </Workbench>
      }
      workbenchOpen={workbenchOpen}
    >

      <div className="main-column">
        {activePage === "conversation" ? (
          <>
        <HubTopbar
          detailsAvailable={sessionDetail !== null}
          detailsOpen={detailsOpen}
          onToggleDetails={() => setDetailsOpen((open) => !open)}
          onToggleWorkbench={() => setWorkbenchOpen((open) => !open)}
          status={latest?.status ?? "ready"}
          statusLabel={statusLabel}
          title={sessionDetail?.turns.at(-1)?.prompt ?? "New conversation"}
          workbenchOpen={workbenchOpen}
          workspace={workspace}
        />

        <ContentFrame className="hub-session-frame">
          <ScrollRegion className="hub-session-scroll">
            <HubTimeline
              activeSessionId={activeSessionId}
              answeredPermissions={answeredPermissions}
              attemptIsLive={attemptIsLive}
              engineId={engine}
              events={timeline}
              onAnswerPermission={(productSessionId, requestId, allow) =>
                void answerPermission(productSessionId, requestId, allow)
              }
              onStop={() => void stopActiveAttempt()}
              onSwitchBack={(sourceEngineId) => void switchBack(sourceEngineId)}
            />

            {detailsOpen && sessionDetail ? (
              <SessionDetails
                describeReason={describeReadinessReason}
                detail={sessionDetail}
                memorySaving={memorySaving}
                onAddMemory={(entry) => void addMemory(entry)}
                readiness={readiness}
                readinessLabel={readinessBadge}
              />
            ) : null}
          </ScrollRegion>

          <DockRegion className="hub-session-dock">

          {approvalGroups
            .filter((group) => group.members.length > 1)
            .map((group) => (
            <Notice
              actions={
                <>
                <Button
                  aria-label={`Allow ${group.members.length} ${group.tool_name} requests`}
                  onClick={() => void decideApprovalGroup(group, true)}
                  size="medium"
                  variant="primary"
                >
                  Allow
                </Button>
                <Button
                  aria-label={`Deny ${group.members.length} ${group.tool_name} requests`}
                  onClick={() => void decideApprovalGroup(group, false)}
                  size="medium"
                  variant="ghost"
                >
                  Deny
                </Button>
                </>
              }
              className="approval-group"
              description={
                group.members[0]?.resource
                  ? `${group.members.length} agents need access to ${group.members[0].resource}.`
                  : `${group.members.length} agents need permission to continue.`
              }
              heading={`Allow ${group.tool_name}?`}
              key={group.group_key}
              role="status"
              tone="info"
            >
              <small>{group.members.map((member) => member.requester).join(" · ")}</small>
            </Notice>
            ))}

          {(() => {
            const selectedId = pendingEngine ?? engine;
            const health = readiness.find(
              (candidate) => candidate.engine_id === selectedId,
            );
            if (!health || health.status === "healthy") return null;
            return (
              <Notice
                className="composer-readiness"
                description={
                  health.reasons[0]
                    ? describeReadinessReason(health.reasons[0])
                    : "Choose another available engine to continue."
                }
                heading={`${health.display_name} is not ready`}
                role="status"
                tone="warning"
              />
            );
          })()}

          <HubComposer
          busy={busy}
          controls={
            <>
              <SelectField
                ariaLabel="Permission profile"
                className="hub-composer__permission"
                disabled={activeSessionId !== null}
                onValueChange={(value) => setProfile(value as PermissionProfile)}
                options={(Object.keys(profileLabel) as PermissionProfile[]).map((candidate) => ({
                  label: profileLabel[candidate],
                  value: candidate,
                }))}
                value={profile}
              />
              {activeSessionId ? (
                <Checkbox
                  checked={transcriptSharing}
                  className="transcript-toggle"
                  label="Share transcript"
                  onCheckedChange={(checked) => {
                    const enabled = checked === true;
                    setTranscriptSharingState(enabled);
                    void setTranscriptSharing_backend(enabled);
                  }}
                />
              ) : null}
            </>
          }
          disabled={!workspace}
          engineId={engine}
          modelPicker={
            selectedModelRoute ? (
              <ModelPicker
                onSelect={(route) => void selectEngine(route.engineId)}
                routes={modelRoutes}
                selectedEngineId={pendingEngine ?? engine}
                selectedModelId={selectedModelRoute.modelId}
              />
            ) : null
          }
          onChange={setPrompt}
          onStop={() => void stopActiveAttempt()}
          onSubmit={submit}
          placeholder={`Ask ${pendingEngine ?? engine} to inspect, change, or explain this project…`}
          status={
            <>
              <span>{workspace ? displayWorkspace(workspace) : "Select a workspace first"}</span>
              {selectedModelRoute ? (
                <span>
                  {selectedModelRoute.engineLabel} · {selectedModelRoute.modelLabel}
                </span>
              ) : null}
              <span>
                {profileLabel[profile]}
                {fallbackPolicy.targets.length > 0
                  ? ` · ${fallbackPolicy.targets.length} fallback ${fallbackPolicy.targets.length === 1 ? "engine" : "engines"}`
                  : ""}
              </span>
            </>
          }
            value={prompt}
          />
          </DockRegion>
        </ContentFrame>
          </>
        ) : activePage === "automations" ? (
          <AutomationsPage />
        ) : activePage === "extensions" ? (
          <ExtensionsPage onStartChat={() => setActivePage("conversation")} />
        ) : activePage === "settings" ? (
          <SettingsPage />
        ) : (
          <SearchPage
            onNavigate={setActivePage}
            onOpenSession={(productSessionId) => {
              setActivePage("conversation");
              void openSessionById(productSessionId);
            }}
            onOpenWorkbench={(tool) => {
              setActiveWorkbenchTool(tool);
              setWorkbenchOpen(true);
            }}
            sessions={sessions.map((session) => ({
              description: `${displayWorkspace(session.workspace)} · ${session.engine_id} · ${session.status}`,
              id: session.product_session_id,
              title: session.last_prompt ?? "Untitled session",
            }))}
          />
        )}
      </div>
    </AppFrame>
    </UIProvider>
  );
}
