// INPUT:  Tauri invoke/listen APIs, RunRecord/ConfigSnapshot JSON from Rust
// OUTPUT: Agent bridge helpers, event subscriptions, frontend data contracts
// POS:    Frontend IPC boundary for chat, session, provider, and inspector data.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ───────────────────────────────────────────────────────────────────────────
// Browser-mode fallback
// ───────────────────────────────────────────────────────────────────────────
// `@tauri-apps/api/core` reaches `window.__TAURI_INTERNALS__.transformCallback`
// in `invoke()` — when the page is loaded in plain Chrome (not the Tauri
// webview), that global is missing and every call throws an unhandled
// `Cannot read properties of undefined (reading 'transformCallback')` /
// `(reading 'invoke')` error. We detect the runtime once and route IPC
// through safe shims so dev-server-only mode (used by the
// autonomous-ui-test skill) renders cleanly with empty defaults instead
// of spamming the console with stack traces.

export const HAS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

if (!HAS_TAURI && typeof console !== "undefined") {
  // One-time hint, not per-call. Real bugs stay visible.
  console.info(
    "[agent-bridge] Tauri runtime not detected — running in browser-only " +
      "mode. IPC commands return empty/no-op defaults. Use `cargo tauri dev` " +
      "for the full desktop experience."
  );
}

/** Run a Tauri command, or return `fallback` when the runtime is absent. */
async function safeInvokeOr<T>(
  cmd: string,
  args: Record<string, unknown> | undefined,
  fallback: T
): Promise<T> {
  if (!HAS_TAURI) return fallback;
  return await invoke<T>(cmd, args);
}

/** Run a Tauri command. In browser mode throws a clear, single-line error
 * the caller can catch and surface — used for mutations/queries that
 * have no sensible empty default (e.g. `send_message`). */
async function safeInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!HAS_TAURI) {
    throw new Error(
      `Tauri command "${cmd}" requires the desktop runtime (run \`cargo tauri dev\`).`
    );
  }
  return await invoke<T>(cmd, args);
}

/** Subscribe to a Tauri event, or no-op when the runtime is absent. */
function safeListen<T>(
  evt: string,
  cb: (e: { payload: T }) => void
): Promise<UnlistenFn> {
  if (!HAS_TAURI) {
    return Promise.resolve((() => {}) as UnlistenFn);
  }
  return listen<T>(evt, cb);
}

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

/** Per-turn reasoning effort. Lowercase strings match the Rust
 * `ReasoningEffort::parse` contract. Unknown values (or `null`) clear
 * any override — provider default behavior. */
export type ReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh";

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
  /** Per-turn reasoning effort override. Applies to every LLM call in
   * this turn — Anthropic requires one mode per turn, so mid-turn
   * switching is not supported. */
  reasoning_effort?: ReasoningEffort | null;
  /** Resolved per-model output cap — Home computes via
   * `useResolvedModelCapabilities`. `null` lets the backend apply its
   * `32_000` fallback. */
  max_output_tokens?: number | null;
  /** Vendor-specific JSON merged verbatim into the LLM request body
   * (last-write-wins). Comes from the per-model override panel, fed
   * straight through to `ModelConfig::extra_body`. */
  provider_options?: Record<string, unknown> | null;
  /** When `true`, the backend skips all tool injection (request goes
   * out without a `tools` field). Resolve from
   * `modelCaps.supports_tools === false`. */
  disable_tools?: boolean | null;
  text: string;
}

export interface PluginToolInfo {
  name: string;
  description: string;
}

export interface PluginInfo {
  /** Stable component id; the key passed back to `setPluginEnabled`. */
  name: string;
  /** Human-friendly display name (from the shared COMPONENTS catalog). */
  label: string;
  description: string;
  /** Component category from COMPONENTS:
   * "tools" | "safety" | "context" | "collab" | "infra" | "ext". */
  category: string;
  default_enabled: boolean;
  enabled: boolean;
  tools: PluginToolInfo[];
}

