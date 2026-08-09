// INPUT:  @tauri-apps/api, @tauri-apps/plugin-dialog
// OUTPUT: workspace/run/switch/policy/readiness/timeline plus persisted Hub IPC DTOs
// POS:    Typed multi-engine authorization boundary between React and Hub Desktop commands.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

export type TimelineStatus =
  | "running"
  | "completed"
  | "failed"
  | "interrupted"
  | "queued"
  | "switched"
  | "cancelled"
  | "fallback"
  | "exhausted"
  | "permission_required"
  | "worker";

export type PermissionProfile =
  | "interactive"
  | "workspace_write"
  | "unrestricted";

export interface PermissionRequestInfo {
  request_id: string;
  tool_name: string;
  description: string | null;
  engine_session_id: string;
  resource?: string | null;
}

export interface TimelineEvent {
  status: TimelineStatus;
  engine_id: string;
  product_session_id: string;
  message: string | null;
  created_at?: number;
  permission_request?: PermissionRequestInfo;
}

export interface RunRequest {
  prompt: string;
  workspace: string;
  engine_id: string;
  product_session_id: string | null;
  permission_profile: PermissionProfile | null;
}

export interface RunOutcome {
  product_session_id: string;
  result: string | null;
  failed: boolean;
}

export interface HubSessionSummary {
  product_session_id: string;
  workspace: string;
  active_branch_id: string;
  engine_id: string;
  model_id: string;
  provider_id: string;
  engine_session_id: string | null;
  status: string;
  updated_at: number;
  last_prompt: string | null;
  last_result: string | null;
  permission_profile: PermissionProfile;
  include_full_transcript: boolean;
}

export interface StoredTurn {
  turn_id: string;
  prompt: string;
  result: string | null;
  error: string | null;
  status: string;
  created_at: number;
  completed_at: number | null;
  attempts: StoredAttempt[];
}

export interface StoredUsage {
  input_tokens: number;
  output_tokens: number;
  total_cost_usd: number | null;
  duration_ms: number;
  num_turns: number;
}

export interface SideEffectEvidence {
  tool_call_id: string;
  tool_name: string;
  succeeded: boolean | null;
  duration_ms: number | null;
  path: string | null;
}

export interface StoredAttempt {
  attempt_id: string;
  turn_id: string;
  branch_id: string;
  engine_id: string;
  status: string;
  failure_reason: string | null;
  usage: StoredUsage | null;
  side_effects: SideEffectEvidence[];
  started_at: number;
  completed_at: number | null;
}

export interface HandoffCompletedWork {
  prompt: string;
  result: string;
}

export interface HandoffPendingWork {
  prompt: string;
  reason: string;
}

export interface HandoffPackage {
  handoff_id: string;
  product_session_id: string;
  source_branch_id: string;
  target_branch_id: string;
  source: EngineTarget;
  target: EngineTarget;
  kind: "full" | "delta";
  reason: string;
  user_goal: string | null;
  latest_instruction: string | null;
  accepted_decisions: string[];
  completed_work: HandoffCompletedWork[];
  pending_work: HandoffPendingWork[];
  changed_files: string[];
  recent_commands: string[];
  verification_results: string[];
  workspace_summary: string;
  /// Collaboration context when this session leads or belongs to a TeamRun.
  team_snapshot: string | null;
  transcript: TranscriptTurn[];
  session_memory: string[];
  available_skills: SkillSnapshot[];
  available_mcp: McpSnapshot[];
  created_at: number;
  used_at: number | null;
}

export interface TranscriptTurn {
  prompt: string;
  result: string | null;
}

export interface SkillSnapshot {
  name: string;
  description: string;
  path: string;
}

export interface McpSnapshot {
  id: string;
  display_name: string;
  transport: string;
}

export interface HubSessionDetail {
  session: HubSessionSummary;
  turns: StoredTurn[];
  branches: StoredBranch[];
  handoffs: HandoffPackage[];
  timeline_events: StoredTimelineEvent[];
  pending_switch: PendingSwitch | null;
  fallback_policy: FallbackPolicy;
  memory: string[];
}

export interface StoredBranch {
  branch_id: string;
  engine_id: string;
  model_id: string;
  provider_id: string;
  engine_session_id: string | null;
  status: string;
  created_at: number;
  updated_at: number;
}

export interface EngineTarget {
  engine_id: string;
  model_id: string;
  provider_id: string;
  account_source: string;
  billing_route: string;
  automatic_use_authorized: boolean;
  required_capabilities: string[];
}

export interface FallbackPolicy {
  targets: EngineTarget[];
  updated_at: number;
}

export interface PendingSwitch {
  switch_id: string;
  source_branch_id: string;
  target: EngineTarget;
  reason: string;
  requested_at: number;
}

