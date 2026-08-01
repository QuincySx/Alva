// INPUT:  @tauri-apps/api, @tauri-apps/plugin-dialog
// OUTPUT: workspace/run/timeline plus persisted Session/Turn/Attempt IPC DTOs
// POS:    Typed IPC boundary between React and Hub Desktop product commands.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

export type TimelineStatus = "running" | "completed" | "failed" | "interrupted";

export interface TimelineEvent {
  status: TimelineStatus;
  engine_id: string;
  product_session_id: string;
  message: string | null;
}

export interface RunRequest {
  prompt: string;
  workspace: string;
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

export interface HubSessionDetail {
  session: HubSessionSummary;
  turns: StoredTurn[];
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

export async function runAlvaPrompt(
  request: RunRequest,
): Promise<RunOutcome> {
  if (!hasTauri) {
    throw new Error("Running Alva requires the ALVA Hub desktop runtime.");
  }
  return await invoke<RunOutcome>("run_alva_prompt", { request });
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