export async function setPluginEnabled(
  name: string,
  enabled: boolean,
): Promise<void> {
  await safeInvokeOr<void>("set_plugin_enabled", { name, enabled }, undefined);
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
  /** Per-model output token cap (separate from context_window — input
   * space is much larger than what the model emits in one response).
   * Pulled from OpenRouter's `top_provider.max_completion_tokens`;
   * other providers leave this `null` and the user can override in
   * Settings. The backend falls back to `32_000` when not provided. */
  max_output_tokens: number | null;
  /** Vision-input models (accepts image content blocks). */
  vision?: boolean | null;
  /** Image-output models (returns `image` content blocks). Different
   * from `vision` — most chat models do neither, GPT-4o does both,
   * DALL-E does only output. */
  image_output?: boolean | null;
  /** Embedding-only models. Filters them out of the chat picker. */
  embedding?: boolean | null;
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
  message: string | null;
  /** The model id that was tested — echoed back so the UI can confirm which one ran. */
  model: string | null;
  /** First ~200 chars of the assistant reply on success. Null on failure. */
  sample_response: string | null;
  /** Usage counts from the test request, if the provider populated them. */
  input_tokens: number | null;
  output_tokens: number | null;
}

export interface RemoteModelsRequest {
  provider: string;
  api_key: string;
  base_url?: string | null;
}

/** Connection test sends one inference ping through the configured
 * provider + model, so it needs the model id in addition to the auth.
 */
export interface ConnectionTestRequest {
  provider: string;
  api_key: string;
  model: string;
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
  /** User-submitted prompts in the order they arrived. Each entry's
   * `before_turn_number` is the 1-indexed turn the message kicked off
   * — render the message block right before that turn in the timeline.
   * Optional for backwards compatibility with old projections. */
  user_messages?: UserMessageRecord[];
}

export interface UserMessageRecord {
  before_turn_number: number;
  text: string;
  timestamp_ms: number;
}

export interface ConfigSnapshot {
  /** Layered system prompt — every entry except the last is rendered
   * with `cache_control: ephemeral` (Anthropic). Old snapshots may
   * deserialize into a 1-element array (legacy single-string shape). */
  system_prompt: string[];
  model_id: string;
  tool_names: string[];
  tool_definitions: unknown[];
  skill_names: string[];
  max_iterations: number;
  plugin_names: string[];
  plugin_assembly: PluginAssemblySnapshot[];
  middleware_names: string[];
  direct_middleware_names?: string[];
}

export interface PluginAssemblySnapshot {
  name: string;
  description: string;
  registered_tool_names: string[];
  finalized_tool_names: string[];
  middleware_names: string[];
  phase_contribution_names: string[];
  command_names: string[];
  system_prompt_fragments: number;
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

  // Prompt-cache observability (Anthropic only — null for other providers).
  /** Tokens written FRESH into prompt cache (you pay for these). */
  cache_creation_input_tokens?: number | null;
  /** Tokens reused FROM cache (~90% discount on Anthropic). */
  cache_read_input_tokens?: number | null;

  // Per-turn config knobs (P2 markers from llm_call_start).
  /** True when this call ran without tools (request omitted `tools`). */
  disable_tools?: boolean;
  /** Cache-segment count of the system prompt this call. >1 = stable+dynamic split. */
  system_prompt_segments?: number;
  /** Number of tools actually sent (0 if disable_tools or no tools registered). */
  tools_count_sent?: number;
  /** Whether vendor-specific JSON pass-through (extra_body) was non-empty. */
  provider_options_applied?: boolean;
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
  return await safeInvoke<string>("send_message", { request: req });
}

export async function cancelRun(): Promise<void> {
  await safeInvokeOr<void>("cancel_run", undefined, undefined);
}

// --- approval flow ---------------------------------------------------------

/** Mirror of `crates/alva-app-tauri/src/agent.rs::PendingApproval`. */
export interface PendingApproval {
  request_id: string;
  tool_name: string;
  arguments: unknown;
}

/** 4 user choices the inline approval bubble offers. The string values
 * match `parse_decision` on the Rust side — keep in sync. */
export type ApprovalDecision =
  | "allow_once"
  | "allow_always"
  | "reject_once"
  | "reject_always";

/** Resolve a pending approval. Idempotent — answering twice is fine. */
export async function respondApproval(
  requestId: string,
  decision: ApprovalDecision,
): Promise<void> {
  await safeInvokeOr<void>("respond_approval", { requestId, decision }, undefined);
}

/** Snapshot of currently-pending approvals. Used to rehydrate UI on
 * mount in case events were emitted before the listener attached. */
export async function listPendingApprovals(): Promise<PendingApproval[]> {
  return await safeInvokeOr<PendingApproval[]>("list_pending_approvals", undefined, []);
}