export interface StoredTimelineEvent {
  event_id: string;
  kind: string;
  source_branch_id: string;
  target_branch_id: string | null;
  source_engine_id: string;
  target_engine_id: string;
  reason: string;
  created_at: number;
}

export type ReadinessStatus =
  | "healthy"
  | "missing"
  | "incompatible"
  | "unauthenticated"
  | "unhealthy";

export type StreamHealth = "ok" | "untested" | "failing";

export type ReadinessReason =
  | { code: "binary_not_found"; searched: string[] }
  | { code: "version_unreadable"; detail: string }
  | { code: "version_incompatible"; found: string; minimum: string }
  | { code: "unauthenticated"; remedy: string }
  | { code: "provider_key_missing"; remedy: string }
  | { code: "stream_failing"; last_failure: string };

export interface EngineReadiness {
  engine_id: string;
  display_name: string;
  status: ReadinessStatus;
  binary_path: string | null;
  version: string | null;
  minimum_version: string;
  capabilities: string[];
  authenticated: boolean | null;
  stream_health: StreamHealth;
  reasons: ReadinessReason[];
}

export interface SwitchOutcome {
  disposition: "applied" | "queued";
  active_branch_id: string;
  active_engine_id: string;
  target_engine_id: string;
}

import {
  demoDetail,
  demoFallbackPolicy,
  demoReadiness,
  demoSessions,
  demoTeamRun,
} from "./demo-data";

const hasTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
// Dev-only: ?demo=1 shows deterministic data so wireframe/vision review
// can inspect a DATA state without the Tauri runtime. Never active in
// production (hasTauri bypasses every branch).
const DEMO =
  !hasTauri &&
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).has("demo");

