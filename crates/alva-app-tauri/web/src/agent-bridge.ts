import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors `alva_kernel_core::event::AgentEvent` (tag = "type").
export type AgentEvent =
  | { type: "AgentStart" }
  | { type: "AgentEnd"; error: string | null }
  | { type: "TurnStart" }
  | { type: "TurnEnd" }
  | { type: "MessageStart"; message: unknown }
  | { type: "MessageUpdate"; message: unknown; delta: unknown }
  | { type: "MessageEnd"; message: unknown }
  | { type: "MessageError"; message: unknown; error: string }
  | { type: "ToolExecutionStart"; tool_call: unknown }
  | { type: "ToolExecutionUpdate"; tool_call_id: string; event: unknown }
  | { type: "ToolExecutionEnd"; tool_call: unknown; result: unknown }
  | { type: "RunChannelClosed" }
  | { type: string; [k: string]: unknown };

// Envelope the Rust side wraps each AgentEvent in before emitting.
export interface AgentEventEnvelope {
  session_id: string;
  event: AgentEvent;
}

export interface SendMessageRequest {
  provider: string;
  model: string;
  api_key?: string | null;
  base_url?: string | null;
  system_prompt?: string | null;
  workspace?: string | null;
  session_id?: string | null;
  skill_names?: string[] | null;
  /** Manual tool allow-list. `null` / absent = auto mode (all tools). */
  tool_names?: string[] | null;
  text: string;
}

export interface PluginToolInfo {
  name: string;
  description: string;
}

export interface PluginInfo {
  name: string;
  description: string;
  category: "tools" | "system" | "middleware" | string;
  default_enabled: boolean;
  enabled: boolean;
  tools: PluginToolInfo[];
}

export async function setPluginEnabled(
  name: string,
  enabled: boolean,
): Promise<void> {
  await invoke<void>("set_plugin_enabled", { name, enabled });
}

export interface SkillInfo {
  name: string;
  description: string;
  kind: string;
  enabled: boolean;
  source_dir: string;
}

export interface SkillSourceInfo {
  path: string;
  exists: boolean;
  label: string;
}

export interface McpServerInfo {
  id: string;
  name: string;
  kind: string;
  command_or_url: string;
  enabled: boolean;
}

export interface ModelCapabilities {
  supports_tools: boolean | null;
  is_reasoning: boolean | null;
  context_window: number | null;
}

export interface RemoteModelInfo {
  id: string;
  display_name: string | null;
  owned_by: string | null;
  capabilities: ModelCapabilities;
}

export interface ConnectionTestResult {
  ok: boolean;
  latency_ms: number;
  status: number | null;
  message: string | null;
  model_count: number;
}

export interface RemoteModelsRequest {
  provider: string;
  api_key: string;
  base_url?: string | null;
}

// --- Session inspector / projection ---------------------------------------
// Mirrors `crates/alva-app-tauri/src/session_projection.rs`. Loose-typed in
// places where the underlying enum is open-ended (Message content blocks,
// arguments, results) — we render those as JSON.

export interface RunRecord {
  config_snapshot: ConfigSnapshot;
  turns: TurnRecord[];
  total_duration_ms: number;
  total_input_tokens: number;
  total_output_tokens: number;
}

export interface ConfigSnapshot {
  system_prompt: string;
  model_id: string;
  tool_names: string[];
  tool_definitions: unknown[];
  skill_names: string[];
  max_iterations: number;
  extension_names: string[];
  middleware_names: string[];
}

export interface TurnRecord {
  turn_number: number;
  llm_call: LlmCallRecord;
  tool_calls: ToolCallRecord[];
  duration_ms: number;
}

export interface LlmCallRecord {
  messages_sent: unknown[];
  messages_sent_count: number;
  response: unknown | null;
  input_tokens: number;
  output_tokens: number;
  duration_ms: number;
  /** "end_turn" | "tool_use" | "max_tokens" | "error" */
  stop_reason: string;
  error_message: string | null;
  middleware_hooks: HookRecord[];
}

export interface ToolCallRecord {
  tool_call: { id: string; name: string; arguments: unknown };
  result: unknown | null;
  is_error: boolean;
  duration_ms: number;
  middleware_hooks: HookRecord[];
  sub_run?: RunRecord | null;
}

export interface HookRecord {
  middleware_name: string;
  hook: string;
  duration_ms: number;
  outcome: string;
}

