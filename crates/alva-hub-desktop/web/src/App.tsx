// INPUT:  React hooks, bridge workspace/run/timeline/session functions
// OUTPUT: session/project onboarding, engine selection, durable branches, composer, timestamp-preserving timeline projection
// POS:    Primary Hub Desktop product surface for reversible, user-authorized engine sessions.

import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  ExternalLink,
  FileCheck2,
  FileCode2,
  FolderTree,
  Gauge,
  GitBranch,
  Globe2,
  ListChecks,
  PanelRight,
  Plus,
  SearchCode,
  ServerCog,
  TerminalSquare,
  Users,
  Wrench,
  X,
  type LucideIcon,
} from "lucide-react";
import { HubComposer } from "./components/hub-composer";
import { EngineReadinessPanel } from "./components/engine-readiness-panel";
import { FallbackPolicyPanel } from "./components/fallback-policy-panel";
import { ModelPicker, type HubModelRoute } from "./components/model-picker";
import { HubSidebar } from "./components/hub-sidebar";
import { ChatTranscript } from "./components/chat-transcript";
import {
  HubTimeline,
  type HubTimelineEvent,
} from "./components/hub-timeline";
import { HubTopbar } from "./components/hub-topbar";
import { SessionDetails } from "./components/session-details";
import { TeamPanel } from "./components/team-panel";
import { WorkspaceGate } from "./components/workspace-gate";
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
  GitTool,
  InspectorTool,
  ReviewTool,
  TasksTool,
  TerminalTool,
  TextBrowserTool,
  UsageTool,
} from "./components/workbench-tools";
import {
  AppFrame,
  Button,
  Checkbox,
  ContentFrame,
  DockRegion,
  IconButton,
  MenuContent,
  MenuItem,
  MenuLabel,
  MenuRoot,
  MenuTrigger,
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
  createSessionWorkspace,
  discoverModelsForProvider,
  getEngineReadiness,
  getDefaultModelRoute,
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
  listModelProviders,
  listModelRoutes,
  listenTimeline,
  respondHubPermission,
  runEngineRepair,
  runHubPrompt,
  setGlobalFallbackPolicy,
  setDefaultModelRoute,
  setSessionFallbackPolicy,
  setTranscriptSharing,
  saveModelProvider,
  stopHubAttempt,
  switchHubEngine,
  testModelRoute,
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
  type ModelRoute,
  type ProviderProfile,
  type SaveProviderProfile,
  type WorkspaceKind,
} from "./bridge";
import { localizeProductText, localizeSystemText, useI18n, type Locale, type Translate } from "./i18n";

export function describeReadinessReason(reason: ReadinessReason, locale: Locale = "en-US"): string {
  const zh = locale === "zh-CN";
  switch (reason.code) {
    case "binary_not_found":
      return zh ? `未安装 — 已搜索 ${reason.searched.join(", ") || "PATH"}。` : `Not installed — searched ${reason.searched.join(", ") || "PATH"}.`;
    case "version_unreadable":
      return zh ? `版本检查失败：${reason.detail}` : `Version check failed: ${reason.detail}`;
    case "version_incompatible":
      return zh ? `当前版本 ${reason.found} 低于要求的 ${reason.minimum}。` : `Version ${reason.found} is older than the required ${reason.minimum}.`;
    case "unauthenticated":
      return localizeProductText(locale, reason.remedy);
    case "provider_key_missing":
      return localizeProductText(locale, reason.remedy);
    case "stream_failing":
      return zh ? `流式响应失败：${reason.last_failure}` : `Streaming is failing: ${reason.last_failure}`;
  }
}

const readinessBadge: Record<EngineReadiness["status"], string> = {
  healthy: "Healthy",
  missing: "Not installed",
  incompatible: "Incompatible",
  unauthenticated: "Sign-in needed",
  unhealthy: "Unhealthy",
};

type WorkbenchTool =
  | "files"
  | "review"
  | "agent"
  | "inspector"
  | "tasks"
  | "usage"
  | "browser"
  | "text"
  | "git"
  | "terminal"
  | "engines"
  | "fallback"
  | "team";

const hubPages: HubPage[] = ["conversation", "automations", "extensions", "settings", "search"];
const workbenchTools: WorkbenchTool[] = ["files", "review", "agent", "inspector", "tasks", "usage", "browser", "text", "git", "terminal", "engines", "fallback", "team"];

const workbenchToolMeta: Record<WorkbenchTool, { icon: LucideIcon }> = {
  files: { icon: FolderTree },
  review: { icon: FileCheck2 },
  agent: { icon: Bot },
  inspector: { icon: SearchCode },
  tasks: { icon: ListChecks },
  usage: { icon: Gauge },
  browser: { icon: Globe2 },
  text: { icon: FileCode2 },
  git: { icon: GitBranch },
  terminal: { icon: TerminalSquare },
  engines: { icon: ServerCog },
  fallback: { icon: Wrench },
  team: { icon: Users },
};

function queryValue(name: string): string | null {
  return typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get(name);
}

/// A standalone window opened at `#/sidebar-window` renders only the Workbench
/// (no sidebar, no main column) so it can sit on a second display.
function isSidebarWindow(): boolean {
  return typeof window !== "undefined" && window.location.hash === "#/sidebar-window";
}

function createRunToken(): string {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  const entropy = new Uint32Array(4);
  globalThis.crypto?.getRandomValues(entropy);
  return `hub-run-${Date.now()}-${Array.from(entropy).join("-")}-${Math.random().toString(36).slice(2)}`;
}

function initialPage(): HubPage {
  const page = queryValue("page") as HubPage | null;
  return page && hubPages.includes(page) ? page : "conversation";
}

function initialWorkbenchTool(): WorkbenchTool {
  const tool = queryValue("tool") as WorkbenchTool | null;
  return tool && workbenchTools.includes(tool) ? tool : "agent";
}

export function describeClaim(claim: ResourceClaim, locale: Locale = "en-US"): string {
  switch (claim.kind) {
    case "path":
      return locale === "zh-CN" ? `${claim.mode === "read" ? "读取" : "写入"} ${claim.path}` : `${claim.mode} ${claim.path}`;
    case "facility":
      return locale === "zh-CN" ? `资源 ${claim.name}` : `facility ${claim.name}`;
    case "exclusive_tool":
      return locale === "zh-CN" ? `独占工具 ${claim.tool}` : `tool ${claim.tool}`;
  }
}

