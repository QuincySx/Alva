// INPUT:  @tauri-apps/api, @tauri-apps/plugin-dialog
// OUTPUT: workspace/run/switch/policy/readiness/Inspector plus provider discovery and persisted Hub IPC DTOs
// POS:    Typed multi-engine authorization boundary between React and Hub Desktop commands.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

export type TimelineStatus =
  | "user_message"
  | "assistant_message"
  | "activity_started"
  | "activity_completed"
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

export type WorkspaceKind = "session" | "project";

export interface WorkspaceBinding {
  kind: WorkspaceKind;
  path: string;
}

export interface PermissionRequestInfo {
  request_id: string;
  tool_name: string;
  description: string | null;
  engine_session_id: string;
  resource?: string | null;
}

export interface TimelineEvent {
  /** Position of a fragment in one attempt-scoped message, in UTF-16 units. */
  message_position?: { stream_id: string; offset: number } | null;
  status: TimelineStatus;
  engine_id: string;
  product_session_id: string;
  message: string | null;
  created_at?: number;
  permission_request?: PermissionRequestInfo;
  item_id?: string | null;
  activity?: TimelineActivity | null;
  run_token?: string | null;
}

export interface TimelineActivity {
  attempt_id?: string | null;
  first_seen_at?: number | null;
  sequence?: number | null;
  tool_name: string;
  succeeded: boolean | null;
  duration_ms: number | null;
  path: string | null;
  tool_input?: string | null;
  tool_output?: string | null;
}

export interface RunRequest {
  prompt: string;
  workspace: string;
  engine_id: string;
  product_session_id: string | null;
  permission_profile: PermissionProfile | null;
}

export interface ProviderModel {
  id: string;
  display_name: string;
  enabled: boolean;
  registry_id?: string | null;
  context_window: number | null;
  capabilities: string[];
  supported_engines: string[];
  runtime_options?: Record<string, {
    reasoning_efforts: string[];
    default_reasoning_effort: string | null;
    supports_fast_mode: boolean;
  }>;
}

export interface ProviderProfile {
  id: string;
  display_name: string;
  kind: string;
  base_url: string;
  enabled: boolean;
  api_key_configured: boolean;
  requires_api_key: boolean;
  endpoint_editable: boolean;
  managed: boolean;
  max_output_tokens: number;
  supported_engines: string[];
  /** Backend honors delete for this connection (user-created API providers). */
  deletable: boolean;
  models: ProviderModel[];
}

export interface SaveProviderProfile {
  id: string;
  display_name: string;
  base_url: string;
  enabled: boolean;
  api_key: string | null;
  clear_api_key: boolean;
  max_output_tokens: number;
  models: ProviderModel[];
}

export interface CreateProviderProfile {
  kind: string;
  display_name: string;
  base_url: string;
  api_key: string | null;
  max_output_tokens: number;
  models: ProviderModel[];
}

export interface ModelRoute {
  target: EngineTarget;
  engine_label: string;
  model_label: string;
  provider_label: string;
  description: string;
  context_window: number | null;
  capabilities: string[];
  reasoning_efforts: string[];
  default_reasoning_effort: string | null;
  supports_fast_mode: boolean;
  available: boolean;
  unavailable_reason: string | null;
}

export interface DiscoveredProviderModel {
  id: string;
  display_name: string | null;
  suggested_registry_id: string | null;
  compatible_engines: string[];
}

export interface ProviderDiagnosticResult {
  models: DiscoveredProviderModel[];
  ok: boolean;
  latency_ms: number;
  message: string;
}

export interface RunOutcome {
  product_session_id: string;
  result: string | null;
  failed: boolean;
}

export interface HubSessionSummary {
  product_session_id: string;
  workspace: string;
  workspace_kind: WorkspaceKind;
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
  /** Owning Project (H04); null/absent surfaces in conversation mode. */
  project_id?: string | null;
}

export interface ProjectSummary {
  project_id: string;
  display_name: string;
  workspace_path: string;
  workspace_canonical: string;
  created_at: number;
  updated_at: number;
  session_count: number;
}