export interface SessionInfo {
  id: string;
  title: string;
  created_at_ms: number;
  updated_at_ms: number;
  /** Per-session sandbox folder. Auto-created by the Rust side on
   *  create_session; user may override via the folder picker BEFORE the
   *  first user message. */
  workspace_path: string | null;
}

/** Rich chat bubble — mirrors Rust `ChatEntry` (serde tag = "type"). */
export type ChatEntry =
  | { type: "user"; text: string }
  | { type: "assistant"; text: string }
  | { type: "system"; text: string }
  | { type: "thinking"; text: string }
  | {
      type: "tool_call";
      id: string;
      name: string;
      arguments: unknown;
      result: string | null;
      is_error: boolean;
    }
  | { type: "error"; text: string };

// --- send / cancel ---------------------------------------------------------

export async function sendMessage(req: SendMessageRequest): Promise<string> {
  return await invoke<string>("send_message", { request: req });
}

export async function cancelRun(): Promise<void> {
  await invoke("cancel_run");
}

// --- session management ----------------------------------------------------

export async function listSessions(): Promise<SessionInfo[]> {
  return await invoke<SessionInfo[]>("list_sessions");
}

export async function createSession(): Promise<SessionInfo> {
  return await invoke<SessionInfo>("create_session");
}

export async function switchSession(id: string): Promise<ChatEntry[]> {
  return await invoke<ChatEntry[]>("switch_session", { id });
}

export async function deleteSession(id: string): Promise<void> {
  await invoke("delete_session", { id });
}

export async function setSessionWorkspace(id: string, path: string): Promise<void> {
  await invoke("set_session_workspace", { id, path });
}

export async function openSessionWorkspace(id: string): Promise<void> {
  await invoke("open_session_workspace", { id });
}

// --- skills & MCP ---------------------------------------------------------

export async function listSkillSources(): Promise<SkillSourceInfo[]> {
  return await invoke<SkillSourceInfo[]>("list_skill_sources");
}

export async function scanSkills(path: string): Promise<SkillInfo[]> {
  return await invoke<SkillInfo[]>("scan_skills", { path });
}

export async function listAllSkills(): Promise<SkillInfo[]> {
  return await invoke<SkillInfo[]>("list_all_skills");
}

export async function listMcpServers(): Promise<McpServerInfo[]> {
  return await invoke<McpServerInfo[]>("list_mcp_servers");
}

export async function listPlugins(): Promise<PluginInfo[]> {
  return await invoke<PluginInfo[]>("list_plugins");
}

export async function listRemoteModels(
  request: RemoteModelsRequest,
): Promise<RemoteModelInfo[]> {
  return await invoke<RemoteModelInfo[]>("list_remote_models", { request });
}

export async function testProviderConnection(
  request: RemoteModelsRequest,
): Promise<ConnectionTestResult> {
  return await invoke<ConnectionTestResult>("test_provider_connection", {
    request,
  });
}

export async function getSessionRecord(id: string): Promise<RunRecord> {
  return await invoke<RunRecord>("get_session_record", { id });
}

/** Loose-typed raw SessionEvent log for the Raw Events inspector tab. */
export async function listSessionEvents(id: string): Promise<unknown[]> {
  return await invoke<unknown[]>("list_session_events", { id });
}

export async function openInspectorWindow(): Promise<void> {
  await invoke("open_inspector_window");
}

// --- event stream ----------------------------------------------------------

export function subscribeAgentEvents(
  handler: (envelope: AgentEventEnvelope) => void,
): Promise<UnlistenFn> {
  return listen<AgentEventEnvelope>("agent_event", (e) => handler(e.payload));
}

// --- message projection ----------------------------------------------------

export function extractMessageText(message: unknown): string {
  if (message == null) return "";
  if (typeof message === "string") return message;
  const m = message as Record<string, unknown>;

  const inner = (m.Standard ?? m.FollowUp ?? m.Steering ?? m) as
    | Record<string, unknown>
    | undefined;
  if (!inner) return JSON.stringify(message);

  const content = inner.content ?? inner.text;
  if (typeof content === "string") return content;

  if (Array.isArray(content)) {
    return content
      .map((block) => {
        if (typeof block === "string") return block;
        const b = block as Record<string, unknown>;
        if (typeof b.text === "string") return b.text;
        if (b.Text && typeof b.Text === "object") {
          const t = (b.Text as Record<string, unknown>).text;
          if (typeof t === "string") return t;
        }
        return "";
      })
      .join("");
  }

  return JSON.stringify(message);
}
