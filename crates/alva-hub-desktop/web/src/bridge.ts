// INPUT:  @tauri-apps/api, @tauri-apps/plugin-dialog
// OUTPUT: workspace/run/switch/policy/readiness/timeline plus persisted Hub IPC DTOs
// POS:    Typed multi-engine authorization boundary between React and Hub Desktop commands.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

export type TimelineStatus =
  | "running"
  | "completed"
  | "failed"
  | "interrupted"
  | "queued"
  | "switched"
  | "cancelled"
  | "fallback"
  | "exhausted";

export interface TimelineEvent {
  status: TimelineStatus;
  engine_id: string;
  product_session_id: string;
  message: string | null;
  created_at?: number;
}

export interface RunRequest {
  prompt: string;
  workspace: string;
  engine_id: string;
  product_session_id: string | null;
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
  created_at: number;
  used_at: number | null;
}

export interface HubSessionDetail {
  session: HubSessionSummary;
  turns: StoredTurn[];
  branches: StoredBranch[];
  handoffs: HandoffPackage[];
  timeline_events: StoredTimelineEvent[];
  pending_switch: PendingSwitch | null;
  fallback_policy: FallbackPolicy;
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

const hasTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

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
  if (!hasTauri) return [];
  return await invoke<HubSessionSummary[]>("list_hub_sessions");
}

export async function getHubSession(
  productSessionId: string,
): Promise<HubSessionDetail | null> {
  if (!hasTauri) return null;
  return await invoke<HubSessionDetail | null>("get_hub_session", {
    productSessionId,
  });
}

export async function getGlobalFallbackPolicy(): Promise<FallbackPolicy> {
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

export async function stopHubAttempt(productSessionId: string): Promise<void> {
  if (!hasTauri) {
    throw new Error("Stopping an attempt requires the ALVA Hub desktop runtime.");
  }
  await invoke("stop_hub_attempt", { productSessionId });
}

export async function getEngineReadiness(
  workspace: string | null,
): Promise<EngineReadiness[]> {
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