export interface WorktreeInfo {
  path: string;
  head: string | null;
  /** Git branch ref (e.g. "refs/heads/main") — NOT an EngineBranch. */
  branch: string | null;
  detached: boolean;
  bare: boolean;
  is_main_worktree: boolean;
  locked: string | null;
  prunable: string | null;
  managed_by_hub: boolean;
  exists_on_disk: boolean;
}

export type WorktreeDiscoveryState =
  | { code: "not_git_repository" }
  | { code: "missing_directory"; path: string }
  | { code: "git"; detail: string };

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
  first_seen_at?: number | null;
  sequence?: number | null;
  tool_call_id: string;
  tool_name: string;
  succeeded: boolean | null;
  duration_ms: number | null;
  path: string | null;
  tool_input?: string | null;
  tool_output?: string | null;
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

export interface InspectorHookRecord {
  middleware_name: string;
  hook: string;
  duration_ms: number;
  outcome: string;
  error: string | null;
  tool_call_id: string | null;
}

export interface InspectorLlmCallRecord {
  messages_sent: unknown[];
  messages_sent_count: number;
  response: unknown | null;
  input_tokens: number;
  output_tokens: number;
  duration_ms: number;
  stop_reason: string;
  error_message: string | null;
  middleware_hooks: InspectorHookRecord[];
  first_event_ms: number | null;
  time_to_first_output_ms: number | null;
  time_to_first_token_ms: number | null;
  stream_duration_ms: number | null;
  stream_event_count: number;
  visible_stream_event_count: number;
  stream_transport: string | null;
  stream_outcome: string | null;
}

export interface InspectorToolCallRecord {
  tool_call: {
    id: string;
    name: string;
    arguments: unknown;
  };
  result: unknown | null;
  is_error: boolean;
  duration_ms: number;
  middleware_hooks: InspectorHookRecord[];
}

export interface InspectorTurnRecord {
  turn_number: number;
  llm_call: InspectorLlmCallRecord;
  tool_calls: InspectorToolCallRecord[];
  duration_ms: number;
}

export interface InspectorRunRecord {
  turns: InspectorTurnRecord[];
  total_duration_ms: number;
  total_input_tokens: number;
  total_output_tokens: number;
  middleware_hooks: InspectorHookRecord[];
  user_messages: Array<{
    before_turn_number: number;
    text: string;
    timestamp_ms: number;
  }>;
}

export interface HubInspectionRecord {
  seq: number;
  record_id: string;
  parent_record_id: string | null;
  timestamp: number;
  category: "conversation" | "execution" | "extension";
  type_name: string;
  emitter: unknown;
  payload: unknown;
}

export interface HubInspectionSnapshot {
  product_session_id: string;
  branch_id: string;
  root_native_session_id: string;
  engine_id: string;
  native_session_id: string;
  parent_session_id: string | null;
  high_water_seq: number;
  live: boolean;
  capabilities: {
    level: "full" | "summary" | "unsupported";
    historical: boolean;
    live: boolean;
    raw_records: boolean;
    recursive_children: boolean;
    export_jsonl_bundle: boolean;
  };
  run: InspectorRunRecord;
  records: HubInspectionRecord[];
  children: Array<{
    child_session_id: string;
    parent_session_id: string;
    parent_run_id: string;
    spawn_tool_call_id: string;
  }>;
}

export interface StoredBranch {
  branch_id: string;
  engine_id: string;
  model_id: string;
  provider_id: string;
  account_source: string;
  billing_route: string;
  automatic_use_authorized: boolean;
  required_capabilities: string[];
  reasoning_effort: string | null;
  fast_mode: boolean;
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
  reasoning_effort: string | null;
  fast_mode: boolean;
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
  | { code: "dependency_unavailable"; detail: string }
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
  demoInspection,
  demoModelRoutes,
  demoProviderProfiles,
  demoReadiness,
  demoSessions,
  demoTeamRun,
} from "./demo-data";

const hasTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** True when the desktop IPC surface (projects, worktrees, runs) exists. */
export function hasTauriRuntime(): boolean {
  return hasTauri;
}
// Dev-only: ?demo=1 shows deterministic data so wireframe/vision review
// can inspect a DATA state without the Tauri runtime. Never active in
// production (hasTauri bypasses every branch).
const DEMO =
  !hasTauri &&
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).has("demo");

// --- HTTP transport -------------------------------------------------------
// When the app is served by alva-hub-server (no Tauri runtime), the same
// product surface reaches the headless service over HTTP + SSE instead of
// Tauri IPC. The URL defaults to same-origin, so a browser pointed at
// http://127.0.0.1:8790 talks to the service with no further config.
const HTTP = !hasTauri && !DEMO;

const HTTP_BASE =
  typeof window !== "undefined"
    ? (import.meta.env?.VITE_HUB_HTTP_BASE as string | undefined) ?? ""
    : "";

async function httpJson<T>(
  path: string,
  body?: unknown,
  method: "GET" | "POST" | "DELETE" = "POST",
): Promise<T> {
  const res = await fetch(`${HTTP_BASE}${path}`, {
    method,
    headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    let detail = `HTTP ${res.status}`;
    try {
      const j = (await res.json()) as { error?: string };
      if (j.error) detail = j.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(detail);
  }
  return (await res.json()) as T;
}

export async function chooseWorkspace(): Promise<string | null> {
  if (DEMO) return "/Users/dev/projects/alva-agent";
  if (HTTP) {
    // Browsers cannot open a native directory picker. Accept an absolute path
    // from the operator instead; `run` forwards it verbatim to the engine.
    const value = window.prompt("Workspace path", "");
    return value && value.trim() ? value.trim() : null;
  }
  if (!hasTauri) {
    throw new Error("Workspace selection requires the ALVA Hub desktop runtime.");
  }
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function createSessionWorkspace(): Promise<WorkspaceBinding> {
  if (DEMO) {
    return {
      kind: "session",
      path: "/ALVA Hub/independent-sessions/demo-session",
    };
  }
  if (HTTP) {
    const value = window.prompt("Session workspace directory", "");
    if (value && value.trim()) {
      return { kind: "session", path: value.trim() };
    }
    throw new Error("A session workspace directory is required.");
  }
  if (!hasTauri) {
    throw new Error("Creating an independent workspace requires the ALVA Hub desktop runtime.");
  }
  return await invoke<WorkspaceBinding>("create_session_workspace");
}

export async function runHubPrompt(
  request: RunRequest,
  selectedTarget: EngineTarget | null,
  runToken: string | null,
): Promise<RunOutcome> {
  if (HTTP) {
    return await httpJson<RunOutcome>("/api/run", {
      prompt: request.prompt,
      workspace: request.workspace,
      engine_id: request.engine_id,
      product_session_id: request.product_session_id,
      selected_target: selectedTarget,
      permission_profile: request.permission_profile,
      run_token: runToken,
    });
  }
  if (!hasTauri) {
    throw new Error("Running an engine requires the ALVA Hub desktop runtime.");
  }
  return await invoke<RunOutcome>("run_hub_prompt", {
    request,
    selectedTarget,
    runToken,
  });
}

export async function switchHubEngine(
  productSessionId: string,
  target: EngineTarget,
  reason: string,
): Promise<SwitchOutcome> {
  if (HTTP) {
    return await httpJson<SwitchOutcome>(`/api/sessions/${encodeURIComponent(productSessionId)}/switch`, { target, reason });
  }
  if (!hasTauri) {
    throw new Error("Engine switching requires the ALVA Hub desktop runtime.");
  }
  return await invoke<SwitchOutcome>("switch_hub_engine", {
    request: { productSessionId, target, reason },
  });
}

export async function listModelProviders(workspace: string | null): Promise<ProviderProfile[]> {
  if (DEMO) return demoProviderProfiles();
  if (HTTP) return await httpJson<ProviderProfile[]>("/api/providers", undefined, "GET");
  if (!hasTauri) return [];
  return await invoke<ProviderProfile[]>("list_model_providers", { workspace });
}

export async function saveModelProvider(
  provider: SaveProviderProfile,
  workspace: string | null,
): Promise<ProviderProfile> {
  if (HTTP) return await httpJson<ProviderProfile>("/api/providers", provider);
  if (!hasTauri) throw new Error("Provider settings require the ALVA Hub desktop runtime.");
  return await invoke<ProviderProfile>("save_model_provider", { provider, workspace });
}

export async function createModelProvider(
  provider: CreateProviderProfile,
  workspace: string | null,
): Promise<ProviderProfile> {
  if (HTTP) return await httpJson<ProviderProfile>("/api/providers/create", provider);
  if (!hasTauri) throw new Error("Provider settings require the ALVA Hub desktop runtime.");
  return await invoke<ProviderProfile>("create_model_provider", { provider, workspace });
}

export async function deleteModelProvider(
  id: string,
  workspace: string | null,
): Promise<ProviderProfile> {
  if (HTTP) {
    return await httpJson<ProviderProfile>(
      `/api/providers/${encodeURIComponent(id)}`,
      undefined,
      "DELETE",
    );
  }
  if (!hasTauri) throw new Error("Provider settings require the ALVA Hub desktop runtime.");
  return await invoke<ProviderProfile>("delete_model_provider", { id, workspace });
}

export async function listModelRoutes(workspace: string | null): Promise<ModelRoute[]> {
  if (DEMO) return demoModelRoutes();
  if (HTTP) return await httpJson<ModelRoute[]>("/api/routes", undefined, "GET");
  if (!hasTauri) return [];
  return await invoke<ModelRoute[]>("list_model_routes", { workspace });
}

export async function getDefaultModelRoute(
  workspace: string | null,
): Promise<ModelRoute | null> {
  if (DEMO) return demoModelRoutes()[0];
  if (HTTP) return await httpJson<ModelRoute | null>("/api/default-route", undefined, "GET");
  if (!hasTauri) return null;
  return await invoke<ModelRoute | null>("get_default_model_route", { workspace });
}

export async function setDefaultModelRoute(
  target: EngineTarget,
  workspace: string | null,
): Promise<ModelRoute> {
  if (HTTP) return await httpJson<ModelRoute>("/api/default-route", target);
  if (!hasTauri) throw new Error("Default model settings require the ALVA Hub desktop runtime.");
  return await invoke<ModelRoute>("set_default_model_route", { target, workspace });
}

export async function discoverModelsForProvider(
  providerId: string,
  workspace: string | null,
): Promise<ProviderDiagnosticResult> {
  if (!hasTauri) throw new Error("Model discovery requires the ALVA Hub desktop runtime.");
  return await invoke<ProviderDiagnosticResult>("discover_models_for_provider", {
    providerId,
    workspace,
  });
}

export async function testModelRoute(
  target: EngineTarget,
  workspace: string | null,
): Promise<ProviderDiagnosticResult> {
  if (!hasTauri) throw new Error("Provider diagnostics require the ALVA Hub desktop runtime.");
  return await invoke<ProviderDiagnosticResult>("test_model_route", { target, workspace });
}

export async function listHubSessions(): Promise<HubSessionSummary[]> {
  if (DEMO) return demoSessions;
  if (HTTP) return await httpJson<HubSessionSummary[]>("/api/sessions", undefined, "GET");
  if (!hasTauri) return [];
  return await invoke<HubSessionSummary[]>("list_hub_sessions");
}

// ---- H04/H05: projects + read-only worktree discovery ----------------------

export async function listProjects(): Promise<ProjectSummary[]> {
  if (DEMO || !hasTauri) return [];
  return await invoke<ProjectSummary[]>("list_projects");
}

export async function createProject(
  workspacePath: string,
  displayName?: string,
): Promise<ProjectSummary> {
  if (!hasTauri) {
    throw new Error("Projects require the ALVA Hub desktop runtime.");
  }
  return await invoke<ProjectSummary>("create_project", {
    workspacePath,
    displayName: displayName ?? null,
  });
}

export async function renameProject(projectId: string, displayName: string): Promise<void> {
  if (!hasTauri) throw new Error("Projects require the ALVA Hub desktop runtime.");
  await invoke("rename_project", { projectId, displayName });
}

export async function deleteProject(projectId: string): Promise<void> {
  if (!hasTauri) throw new Error("Projects require the ALVA Hub desktop runtime.");
  await invoke("delete_project", { projectId });
}

export async function setSessionProject(
  productSessionId: string,
  projectId: string | null,
): Promise<void> {
  if (!hasTauri) throw new Error("Projects require the ALVA Hub desktop runtime.");
  await invoke("set_session_project", { productSessionId, projectId });
}

/** Inner state is a product STATE (not-git / missing directory), not a throw. */
export async function discoverProjectWorktrees(
  workspacePath: string,
): Promise<WorktreeInfo[] | WorktreeDiscoveryState> {
  if (!hasTauri) return { code: "git", detail: "worktree discovery requires the desktop runtime" };
  const result = await invoke<WorktreeInfo[] | WorktreeDiscoveryState>(
    "discover_project_worktrees",
    { workspacePath },
  );
  return result;
}

export async function getHubSession(
  productSessionId: string,
): Promise<HubSessionDetail | null> {
  if (DEMO) return demoDetail();
  if (HTTP) {
    const res = await fetch(`${HTTP_BASE}/api/sessions/${encodeURIComponent(productSessionId)}`);
    if (res.status === 404) return null;
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as HubSessionDetail;
  }
  if (!hasTauri) return null;
  return await invoke<HubSessionDetail | null>("get_hub_session", {
    productSessionId,
  });
}

export async function getHubInspection(
  productSessionId: string,
  nativeSessionId?: string | null,
): Promise<HubInspectionSnapshot> {
  if (DEMO) return demoInspection(nativeSessionId ?? undefined);
  if (HTTP) {
    const query = nativeSessionId
      ? `?native_session_id=${encodeURIComponent(nativeSessionId)}`
      : "";
    return await httpJson<HubInspectionSnapshot>(
      `/api/sessions/${encodeURIComponent(productSessionId)}/inspection${query}`,
      undefined,
      "GET",
    );
  }
  if (!hasTauri) {
    throw new Error("Agent Inspector requires the ALVA Hub desktop runtime.");
  }
  return await invoke<HubInspectionSnapshot>("get_hub_inspection", {
    productSessionId,
    nativeSessionId: nativeSessionId ?? null,
  });
}

export async function addSessionMemory(
  productSessionId: string,
  entry: string,
): Promise<string[]> {
  if (HTTP) {
    return await httpJson<string[]>(`/api/sessions/${encodeURIComponent(productSessionId)}/memory`, { entry });
  }
  if (!hasTauri) return [];
  return await invoke<string[]>("add_session_memory", {
    productSessionId,
    entry,
  });
}

export async function getGlobalFallbackPolicy(): Promise<FallbackPolicy> {
  if (DEMO) return demoFallbackPolicy();
  if (HTTP) return await httpJson<FallbackPolicy>("/api/fallback-policy", undefined, "GET");
  if (!hasTauri) {
    return { targets: [], updated_at: 0 };
  }
  return await invoke<FallbackPolicy>("get_global_fallback_policy");
}

export async function setGlobalFallbackPolicy(
  targets: EngineTarget[],
): Promise<FallbackPolicy> {
  if (HTTP) return await httpJson<FallbackPolicy>("/api/fallback-policy", { targets });
  if (!hasTauri) {
    throw new Error("Fallback settings require the ALVA Hub desktop runtime.");
  }
  return await invoke<FallbackPolicy>("set_global_fallback_policy", { targets });
}

export async function setSessionFallbackPolicy(
  productSessionId: string,
  targets: EngineTarget[],
): Promise<FallbackPolicy> {
  if (HTTP) return await httpJson<FallbackPolicy>(`/api/sessions/${encodeURIComponent(productSessionId)}/fallback-policy`, { targets });
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
  if (HTTP) return await httpJson<EngineEligibility[]>(`/api/permission-eligibility?profile=${encodeURIComponent(profile)}`, undefined, "GET");
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
): Promise<"delivered" | "expired"> {
  if (HTTP) {
    const body = await httpJson<{ outcome?: string }>(
      `/api/sessions/${encodeURIComponent(productSessionId)}/permission`,
      { request_id: requestId, allow, message: message ?? null },
    );
    if (body?.outcome === "expired") return "expired";
    return "delivered";
  }
  if (!hasTauri) {
    throw new Error(
      "Answering a permission request requires the ALVA Hub desktop runtime.",
    );
  }
  return await invoke<"delivered" | "expired">("respond_hub_permission", {
    request: { productSessionId, requestId, allow, message: message ?? null },
  });
}

export async function setTranscriptSharing(
  productSessionId: string,
  enabled: boolean,
): Promise<void> {
  if (HTTP) {
    await httpJson(`/api/sessions/${encodeURIComponent(productSessionId)}/transcript`, { enabled });
    return;
  }
  if (!hasTauri) {
    throw new Error("Transcript settings require the ALVA Hub desktop runtime.");
  }
  await invoke("set_transcript_sharing", { productSessionId, enabled });
}

export async function stopHubAttempt(productSessionId: string): Promise<void> {
  if (HTTP) {
    await httpJson(`/api/sessions/${encodeURIComponent(productSessionId)}/stop`, {});
    return;
  }
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
  if (DEMO)
    return [
      {
        group_key: "g-demo-bash",
        tool_name: "bash",
        members: [
          {
            scope: "tr-demo-1",
            product_session_id: "ps-worker-1",
            request_id: "rq-demo-1",
            requester: "worker-demo-1",
            tool_name: "bash",
            resource: "src/queue.rs",
          },
          {
            scope: "tr-demo-1",
            product_session_id: "ps-worker-2",
            request_id: "rq-demo-2",
            requester: "worker-demo-2",
            tool_name: "bash",
            resource: "docs/",
          },
        ],
      },
    ];
  if (HTTP) return await httpJson<PermissionGroup[]>("/api/permission-groups", undefined, "GET");
  if (!hasTauri) return [];
  return await invoke<PermissionGroup[]>("get_permission_groups");
}

export async function respondPermissionGroup(
  groupKey: string,
  allow: boolean,
  requestIds?: string[],
): Promise<number> {
  if (HTTP) {
    const res = await httpJson<{ answered: number }>("/api/permission/respond", { group_key: groupKey, allow, request_ids: requestIds });
    return res.answered;
  }
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
  if (HTTP) {
    const q = workspace ? `?workspace=${encodeURIComponent(workspace)}` : "";
    return await httpJson<EngineReadiness[]>(`/api/readiness${q}`, undefined, "GET");
  }
  if (!hasTauri) return [];
  return await invoke<EngineReadiness[]>("get_engine_readiness", { workspace });
}

export async function listenTimeline(
  onEvent: (event: TimelineEvent) => void,
): Promise<UnlistenFn> {
  if (HTTP) {
    const es = new EventSource(`${HTTP_BASE}/api/events`);
    const handler = (msg: MessageEvent<string>) => {
      try {
        const wire = JSON.parse(msg.data) as { status: string };
        // The server emits a flat TimelineEvent under the `hub_timeline`
        // event name; parse it back to the product DTO shape.
        onEvent(wire as unknown as TimelineEvent);
      } catch {
        /* ignore malformed frame */
      }
    };
    es.addEventListener("hub_timeline", handler as EventListener);
    return () => {
      es.removeEventListener("hub_timeline", handler as EventListener);
      es.close();
    };
  }
  if (!hasTauri) {
    return () => {};
  }
  return await listen<TimelineEvent>("hub_timeline", ({ payload }) => {
    onEvent(payload);
  });
}
