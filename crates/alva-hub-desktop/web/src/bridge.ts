// INPUT:  @tauri-apps/api, @tauri-apps/plugin-dialog
// OUTPUT: chooseWorkspace(), runAlvaPrompt(), listenTimeline(), shared DTOs
// POS:    Typed IPC boundary between React and Hub Desktop Rust commands.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

export type TimelineStatus = "running" | "completed" | "failed";

export interface TimelineEvent {
  status: TimelineStatus;
  engine_id: string;
  product_session_id: string;
  message: string | null;
}

export interface RunRequest {
  prompt: string;
  workspace: string;
}

export interface RunOutcome {
  product_session_id: string;
  result: string | null;
  failed: boolean;
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