/// `src/lib.rs, read:docs, facility:target-dir, tool:git` → claims.
/// "now-relative" label for a session's updated_at (e.g. "2m ago",
/// "3h ago", "2024-08-01"). Pure and testable.
export function formatRelativeTime(updatedAt: number, now: number = Date.now(), locale: Locale = "en-US"): string {
  if (!Number.isFinite(updatedAt) || !Number.isFinite(now)) return "";
  const delta = Math.max(0, now - updatedAt);
  const min = Math.floor(delta / 60_000);
  if (min < 1) return locale === "zh-CN" ? "刚刚" : "just now";
  if (min < 60) return locale === "zh-CN" ? `${min} 分钟前` : `${min}m ago`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return locale === "zh-CN" ? `${hours} 小时前` : `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return locale === "zh-CN" ? `${days} 天前` : `${days}d ago`;
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

export function projectTimeline(detail: HubSessionDetail, locale: Locale = "en-US"): HubTimelineEvent[] {
  const turns = detail.turns.flatMap((turn): HubTimelineEvent[] => {
    const engineId = turn.attempts.at(-1)?.engine_id ?? detail.session.engine_id;
    const user: HubTimelineEvent = {
      status: "user_message",
      role: "user",
      engine_id: engineId,
      product_session_id: detail.session.product_session_id,
      message: turn.prompt,
      created_at: turn.created_at,
      item_id: `${turn.turn_id}:user`,
    };
    const assistant: HubTimelineEvent = {
      status:
        turn.status === "failed"
          ? "failed"
          : turn.status === "running"
            ? "running"
            : turn.status === "interrupted"
              ? "interrupted"
              : "assistant_message",
      role: "assistant",
      engine_id: engineId,
      product_session_id: detail.session.product_session_id,
      message: turn.result ?? localizeSystemText(locale, turn.error, "操作失败。请在检查器中查看技术详情。"),
      created_at: turn.completed_at ?? turn.created_at,
      item_id: `${turn.turn_id}:assistant`,
    };
    const activities = turn.attempts.flatMap((attempt) =>
      attempt.side_effects.map((effect): HubTimelineEvent => ({
        status: effect.succeeded === null ? "activity_started" : "activity_completed",
        engine_id: attempt.engine_id,
        product_session_id: detail.session.product_session_id,
        message: effect.succeeded === false
          ? locale === "zh-CN" ? `${effect.tool_name} 失败` : `${effect.tool_name} failed`
          : effect.succeeded === true
            ? locale === "zh-CN" ? `${effect.tool_name} 已完成` : `${effect.tool_name} completed`
            : locale === "zh-CN" ? `${effect.tool_name} 运行中` : `${effect.tool_name} running`,
        created_at: effect.duration_ms === null
          ? attempt.started_at
          : (attempt.completed_at ?? attempt.started_at),
        item_id: effect.tool_call_id,
        activity: {
          tool_name: effect.tool_name,
          succeeded: effect.succeeded,
          duration_ms: effect.duration_ms,
          path: effect.path,
        },
      })),
    );
    return assistant.message || assistant.status !== "assistant_message"
      ? [user, ...activities, assistant]
      : [user, ...activities];
  });
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
                ? ("activity_completed" as const)
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
        ? `${locale === "zh-CN" ? "已跳过" : "Skipped"} ${event.target_engine_id}\n\n${localizeSystemText(locale, event.reason, "引擎不可用。")}`
        : event.kind === "fallback_exhausted" ||
            event.kind === "attempt_cancelled" ||
            event.kind === "worker_outcome" ||
            event.kind === "permission_decision" ||
            event.kind === "resource_wait" ||
            event.kind === "worktree_adopted" ||
            event.kind === "worktree_discarded"
          ? localizeSystemText(locale, event.reason, "操作状态已更新。")
          : `${event.source_engine_id} → ${event.target_engine_id}\n\n${localizeSystemText(locale, event.reason, "引擎已切换。")}`,
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

/// What the header shows next to the session title.
///
/// `timeline.at(-1)` is "the last thing that happened", **not** "what the
/// system is doing right now". A finished turn replayed from the store ends on
/// `assistant_message` — `projectTimeline` has no completed state, it only
/// special-cases failed / running / interrupted — so deriving an in-progress
/// phrasing from the last event alone leaves the header stuck on
/// "<engine> is replying" for a turn that ended minutes ago. Every in-progress
/// wording is therefore gated on the attempt actually being live.
export function statusLabelFor(
  latest: HubTimelineEvent | undefined,
  attemptIsLive: boolean,
  t: Translate,
): string {
  if (!latest) return t("status.ready");
  if (!attemptIsLive) {
    switch (latest.status) {
      case "failed":
        return t("status.failed");
      case "interrupted":
        return t("status.interrupted");
      case "exhausted":
        return t("status.fallbackExhausted");
      case "queued":
        return t("status.switchQueued");
      case "cancelled":
        return t("status.switchCancelled");
      case "switched":
        return t("status.switched");
      case "permission_required":
        return t("status.awaitingApproval", { engine: latest.engine_id });
      default:
        return t("status.completed");
    }
  }
  switch (latest.status) {
    case "user_message":
      return t("status.messageSent");
    case "assistant_message":
      return t("status.responding", { engine: latest.engine_id });
    case "activity_started":
      return t("status.usingTool", {
        engine: latest.engine_id,
        tool: latest.activity?.tool_name ?? t("settings.tools"),
      });
    case "activity_completed":
      return t("status.activityCompleted");
    case "running":
      return t("status.working", { engine: latest.engine_id });
    case "completed":
      return t("status.completed");
    case "failed":
      return t("status.failed");
    case "interrupted":
      return t("status.interrupted");
    case "queued":
      return t("status.switchQueued");
    case "switched":
      return t("status.switched");
    case "cancelled":
      return t("status.switchCancelled");
    case "fallback":
      return t("status.fallback");
    case "exhausted":
      return t("status.fallbackExhausted");
    case "permission_required":
      return t("status.awaitingApproval", { engine: latest.engine_id });
    case "worker":
      return t("status.workerOutcome");
    default:
      return t("status.ready");
  }
}

export function mergeLiveTimeline(
  current: HubTimelineEvent[],
  event: HubTimelineEvent,
): HubTimelineEvent[] {
  // R29: live events arrive without `role`; derive it from the status so the
  // transcript keeps owning the item after a terminal merge flips the status
  // away from assistant_message/user_message.
  const derivedRole: HubTimelineEvent["role"] = event.status === "user_message"
    ? "user"
    : event.status === "assistant_message"
      ? "assistant"
      : undefined;
  const observed: HubTimelineEvent = {
    ...event,
    role: event.role ?? derivedRole,
    started_at: event.started_at ?? event.created_at,
  };
  if (!event.item_id) return [...current, observed];
  const index = current.findIndex(
    (candidate) =>
      candidate.product_session_id === event.product_session_id &&
      candidate.item_id === event.item_id,
  );
  if (index < 0) return [...current, observed];
  const next = [...current];
  const previous = current[index];
  // R29: a terminal event merging into the assistant's message item carries a
  // status caption, NOT prose — it must never replace text the user was
  // already reading. Status drives the visuals; the body keeps whatever was
  // there (streamed or restored history).
  next[index] = event.status === "assistant_message"
    ? {
        ...previous,
        ...event,
        role: previous.role ?? derivedRole,
        started_at: previous.started_at ?? previous.created_at ?? event.created_at,
        message: `${previous.message ?? ""}${event.message ?? ""}`,
      }
    : {
        ...previous,
        ...event,
        role: previous.role ?? derivedRole,
        started_at: previous.started_at ?? previous.created_at ?? event.created_at,
        // Empty-string bodies (a running turn with no output yet) are absent
        // too — the terminal caption fills them in; real prose always wins.
        message: previous.message || event.message || null,
      };
  return next;
}

export default function App() {
  const { locale, localize, t } = useI18n();
  const localizedProfileLabel: Record<PermissionProfile, string> = {
    interactive: t("permission.interactive"),
    workspace_write: t("permission.workspaceWrite"),
    unrestricted: t("permission.unrestricted"),
  };
  const workbenchLabels: Record<WorkbenchTool, string> = {
    files: t("workbench.files"),
    review: t("workbench.review"),
    agent: t("workbench.agent"),
    inspector: t("workbench.inspector"),
    tasks: t("workbench.tasks"),
    usage: t("workbench.usage"),
    browser: t("workbench.browser"),
    text: t("workbench.text"),
    git: t("workbench.git"),
    terminal: t("workbench.terminal"),
    engines: t("workbench.engines"),
    fallback: t("workbench.fallback"),
    team: t("workbench.team"),
  };
  const localizeSystem = (value: string | null | undefined, fallback = t("system.operationFailed")) =>
    localizeSystemText(locale, value, fallback);
  const [workspace, setWorkspace] = useState("");
  const [workspaceKind, setWorkspaceKind] = useState<WorkspaceKind | null>(null);
  const [workspaceCreating, setWorkspaceCreating] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [timeline, setTimeline] = useState<HubTimelineEvent[]>([]);
  // Drawer handoff expansion (by handoff_id) for the full cross-engine
  // envelope: memory, skills, MCP pointers and filtered transcript.
  const [sessions, setSessions] = useState<HubSessionSummary[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [engine, setEngine] = useState("alva");
  const [selectedTarget, setSelectedTarget] = useState<EngineTarget | null>(null);
  const [pendingEngine, setPendingEngine] = useState<string | null>(null);
  const [profile, setProfile] = useState<PermissionProfile>("workspace_write");
  const [transcriptSharing, setTranscriptSharingState] = useState(false);
  const [eligibility, setEligibility] = useState<EngineEligibility[]>([]);
  // request_id → outcome, so an answered approval card cannot be re-sent.
  // "expired" marks a card whose engine already auto-denied / attempt ended.
  const [answeredPermissions, setAnsweredPermissions] = useState<
    Record<string, "allowed" | "denied" | "expired">
  >({});
  const [fallbackPolicy, setFallbackPolicy] = useState<FallbackPolicy>({
    targets: [],
    updated_at: 0,
  });
  const [policySaving, setPolicySaving] = useState(false);
  const [sessionError, setSessionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [cancellableSessionId, setCancellableSessionId] = useState<string | null>(null);
  const [readiness, setReadiness] = useState<EngineReadiness[]>([]);
  const [providerRoutes, setProviderRoutes] = useState<ModelRoute[]>([]);
  const [defaultRoute, setDefaultRoute] = useState<ModelRoute | null>(null);
  const [providers, setProviders] = useState<ProviderProfile[]>([]);
  const [providersLoading, setProvidersLoading] = useState(false);
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
  const [workbenchOpen, setWorkbenchOpen] = useState(() => isSidebarWindow() || queryValue("workbench") === "1");
  const sidebarWindow = isSidebarWindow();
  // UI10: <760px the sidebar becomes an off-canvas sheet with its own toggle.
  const [narrowSidebarOpen, setNarrowSidebarOpen] = useState(false);
  useEffect(() => {
    if (!narrowSidebarOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if ((event.target as Element | null)?.closest?.("[role=dialog]")) return;
      setNarrowSidebarOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [narrowSidebarOpen]);
  const [activeWorkbenchTool, setActiveWorkbenchTool] = useState<WorkbenchTool>(initialWorkbenchTool);
  const [openWorkbenchTools, setOpenWorkbenchTools] = useState<WorkbenchTool[]>(() => [initialWorkbenchTool()]);
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

  function openWorkbenchTool(tool: WorkbenchTool) {
    setOpenWorkbenchTools((current) => current.includes(tool) ? current : [...current, tool]);
    setActiveWorkbenchTool(tool);
    setWorkbenchOpen(true);
  }

  function closeWorkbenchTool(tool: WorkbenchTool) {
    setOpenWorkbenchTools((current) => {
      const closingIndex = current.indexOf(tool);
      const next = current.filter((entry) => entry !== tool);
      if (activeWorkbenchTool === tool) {
        const nextActive = next[Math.min(Math.max(closingIndex, 0), next.length - 1)];
        if (nextActive) setActiveWorkbenchTool(nextActive);
        else setWorkbenchOpen(false);
      }
      return next;
    });
  }
  // Logic value: the session the long-lived timeline listener should refresh
  // (reading state there would capture a stale closure).
  const activeSessionRef = useRef<string | null>(null);
  // Only events carrying the explicit new-run correlation token may claim
  // the first live product id. Background workers never receive this token.
  const pendingNewRunTokenRef = useRef<string | null>(null);
  // R34: the composer input is the keyboard home of the conversation page.
  const composerInputRef = useRef<HTMLTextAreaElement | null>(null);
  const previousWorkspaceRef = useRef(workspace);

  // R34: after a workspace is chosen — gate project/independent pick or a
  // session opened from the sidebar — focus returns to the input so a
  // keyboard-only flow can continue with typing instead of tab hunting.
  useEffect(() => {
    if (workspace && previousWorkspaceRef.current !== workspace) {
      composerInputRef.current?.focus();
    }
    previousWorkspaceRef.current = workspace;
  }, [workspace]);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void listenTimeline((event) => {
      if (active) {
        if (
          activeSessionRef.current === null &&
          pendingNewRunTokenRef.current !== null &&
          event.run_token === pendingNewRunTokenRef.current &&
          (event.status === "user_message" || event.status === "running")
        ) {
          activeSessionRef.current = event.product_session_id;
          setActiveSessionId(event.product_session_id);
        }
        const belongsToVisibleSession =
          event.product_session_id === activeSessionRef.current;
        if (belongsToVisibleSession) {
          setTimeline((current) => mergeLiveTimeline(current, event));
        }
        if (belongsToVisibleSession && event.status === "running") {
          setCancellableSessionId(event.product_session_id);
        }
        // A new approval request may merge with others: refresh the cards.
        if (belongsToVisibleSession && event.status === "permission_required") {
          void getPermissionGroups().then((groups) => {
            if (active) setApprovalGroups(groups);
          });
        }
        // Terminal events change what the store says about this session, and
        // both the details drawer and the Stop gate read that.
        if (
          event.product_session_id === activeSessionRef.current &&
          ["completed", "failed", "interrupted", "fallback", "exhausted"].includes(
            event.status,
          )
        ) {
          setCancellableSessionId(null);
          void getHubSession(event.product_session_id).then((detail) => {
            if (active && detail) setSessionDetail(detail);
          });
        }
        // Worker pumps finish in the background — pull the fresh queue state.
        if (belongsToVisibleSession && event.status === "worker" && teamLeadRef.current) {
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
    void refreshProviders(workspace || null);
  }, [workspace]);

  async function refreshProviders(workspacePath: string | null) {
    setProvidersLoading(true);
    try {
      const [nextProviders, nextRoutes, nextDefault] = await Promise.all([
        listModelProviders(workspacePath),
        listModelRoutes(workspacePath),
        getDefaultModelRoute(workspacePath),
      ]);
      setProviders(nextProviders);
      setProviderRoutes(nextRoutes);
      setDefaultRoute(nextDefault);
      if (activeSessionRef.current === null) {
        const usableDefault = nextDefault?.available
          ? nextRoutes.find((route) =>
              route.target.engine_id === nextDefault.target.engine_id &&
              route.target.provider_id === nextDefault.target.provider_id &&
              route.target.model_id === nextDefault.target.model_id &&
              route.available)
          : undefined;
        setSelectedTarget(usableDefault?.target ?? null);
        if (usableDefault) setEngine(usableDefault.target.engine_id);
      }
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setProvidersLoading(false);
    }
  }

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
          ? locale === "zh-CN" ? "修复完成。" : `Repair finished. ${result.outcome.log.trim()}`.trim()
          : locale === "zh-CN" ? "修复失败。" : `Repair failed. ${result.outcome.log.trim()}`.trim(),
      }));
    } catch (error) {
      // A refused repair (e.g. stale plan) lands on the same card as any
      // other outcome — never attributed to the selected engine.
      setRepairLogs((current) => ({
        ...current,
        [engineId]: locale === "zh-CN" ? "修复失败。请在检查器中查看技术详情。" : `Repair failed. ${error instanceof Error ? error.message : String(error)}`,
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
  const attemptIsLive =
    activeSessionId !== null && cancellableSessionId === activeSessionId;

  const latest = timeline.at(-1);
  const statusLabel = useMemo(
    () => statusLabelFor(latest, attemptIsLive, t),
    [latest, attemptIsLive, t],
  );

  const activeBranch = sessionDetail?.branches.find(
    (branch) => branch.branch_id === sessionDetail.session.active_branch_id,
  );
  const activeBranchTarget: EngineTarget | null = activeBranch ? {
    engine_id: activeBranch.engine_id,
    model_id: activeBranch.model_id,
    provider_id: activeBranch.provider_id,
    account_source: activeBranch.account_source,
    billing_route: activeBranch.billing_route,
    automatic_use_authorized: activeBranch.automatic_use_authorized,
    required_capabilities: activeBranch.required_capabilities,
    reasoning_effort: activeBranch.reasoning_effort,
    fast_mode: activeBranch.fast_mode,
  } : null;
  const configuredTarget = activeSessionId
    ? sessionDetail?.pending_switch?.target ?? activeBranchTarget
    : selectedTarget;

  const modelRoutes = useMemo<HubModelRoute[]>(
    () => {
      return providerRoutes.map((route) => {
        const target = route.target;
        const configured = configuredTarget
          && configuredTarget.engine_id === target.engine_id
          && configuredTarget.provider_id === target.provider_id
          && configuredTarget.model_id === target.model_id
          ? configuredTarget
          : target;
        const health = readiness.find(
          (candidate) => candidate.engine_id === target.engine_id,
        );
        const engineEligibility = eligibility.find(
          (candidate) => candidate.engine_id === target.engine_id,
        );
        return {
          engineId: target.engine_id,
          engineLabel:
            health?.display_name ?? route.engine_label,
          modelId: target.model_id,
          modelLabel: route.model_label,
          providerId: target.provider_id,
          accountSource: localize(target.account_source),
          billingRoute: localize(target.billing_route),
          description: localizeSystem(route.description, t("modelPicker.routeDescription")),
          capabilities: (route.capabilities.length > 0 ? route.capabilities : health?.capabilities ?? []).map(localize),
          reasoningEfforts: route.reasoning_efforts,
          reasoningEffort: configured.reasoning_effort ?? route.default_reasoning_effort,
          supportsFastMode: route.supports_fast_mode,
          fastMode: configured.fast_mode,
          status:
            health && health.status !== "healthy"
              ? localize(readinessBadge[health.status])
              : undefined,
          disabled:
            !route.available ||
            health?.status !== "healthy" ||
            engineEligibility?.eligible === false,
          disabledReason:
            !route.available
              ? localizeSystem(route.unavailable_reason, t("app.routeUnavailable")) || t("app.routeUnavailable")
              : health?.status !== "healthy"
              ? health?.reasons[0]
                ? describeReadinessReason(health.reasons[0], locale)
                : t("app.engineNotReady")
              : engineEligibility?.eligible === false
              ? localizeSystem(engineEligibility.detail, t("modelPicker.permissionIncompatible"))
              : undefined,
        };
      });
    },
    [configuredTarget, eligibility, locale, localize, providerRoutes, readiness, t],
  );

  const desiredTarget: EngineTarget | null = activeSessionId
    ? configuredTarget ?? selectedTarget
    : selectedTarget;
  const selectedModelRoute = desiredTarget
    ? modelRoutes.find(
        (route) =>
          route.engineId === desiredTarget.engine_id &&
          route.providerId === desiredTarget.provider_id &&
          route.modelId === desiredTarget.model_id,
      )
    : undefined;

  const availableEngineTargets = useMemo(
    () => Array.from(
      new Map(
        providerRoutes
          .filter((route) => route.available)
          .map((route) => [route.target.engine_id, route.target]),
      ).values(),
    ),
    [providerRoutes],
  );

  /// Everything that describes "the session I am looking at". Both the New
  /// session button and picking a workspace go through this, so neither can
  /// leak the previous session's profile, fallback chain or details.
  function resetSessionSurface() {
    setActivePage("conversation");
    setActiveSessionId(null);
    activeSessionRef.current = null;
    pendingNewRunTokenRef.current = null;
    setSessionDetail(null);
    setTeamRun(null);
    teamLeadRef.current = null;
    setTimeline([]);
    setCancellableSessionId(null);
    setPendingEngine(null);
    setEngine("alva");
    setSelectedTarget(defaultRoute?.available ? defaultRoute.target : null);
    setProfile("workspace_write");
    setTranscriptSharingState(false);
    setAnsweredPermissions({});
    // Keyed by timeline index, so a stale entry would re-expand an
    // unrelated card once the timeline refills.
    setDetailsOpen(false);
    setDispatchWorkerId(null);
  }

  function startNewSession() {
    setWorkspace("");
    setWorkspaceKind(null);
    resetSessionSurface();
    void loadGlobalFallbackPolicy().catch(appendLocalFailure);
  }

  async function selectWorkspace() {
    try {
      const selected = await chooseWorkspace();
      if (selected) {
        setWorkspace(selected);
        setWorkspaceKind("project");
        resetSessionSurface();
        await loadGlobalFallbackPolicy();
      }
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function createIndependentSession() {
    if (workspaceCreating) return;
    setWorkspaceCreating(true);
    try {
      const binding = await createSessionWorkspace();
      setWorkspace(binding.path);
      setWorkspaceKind(binding.kind);
      resetSessionSurface();
      await loadGlobalFallbackPolicy();
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setWorkspaceCreating(false);
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
    setWorkspaceKind(detail.session.workspace_kind ?? "project");
    setEngine(detail.session.engine_id);
    const activeBranch = detail.branches.find(
      (branch) => branch.branch_id === detail.session.active_branch_id,
    );
    setSelectedTarget(detail.pending_switch?.target ?? (activeBranch ? {
      engine_id: activeBranch.engine_id,
      model_id: activeBranch.model_id,
      provider_id: activeBranch.provider_id,
      account_source: activeBranch.account_source,
      billing_route: activeBranch.billing_route,
      automatic_use_authorized: activeBranch.automatic_use_authorized,
      required_capabilities: activeBranch.required_capabilities,
      reasoning_effort: activeBranch.reasoning_effort,
      fast_mode: activeBranch.fast_mode,
    } : null));
    setProfile(detail.session.permission_profile);
    setTranscriptSharingState(detail.session.include_full_transcript);
    setAnsweredPermissions({});
    setPendingEngine(detail.pending_switch?.target.engine_id ?? null);
    setFallbackPolicy(detail.fallback_policy);
    setTimeline(projectTimeline(detail, locale));
    setCancellableSessionId(
      detail.session.status === "running" && detail.session.engine_session_id
        ? detail.session.product_session_id
        : null,
    );
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
      const catalogTarget = availableEngineTargets.find((target) => target.engine_id === engineId);
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

  async function selectModelRoute(route: HubModelRoute) {
    const catalogTarget = providerRoutes.find(
      (candidate) =>
        candidate.target.engine_id === route.engineId &&
        candidate.target.provider_id === route.providerId &&
        candidate.target.model_id === route.modelId,
    )?.target;
    const target: EngineTarget = {
      engine_id: route.engineId,
      model_id: route.modelId,
      provider_id: route.providerId,
      account_source: catalogTarget?.account_source ?? route.accountSource,
      billing_route: catalogTarget?.billing_route ?? route.billingRoute,
      automatic_use_authorized: catalogTarget?.automatic_use_authorized ?? false,
      required_capabilities: catalogTarget?.required_capabilities ?? route.capabilities ?? [],
      reasoning_effort: route.reasoningEffort,
      fast_mode: route.fastMode,
    };
    if (!activeSessionId) {
      setEngine(route.engineId);
      setSelectedTarget(target);
      setPendingEngine(null);
      return;
    }
    try {
      const outcome = await switchHubEngine(
        activeSessionId,
        target,
        `User explicitly selected ${route.engineLabel} · ${route.modelLabel}`,
      );
      setEngine(outcome.active_engine_id);
      setSelectedTarget(target);
      setPendingEngine(outcome.disposition === "queued" ? route.engineId : null);
      await loadSession(activeSessionId);
      await refreshSessions();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function saveProvider(provider: SaveProviderProfile) {
    await saveModelProvider(provider, workspace || null);
    await refreshProviders(workspace || null);
  }

  async function setDefaultProviderRoute(target: EngineTarget) {
    const saved = await setDefaultModelRoute(target, workspace || null);
    setDefaultRoute(saved);
    if (!activeSessionId) {
      setSelectedTarget(saved.target);
      setEngine(saved.target.engine_id);
    }
  }

  async function discoverProviderModels(providerId: string) {
    return await discoverModelsForProvider(providerId, workspace || null);
  }

  async function testProviderRoute(target: EngineTarget) {
    return await testModelRoute(target, workspace || null);
  }

  function appendLocalFailure(error: unknown) {
    setTimeline((current) => [
      ...current,
      {
        status: "failed",
        engine_id: engine,
        product_session_id: "setup",
        message: localizeSystem(error instanceof Error ? error.message : String(error)),
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
      setCancellableSessionId(null);
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function switchBack(source: EngineTarget) {
    const route = modelRoutes.find(
      (candidate) =>
        candidate.engineId === source.engine_id &&
        candidate.providerId === source.provider_id &&
        candidate.modelId === source.model_id &&
        !candidate.disabled,
    );
    if (!route) {
      throw new Error(
        `The previous route ${source.engine_id}/${source.provider_id}/${source.model_id} is unavailable.`,
      );
    }
    await selectModelRoute(route);
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
      const outcome = await respondHubPermission(productSessionId, requestId, allow);
      if (outcome === "expired") {
        // The engine already auto-denied (or the attempt ended) before the
        // user replied — never record a decision, just mark the card stale.
        setAnsweredPermissions((current) => ({ ...current, [requestId]: "expired" }));
      } else {
        setAnsweredPermissions((current) => ({
          ...current,
          [requestId]: allow ? "allowed" : "denied",
        }));
      }
      // A group card must not keep offering a request that is now answered
      // (or no longer pending).
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
    if (!selectedModelRoute || selectedModelRoute.disabled) {
      appendLocalFailure(new Error(
        activeSessionId
          ? "This session's pinned model route is unavailable. Restore it in Settings before continuing."
          : "Choose an available default model in Settings before starting a session.",
      ));
      return;
    }

    setBusy(true);
    setPrompt("");
    const runToken = activeSessionId === null ? createRunToken() : null;
    pendingNewRunTokenRef.current = runToken;
    try {
      const outcome = await runHubPrompt({
        prompt: text,
        workspace,
        engine_id: engine,
        product_session_id: activeSessionId,
        // The profile is fixed at session creation; resumes never restate it.
        permission_profile: activeSessionId ? null : profile,
      }, activeSessionId ? null : selectedTarget, runToken);
      setActiveSessionId(outcome.product_session_id);
      activeSessionRef.current = outcome.product_session_id;
      await refreshSessions();
      await loadSession(outcome.product_session_id);
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      pendingNewRunTokenRef.current = null;
      setBusy(false);
    }
  }

  return (
    <UIProvider>
    <AppFrame
      onWorkbenchDismiss={() => setWorkbenchOpen(false)}
      sidebar={
        sidebarWindow ? null : (
        <>
        <HubSidebar
          activePage={activePage}
          activeSessionId={activeSessionId}
          busy={busy}
          formatUpdatedAt={(updatedAt) => formatRelativeTime(updatedAt, Date.now(), locale)}
          narrowOpen={narrowSidebarOpen}
          onNewSession={() => { startNewSession(); setNarrowSidebarOpen(false); }}
          onNavigate={setActivePage}
          onNarrowNavigate={() => setNarrowSidebarOpen(false)}
          onOpenSession={(session) => { void openSession(session); setNarrowSidebarOpen(false); }}
          onSessionQueryChange={setSessionQuery}
          sessionError={sessionError ? localize(sessionError) : null}
          sessionQuery={sessionQuery}
          sessions={filteredSessions}
          totalSessionCount={sessions.length}
        />
        <button
          aria-hidden={!narrowSidebarOpen}
          aria-label={locale === "zh-CN" ? "关闭导航" : "Close navigation"}
          className="ui-sidebar-scrim"
          data-state={narrowSidebarOpen ? "open" : "closed"}
          onClick={() => setNarrowSidebarOpen(false)}
          tabIndex={narrowSidebarOpen ? 0 : -1}
          type="button"
        />
        </>
        )
      }
      workbench={
        <Workbench className="hub-workbench">
          <TabsRoot className="hub-workbench__tabs" onValueChange={(value) => setActiveWorkbenchTool(value as WorkbenchTool)} value={activeWorkbenchTool}>
            <TabsList aria-label={t("app.workbenchPanels")} className="hub-workbench__tab-list">
              <div className="hub-workbench__open-tabs">
                {openWorkbenchTools.map((tool) => {
                  const meta = workbenchToolMeta[tool];
                  const label = workbenchLabels[tool];
                  const ToolIcon = meta.icon;
                  return (
                    <div className="hub-workbench__tab" key={tool}>
                      <TabsTrigger value={tool}>
                        <ToolIcon aria-hidden="true" size={13} />
                        <span>{label}</span>
                      </TabsTrigger>
                      <button
                        aria-label={`${t("workbench.close")} · ${label}`}
                        className="hub-workbench__tab-close"
                        onClick={() => closeWorkbenchTool(tool)}
                        type="button"
                      >
                        <X aria-hidden="true" size={12} />
                      </button>
                    </div>
                  );
                })}
              </div>
              <div className="hub-workbench__tab-actions">
                <MenuRoot>
                  <MenuTrigger asChild>
                    <IconButton icon={<Plus aria-hidden="true" size={15} />} label={t("workbench.addTabLabel")} variant="ghost" />
                  </MenuTrigger>
                  <MenuContent align="end" aria-label={t("workbench.addTabLabel")} className="hub-workbench__add-menu">
                    <MenuLabel>{t("workbench.addTab")}</MenuLabel>
                    {workbenchTools.map((tool) => {
                      const meta = workbenchToolMeta[tool];
                      const label = workbenchLabels[tool];
                      const ToolIcon = meta.icon;
                      return (
                        <MenuItem disabled={openWorkbenchTools.includes(tool)} key={tool} onSelect={() => openWorkbenchTool(tool)}>
                          <ToolIcon aria-hidden="true" size={14} />
                          {label}
                          {openWorkbenchTools.includes(tool) ? <span className="ui-menu-trailing">{t("workbench.open")}</span> : null}
                        </MenuItem>
                      );
                    })}
                  </MenuContent>
                </MenuRoot>
                {!sidebarWindow ? (
                  <IconButton
                    icon={<ExternalLink aria-hidden="true" size={15} />}
                    label={t("workbench.openSeparateWindow")}
                    onClick={() => window.open(`${window.location.pathname}#/sidebar-window`, "_blank", "width=1200,height=900")}
                    variant="ghost"
                  />
                ) : null}
                <IconButton icon={<PanelRight aria-hidden="true" size={15} />} label={t("workbench.close")} onClick={() => setWorkbenchOpen(false)} variant="ghost" />
              </div>
            </TabsList>
            {openWorkbenchTools.includes("files") ? <TabsContent className="hub-workbench__content" forceMount value="files"><WorkbenchPage><FilesTool detail={sessionDetail} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("review") ? <TabsContent className="hub-workbench__content" forceMount value="review"><WorkbenchPage><ReviewTool detail={sessionDetail} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("agent") ? <TabsContent className="hub-workbench__content" forceMount value="agent"><WorkbenchPage><AgentTool onFocus={(sessionId) => void openSessionById(sessionId)} run={teamRun} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("inspector") ? <TabsContent className="hub-workbench__content" forceMount value="inspector"><WorkbenchPage><InspectorTool detail={sessionDetail} events={timeline} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("tasks") ? <TabsContent className="hub-workbench__content" forceMount value="tasks"><WorkbenchPage><TasksTool run={teamRun} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("usage") ? <TabsContent className="hub-workbench__content" forceMount value="usage"><WorkbenchPage><UsageTool detail={sessionDetail} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("browser") ? <TabsContent className="hub-workbench__content" forceMount value="browser"><WorkbenchPage><BrowserTool /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("text") ? <TabsContent className="hub-workbench__content" forceMount value="text"><WorkbenchPage><TextBrowserTool detail={sessionDetail} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("git") ? <TabsContent className="hub-workbench__content" forceMount value="git"><WorkbenchPage><GitTool detail={sessionDetail} /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("terminal") ? <TabsContent className="hub-workbench__content" forceMount value="terminal"><WorkbenchPage><TerminalTool /></WorkbenchPage></TabsContent> : null}
            {openWorkbenchTools.includes("engines") ? <TabsContent className="hub-workbench__content" forceMount value="engines">
              <WorkbenchPage>
                <EngineReadinessPanel
                busy={readinessBusy}
                describeReason={(reason) => describeReadinessReason(reason, locale)}
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
            </TabsContent> : null}
            {openWorkbenchTools.includes("fallback") ? <TabsContent className="hub-workbench__content" forceMount value="fallback">
              <WorkbenchPage>
                <FallbackPolicyPanel
                activeSession={activeSessionId !== null}
                catalog={availableEngineTargets}
                onMoveTarget={moveFallbackTarget}
                onSave={() => void saveFallbackPolicy()}
                onToggleTarget={toggleFallbackTarget}
                onUpdateTarget={updateFallbackTarget}
                policy={fallbackPolicy}
                saving={policySaving}
                />
              </WorkbenchPage>
            </TabsContent> : null}
            {openWorkbenchTools.includes("team") ? <TabsContent className="hub-workbench__content" forceMount value="team">
              <WorkbenchPage>
                <TeamPanel
                activeSession={activeSessionId !== null}
                adoption={adoption}
                busy={teamBusy}
                catalog={availableEngineTargets}
                claims={teamClaims}
                describeClaim={(claim) => describeClaim(claim, locale)}
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
            </TabsContent> : null}
          </TabsRoot>
        </Workbench>
      }
      workbenchOpen={workbenchOpen}
    >

      {sidebarWindow ? null : (
      <div className="main-column">
        {activePage === "conversation" ? (
          <>
        <HubTopbar
          detailsAvailable={sessionDetail !== null}
          detailsOpen={detailsOpen}
          onToggleDetails={() => setDetailsOpen((open) => !open)}
          onToggleWorkbench={() => {
            if (workbenchOpen) setWorkbenchOpen(false);
            else openWorkbenchTool(openWorkbenchTools.at(-1) ?? "agent");
          }}
          status={latest?.status ?? "ready"}
          statusLabel={statusLabel}
          title={sessionDetail?.turns.at(-1)?.prompt ?? t("conversation.new")}
          workbenchOpen={workbenchOpen}
          workspace={workspace}
          workspaceLabel={
            workspace && workspaceKind === "session"
              ? t("nav.independentSession")
              : undefined
          }
          onToggleSidebar={() => setNarrowSidebarOpen((open) => !open)}
        />

        <ContentFrame className="hub-session-frame">
          <ScrollRegion className="hub-session-scroll" data-chat-scroll-region="">
            {!workspace && activeSessionId === null ? (
              <>
                <WorkspaceGate
                  busy={busy || workspaceCreating}
                  onChooseProject={() => void selectWorkspace()}
                  onCreateSession={() => void createIndependentSession()}
                />
                {/* R17: 失败通知经时间线可见，即便尚未选择工作区。 */}
                {timeline.length > 0 ? (
                  <HubTimeline
                    activeSessionId={activeSessionId}
                    answeredPermissions={answeredPermissions}
                    attemptIsLive={attemptIsLive}
                    events={timeline}
                    onAnswerPermission={(productSessionId, requestId, allow) =>
                      void answerPermission(productSessionId, requestId, allow)
                    }
                    onStop={() => void stopActiveAttempt()}
                    onSwitchBack={(source) => void switchBack(source)}
                  />
                ) : null}
              </>
            ) : (
            <>
            {/* R29/R30: 消息阅读流常驻——用户问题与回答从流式到终态始终可读，
                不按运行状态卸载；Markdown/代码复制/滚动跟随由 ChatTranscript
                承担。 */}
            <ChatTranscript
              events={timeline}
              streaming={attemptIsLive}
            />
            {/* 执行/控制视图只在确有未落定内容时渲染：运行中、待审批、或
                失败/中断/回退等需要动作与原因的事件；完成的执行细节（含
                终态后残留的 running 工具标记）归 Inspector。 */}
            {(attemptIsLive ||
              approvalGroups.some((g) => g.members.length > 0) ||
              timeline.some((event) =>
                ["failed", "interrupted", "exhausted", "permission_required", "fallback", "queued"].includes(
                  event.status,
                ),
              )) ? (
              <HubTimeline
                activeSessionId={activeSessionId}
                answeredPermissions={answeredPermissions}
                attemptIsLive={attemptIsLive}
                events={timeline}
                onAnswerPermission={(productSessionId, requestId, allow) =>
                  void answerPermission(productSessionId, requestId, allow)
                }
                onStop={() => void stopActiveAttempt()}
                onSwitchBack={(source) => void switchBack(source)}
              />
            ) : null}
            </>
            )}
          </ScrollRegion>

          <DockRegion className="hub-session-dock">

          {approvalGroups
            .filter((group) => group.members.length > 1)
            .map((group) => (
            <Notice
              actions={
                <>
                <Button
                  aria-label={t("approval.allowMany", { count: group.members.length, tool: group.tool_name })}
                  onClick={() => void decideApprovalGroup(group, true)}
                  size="medium"
                  variant="primary"
                >
                  {t("timeline.allow")}
                </Button>
                <Button
                  aria-label={t("approval.denyMany", { count: group.members.length, tool: group.tool_name })}
                  onClick={() => void decideApprovalGroup(group, false)}
                  size="medium"
                  variant="ghost"
                >
                  {t("timeline.deny")}
                </Button>
                </>
              }
              className="approval-group"
              description={
                group.members[0]?.resource
                  ? t("approval.needResource", { count: group.members.length, resource: group.members[0].resource })
                  : t("approval.needPermission", { count: group.members.length })
              }
              heading={t("approval.allowTool", { tool: group.tool_name })}
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
                    ? describeReadinessReason(health.reasons[0], locale)
                    : t("readiness.chooseAnother")
                }
                heading={t("readiness.engineNotReady", { engine: health.display_name })}
                role="status"
                tone="warning"
              />
            );
          })()}

          <HubComposer
          busy={busy}
          canStop={attemptIsLive}
          controls={
            <>
              <SelectField
                ariaLabel={t("composer.permission")}
                className="hub-composer__permission"
                disabled={activeSessionId !== null}
                onValueChange={(value) => setProfile(value as PermissionProfile)}
              options={(Object.keys(localizedProfileLabel) as PermissionProfile[]).map((candidate) => ({
                  label: localizedProfileLabel[candidate],
                  value: candidate,
                }))}
                value={profile}
              />
              {activeSessionId ? (
                <Checkbox
                  checked={transcriptSharing}
                  className="transcript-toggle"
                  label={t("composer.shareTranscript")}
                  onCheckedChange={(checked) => {
                    const enabled = checked === true;
                    setTranscriptSharingState(enabled);
                    void setTranscriptSharing_backend(enabled);
                  }}
                />
              ) : null}
            </>
          }
          disabled={!workspace || selectedModelRoute?.disabled !== false}
          engineId={engine}
          modelPicker={
            modelRoutes.length > 0 ? (
              <ModelPicker
                onOptionsChange={(route, update) => void selectModelRoute({
                  ...route,
                  reasoningEffort: update.reasoningEffort ?? route.reasoningEffort,
                  fastMode: update.fastMode ?? route.fastMode,
                })}
                onSelect={(route) => void selectModelRoute(route)}
                routes={modelRoutes}
                selectedEngineId={pendingEngine ?? selectedModelRoute?.engineId ?? modelRoutes[0].engineId}
                selectedModelId={selectedModelRoute?.modelId ?? ""}
                selectedProviderId={selectedModelRoute?.providerId ?? ""}
              />
            ) : null
          }
          onChange={setPrompt}
          inputRef={composerInputRef}
          onStop={() => void stopActiveAttempt()}
          onSubmit={submit}
          placeholder={t("composer.placeholder", { engine: pendingEngine ?? engine })}
          status={
            <>
              {/* UI07/UI08: 独立会话不常驻展示 UUID 目录名，完整路径放 tooltip。 */}
              <span title={workspace ?? undefined}>
                {workspace && workspaceKind
                  ? workspaceKind === "session"
                    ? t("workspace.kindSession")
                    : `${t("workspace.kindProject")} · ${displayWorkspace(workspace)}`
                  : t("workspace.selectFirst")}
              </span>
              {selectedModelRoute ? (
                <span>
                  {selectedModelRoute.engineLabel} · {selectedModelRoute.modelLabel}
                </span>
              ) : workspace ? <span>{t("composer.chooseModel")}</span> : null}
              <span>
                {localizedProfileLabel[profile]}
                {fallbackPolicy.targets.length > 0
                  ? ` · ${t("fallback.count", { count: fallbackPolicy.targets.length })}`
                  : ""}
              </span>
            </>
          }
            value={prompt}
          />
          </DockRegion>
        </ContentFrame>
        {/* UI06: Details 打开即见——作为右侧固定 rail 覆盖显示，聊天滚动
            位置不受打开/关闭影响，长会话无需滚到末尾。 */}
        {detailsOpen && sessionDetail ? (
          <aside aria-label={t("topbar.details")} className="hub-details-rail">
            <SessionDetails
              describeReason={(reason) => describeReadinessReason(reason, locale)}
              detail={sessionDetail}
              memorySaving={memorySaving}
              onAddMemory={(entry) => void addMemory(entry)}
              readiness={readiness}
              readinessLabel={readinessBadge}
            />
          </aside>
        ) : null}
          </>
        ) : activePage === "automations" ? (
          <AutomationsPage />
        ) : activePage === "extensions" ? (
          <ExtensionsPage onStartChat={() => setActivePage("conversation")} />
        ) : activePage === "settings" ? (
          <SettingsPage
            defaultTarget={defaultRoute?.target ?? null}
            onDiscoverProviderModels={discoverProviderModels}
            onSaveProvider={saveProvider}
            onSetDefaultProvider={setDefaultProviderRoute}
            onTestProviderRoute={testProviderRoute}
            providers={providers}
            providersLoading={providersLoading}
            routes={providerRoutes}
          />
        ) : (
          <SearchPage
            onNavigate={setActivePage}
            onOpenSession={(productSessionId) => {
              setActivePage("conversation");
              void openSessionById(productSessionId);
            }}
            onOpenWorkbench={(tool) => {
              openWorkbenchTool(tool);
            }}
            sessions={sessions.map((session) => ({
              description: `${displayWorkspace(session.workspace)} · ${session.engine_id} · ${localize(session.status)}`,
              id: session.product_session_id,
              title: session.last_prompt ?? t("nav.untitledSession"),
            }))}
          />
        )}
      </div>
      )}
    </AppFrame>
    </UIProvider>
  );
}