export async function chooseWorkspace(): Promise<string | null> {
  if (!hasTauri) {
    throw new Error("Workspace selection requires the ALVA Hub desktop runtime.");
  }
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function runHubPrompt(
  request: RunRequest,
): Promise<RunOutcome> {
  if (!hasTauri) {
    throw new Error("Running an engine requires the ALVA Hub desktop runtime.");
  }
  return await invoke<RunOutcome>("run_hub_prompt", { request });
}

export async function switchHubEngine(
  productSessionId: string,
  engineId: string,
  reason: string,
): Promise<SwitchOutcome> {
  if (!hasTauri) {
    throw new Error("Engine switching requires the ALVA Hub desktop runtime.");
  }
  return await invoke<SwitchOutcome>("switch_hub_engine", {
    request: { productSessionId, engineId, reason },
  });
}

export async function listHubSessions(): Promise<HubSessionSummary[]> {
  if (DEMO) return demoSessions;
  if (!hasTauri) return [];
  return await invoke<HubSessionSummary[]>("list_hub_sessions");
}

export async function getHubSession(
  productSessionId: string,
): Promise<HubSessionDetail | null> {
  if (DEMO) return demoDetail();
  if (!hasTauri) return null;
  return await invoke<HubSessionDetail | null>("get_hub_session", {
    productSessionId,
  });
}

export async function addSessionMemory(
  productSessionId: string,
  entry: string,
): Promise<string[]> {
  if (!hasTauri) return [];
  return await invoke<string[]>("add_session_memory", {
    productSessionId,
    entry,
  });
}

export async function getGlobalFallbackPolicy(): Promise<FallbackPolicy> {
  if (DEMO) return demoFallbackPolicy();
  if (!hasTauri) {
    return { targets: [], updated_at: 0 };
  }
  return await invoke<FallbackPolicy>("get_global_fallback_policy");
}

export async function setGlobalFallbackPolicy(
  targets: EngineTarget[],
): Promise<FallbackPolicy> {
  if (!hasTauri) {
    throw new Error("Fallback settings require the ALVA Hub desktop runtime.");
  }
  return await invoke<FallbackPolicy>("set_global_fallback_policy", { targets });
}

export async function setSessionFallbackPolicy(
  productSessionId: string,
  targets: EngineTarget[],
): Promise<FallbackPolicy> {
  if (!hasTauri) {
    throw new Error("Fallback settings require the ALVA Hub desktop runtime.");
  }
  return await invoke<FallbackPolicy>("set_session_fallback_policy", {
    productSessionId,
    targets,
  });
}

export interface EngineEligibility {
  engine_id: string;
  eligible: boolean;
  relation: "equivalent" | "stricter" | null;
  detail: string;
}

export async function getPermissionEligibility(
  profile: PermissionProfile,
): Promise<EngineEligibility[]> {
  if (!hasTauri) return [];
  return await invoke<EngineEligibility[]>("get_permission_eligibility", {
    profile,
  });
}

export async function respondHubPermission(
  productSessionId: string,
  requestId: string,
  allow: boolean,
  message?: string,
): Promise<void> {
  if (!hasTauri) {
    throw new Error(
      "Answering a permission request requires the ALVA Hub desktop runtime.",
    );
  }
  await invoke("respond_hub_permission", {
    request: { productSessionId, requestId, allow, message: message ?? null },
  });
}

export async function setTranscriptSharing(
  productSessionId: string,
  enabled: boolean,
): Promise<void> {
  if (!hasTauri) {
    throw new Error("Transcript settings require the ALVA Hub desktop runtime.");
  }
  await invoke("set_transcript_sharing", { productSessionId, enabled });
}

export async function stopHubAttempt(productSessionId: string): Promise<void> {
  if (!hasTauri) {
    throw new Error("Stopping an attempt requires the ALVA Hub desktop runtime.");
  }
  await invoke("stop_hub_attempt", { productSessionId });
}

export interface WorktreeChange {
  path: string;
  status: string;
}

export interface AdoptionPreview {
  changes: WorktreeChange[];
  conflicts: string[];
  applies_cleanly: boolean;
  verification: string[];
}

export type WorkerLifecycle = "active" | "hibernated" | "archived";

export interface TeamWorker {
  worker_id: string;
  lifecycle: WorkerLifecycle;
  worktree_path: string | null;
  worktree_branch: string | null;
  worker_session_id: string;
  engine_id: string;
  created_at: number;
}

export type ClaimMode = "read" | "write";

export type ResourceClaim =
  | { kind: "path"; path: string; mode: ClaimMode }
  | { kind: "facility"; name: string }
  | { kind: "exclusive_tool"; tool: string };

export interface WorkerTask {
  task_id: string;
  team_run_id: string;
  worker_id: string;
  scope: string;
  status: string;
  result: string | null;
  changed_files: string[];
  verification: SideEffectEvidence[];
  claims: ResourceClaim[];
  created_at: number;
  completed_at: number | null;
}

export interface PermissionMember {
  scope: string;
  product_session_id: string;
  request_id: string;
  requester: string;
  tool_name: string;
  resource: string | null;
}

export interface PermissionGroup {
  group_key: string;
  tool_name: string;
  members: PermissionMember[];
}

export interface TeamRunDetail {
  team_run_id: string;
  autonomous: boolean;
  lead_session_id: string;
  status: string;
  workers: TeamWorker[];
  tasks: WorkerTask[];
  created_at: number;
}

export async function createTeamRun(
  leadSessionId: string,
  workerEngineId: string,
): Promise<TeamRunDetail> {
  if (!hasTauri) {
    throw new Error("Team runs require the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamRunDetail>("create_team_run", {
    leadSessionId,
    workerEngineId,
  });
}

export async function getTeamRunForLead(
  leadSessionId: string,
): Promise<TeamRunDetail | null> {
  if (DEMO) return demoTeamRun();
  if (!hasTauri) return null;
  return await invoke<TeamRunDetail | null>("get_team_run_for_lead", {
    leadSessionId,
  });
}

export async function dispatchWorkerTask(
  teamRunId: string,
  workerId: string,
  scope: string,
  claims: ResourceClaim[],
): Promise<WorkerTask> {
  if (!hasTauri) {
    throw new Error("Worker dispatch requires the ALVA Hub desktop runtime.");
  }
  return await invoke<WorkerTask>("dispatch_worker_task", {
    teamRunId,
    workerId,
    scope,
    claims,
  });
}

export async function addTeamWorker(
  teamRunId: string,
  workerEngineId: string,
  isolated: boolean,
): Promise<TeamRunDetail> {
  if (!hasTauri) {
    throw new Error("Team runs require the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamRunDetail>("add_team_worker", {
    teamRunId,
    workerEngineId,
    isolated,
  });
}

export async function getPermissionGroups(): Promise<PermissionGroup[]> {
  if (!hasTauri) return [];
  return await invoke<PermissionGroup[]>("get_permission_groups");
}

export async function respondPermissionGroup(
  groupKey: string,
  allow: boolean,
  requestIds?: string[],
): Promise<number> {
  if (!hasTauri) {
    throw new Error("Approvals require the ALVA Hub desktop runtime.");
  }
  return await invoke<number>("respond_permission_group", {
    request: { groupKey, allow, requestIds },
  });
}

export async function setTeamAutonomy(
  teamRunId: string,
  autonomous: boolean,
): Promise<TeamRunDetail> {
  if (!hasTauri) {
    throw new Error("Autonomy requires the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamRunDetail>("set_team_autonomy", {
    teamRunId,
    autonomous,
  });
}

export async function hibernateWorker(workerId: string): Promise<TeamWorker> {
  if (!hasTauri) {
    throw new Error("Hibernating requires the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamWorker>("hibernate_worker", { workerId });
}

export async function restoreWorker(workerId: string): Promise<TeamWorker> {
  if (!hasTauri) {
    throw new Error("Restoring requires the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamWorker>("restore_worker", { workerId });
}

export async function archiveWorker(workerId: string): Promise<TeamWorker> {
  if (!hasTauri) {
    throw new Error("Archiving requires the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamWorker>("archive_worker", { workerId });
}

export async function completeTeamRun(
  teamRunId: string,
): Promise<TeamRunDetail> {
  if (!hasTauri) {
    throw new Error("Ending a team run requires the ALVA Hub desktop runtime.");
  }
  return await invoke<TeamRunDetail>("complete_team_run", { teamRunId });
}

export async function previewWorkerAdoption(
  workerId: string,
): Promise<AdoptionPreview> {
  if (!hasTauri) {
    throw new Error("Adoption requires the ALVA Hub desktop runtime.");
  }
  return await invoke<AdoptionPreview>("preview_worker_adoption", { workerId });
}

export async function adoptWorkerWork(
  teamRunId: string,
  workerId: string,
): Promise<string[]> {
  if (!hasTauri) {
    throw new Error("Adoption requires the ALVA Hub desktop runtime.");
  }
  return await invoke<string[]>("adopt_worker_work", { teamRunId, workerId });
}

export async function discardWorkerWorktree(
  teamRunId: string,
  workerId: string,
): Promise<void> {
  if (!hasTauri) {
    throw new Error("Discarding requires the ALVA Hub desktop runtime.");
  }
  await invoke("discard_worker_worktree", { teamRunId, workerId });
}

export async function updateWorkerTask(
  taskId: string,
  scope: string,
): Promise<WorkerTask> {
  if (!hasTauri) {
    throw new Error("Task editing requires the ALVA Hub desktop runtime.");
  }
  return await invoke<WorkerTask>("update_worker_task", { taskId, scope });
}

export async function cancelWorkerTask(taskId: string): Promise<WorkerTask> {
  if (!hasTauri) {
    throw new Error("Task cancellation requires the ALVA Hub desktop runtime.");
  }
  return await invoke<WorkerTask>("cancel_worker_task", { taskId });
}

export async function interruptWorker(
  workerSessionId: string,
): Promise<void> {
  if (!hasTauri) {
    throw new Error("Worker interrupt requires the ALVA Hub desktop runtime.");
  }
  await invoke("interrupt_worker", { workerSessionId });
}

export type RepairKind = "install" | "upgrade" | "authenticate" | "configure";

export interface RepairAction {
  engine_id: string;
  kind: RepairKind;
  title: string;
  command: string | null;
  url: string | null;
  one_click: boolean;
}

export interface RepairResult {
  outcome: { success: boolean; log: string };
  readiness: EngineReadiness;
}

/// A bare <a target="_blank"> does nothing in a packaged wry webview, so
/// official-guidance links go through the opener plugin.
export async function openExternal(url: string): Promise<void> {
  if (!url) return;
  if (!hasTauri) {
    // Browser-only mode must fail with a clear error, never an uncaught
    // rejection (popup blockers make window.open return null).
    if (!window.open(url, "_blank", "noreferrer")) {
      throw new Error(`Could not open ${url}. Copy it into a browser instead.`);
    }
    return;
  }
  try {
    await openUrl(url);
  } catch (error) {
    throw new Error(
      `Could not open ${url}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

export async function getRepairPlan(
  workspace: string | null,
): Promise<RepairAction[]> {
  if (!hasTauri) return [];
  return await invoke<RepairAction[]>("get_repair_plan", { workspace });
}

export async function runEngineRepair(
  engineId: string,
  kind: RepairKind,
  workspace: string | null,
): Promise<RepairResult> {
  if (!hasTauri) {
    throw new Error("Engine repair requires the ALVA Hub desktop runtime.");
  }
  return await invoke<RepairResult>("run_engine_repair", {
    engineId,
    kind,
    workspace,
  });
}

export async function getEngineReadiness(
  workspace: string | null,
): Promise<EngineReadiness[]> {
  if (DEMO) return demoReadiness();
  if (!hasTauri) return [];
  return await invoke<EngineReadiness[]>("get_engine_readiness", { workspace });
}

export async function listenTimeline(
  onEvent: (event: TimelineEvent) => void,
): Promise<UnlistenFn> {
  if (!hasTauri) {
    return () => {};
  }
  return await listen<TimelineEvent>("hub_timeline", ({ payload }) => {
    onEvent(payload);
  });
}