/** Subscribe to new approval requests pushed from the backend. */
export async function listenApprovalRequest(
  cb: (req: PendingApproval) => void,
): Promise<UnlistenFn> {
  return await safeListen<PendingApproval>("approval_request", (e) => cb(e.payload));
}

/** Subscribe to approval-resolved notifications (so other windows /
 * stale views can drop the bubble from their list). */
export async function listenApprovalResolved(
  cb: (requestId: string) => void,
): Promise<UnlistenFn> {
  return await safeListen<{ request_id: string }>("approval_resolved", (e) =>
    cb(e.payload.request_id),
  );
}

/** Subscribe to "all pending approvals cleared" — sent when the agent
 * is rebuilt and the previous request_ids are invalid. */
export async function listenApprovalsCleared(
  cb: () => void,
): Promise<UnlistenFn> {
  return await safeListen<null>("approvals_cleared", () => cb());
}

// --- session management ----------------------------------------------------

export async function listSessions(): Promise<SessionInfo[]> {
  return await safeInvokeOr<SessionInfo[]>("list_sessions", undefined, []);
}

export async function createSession(): Promise<SessionInfo> {
  return await safeInvoke<SessionInfo>("create_session");
}

export async function switchSession(id: string): Promise<ChatEntry[]> {
  return await safeInvokeOr<ChatEntry[]>("switch_session", { id }, []);
}

export async function deleteSession(id: string): Promise<void> {
  await safeInvokeOr<void>("delete_session", { id }, undefined);
}

export async function setSessionWorkspace(id: string, path: string): Promise<void> {
  await safeInvokeOr<void>("set_session_workspace", { id, path }, undefined);
}

export async function openSessionWorkspace(id: string): Promise<void> {
  await safeInvokeOr<void>("open_session_workspace", { id }, undefined);
}

// --- skills & MCP ---------------------------------------------------------

export async function listSkillSources(): Promise<SkillSourceInfo[]> {
  return await safeInvokeOr<SkillSourceInfo[]>("list_skill_sources", undefined, []);
}

export async function scanSkills(path: string): Promise<SkillInfo[]> {
  return await safeInvokeOr<SkillInfo[]>("scan_skills", { path }, []);
}

export async function listAllSkills(): Promise<SkillInfo[]> {
  return await safeInvokeOr<SkillInfo[]>("list_all_skills", undefined, []);
}

export async function listMcpServers(): Promise<McpServerInfo[]> {
  return await safeInvokeOr<McpServerInfo[]>("list_mcp_servers", undefined, []);
}

export async function listPlugins(): Promise<PluginInfo[]> {
  return await safeInvokeOr<PluginInfo[]>("list_plugins", undefined, []);
}

export async function listRemoteModels(
  request: RemoteModelsRequest,
): Promise<RemoteModelInfo[]> {
  return await safeInvokeOr<RemoteModelInfo[]>("list_remote_models", { request }, []);
}

export async function testProviderConnection(
  request: ConnectionTestRequest,
): Promise<ConnectionTestResult> {
  return await safeInvoke<ConnectionTestResult>("test_provider_connection", {
    request,
  });
}

export async function getSessionRecord(id: string): Promise<RunRecord> {
  return await safeInvoke<RunRecord>("get_session_record", { id });
}

// --- protocol gateway --------------------------------------------------------

export async function startGateway(
  cfg: { provider: string; model: string; api_key: string; base_url?: string | null },
  port: number,
): Promise<string> {
  return await safeInvoke<string>("start_gateway", {
    req: {
      provider: cfg.provider,
      model: cfg.model,
      api_key: cfg.api_key,
      base_url: cfg.base_url || null,
    },
    port,
  });
}

export async function stopGateway(): Promise<void> {
  await safeInvoke<void>("stop_gateway", {});
}

/** Loose-typed raw SessionEvent log for the Raw Events inspector tab. */
export async function listSessionEvents(id: string): Promise<unknown[]> {
  return await safeInvokeOr<unknown[]>("list_session_events", { id }, []);
}

export async function openInspectorWindow(): Promise<void> {
  await safeInvokeOr<void>("open_inspector_window", undefined, undefined);
}

// --- event stream ----------------------------------------------------------

export function subscribeAgentEvents(
  handler: (envelope: AgentEventEnvelope) => void,
): Promise<UnlistenFn> {
  return safeListen<AgentEventEnvelope>("agent_event", (e) => handler(e.payload));
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
