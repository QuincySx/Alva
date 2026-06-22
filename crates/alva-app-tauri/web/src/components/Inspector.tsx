// INPUT:  RunRecord/ConfigSnapshot data, React state, lucide-react icons
// OUTPUT: Inspector React component and record detail subcomponents
// POS:    Visual session inspector for prompts, plugins, tools, turns, and events.
import {
  AlertCircle,
  ArrowLeft,
  ChevronDown,
  RefreshCcw,
} from "lucide-react";
import {
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  getSessionRecord,
  type ConfigSnapshot,
  type LlmCallRecord,
  type RunRecord,
  type ToolCallRecord,
  type TurnRecord,
  type UserMessageRecord,
} from "../agent-bridge";

interface InspectorProps {
  sessionId: string | null;
  /** Bumped by the parent (Home) on AgentEnd to trigger a refetch. */
  refreshNonce?: number;
}

/**
 * Two-pane visual inspector for a session's projected `RunRecord`.
 *   - Left column: vertical timeline of overview / per-turn blocks.
 *     Every block is clickable; clicking populates the right column.
 *   - Right column: detail panel showing whatever the selected block wants
 *     to show (config, JSON dumps, error text, etc.).
 *
 * Sub-agent tool calls (`tool_call.name === "agent"`) with a non-empty
 * `sub_run` open a stacked full-screen modal that **reuses the same body
 * recursively** — exactly how the original eval inspector handled nesting.
 * The modal stack lets you drill grandchildren and ESC pops one level.
 *
 * Ported in spirit from `alva-app-eval/static/inspector.js`.
 */
export function Inspector({ sessionId, refreshNonce = 0 }: InspectorProps) {
  const [record, setRecord] = useState<RunRecord | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [modalStack, setModalStack] = useState<SubAgentEntry[]>([]);

  const refresh = async () => {
    if (!sessionId) {
      setRecord(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const r = await getSessionRecord(sessionId);
      setRecord(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, refreshNonce]);

  // ESC closes the topmost open sub-agent modal, NOT the parent inspector.
  useEffect(() => {
    if (modalStack.length === 0) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setModalStack((prev) => prev.slice(0, -1));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [modalStack.length]);

  const openSubAgent = (tc: ToolCallRecord) => {
    if (!tc.sub_run || tc.sub_run.turns.length === 0) return;
    const args = (tc.tool_call.arguments ?? {}) as Record<string, unknown>;
    const role = (args.role as string) ?? "sub-agent";
    const task =
      (args.task as string) ?? (args.description as string) ?? "";
    setModalStack((prev) => [...prev, { record: tc.sub_run!, role, task }]);
  };
  const popModal = () => setModalStack((prev) => prev.slice(0, -1));

  if (!sessionId) {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500 text-xs">
        没有当前会话
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-neutral-950 relative">
      <header className="shrink-0 flex items-center justify-between px-3 py-2 border-b border-neutral-800 text-[10px] uppercase tracking-wider text-neutral-500 gap-2">
        <span className="shrink-0">Inspector · 会话投影</span>
        <SessionIdBadge sessionId={sessionId} />
        <button
          type="button"
          onClick={refresh}
          disabled={loading}
          className="inline-flex items-center gap-1 text-neutral-400 hover:text-white disabled:opacity-50 shrink-0"
        >
          <RefreshCcw size={11} className={loading ? "animate-spin" : ""} />
          {loading ? "加载中" : "刷新"}
        </button>
      </header>

      {error && (
        <div className="px-3 py-2 text-[11px] text-red-300 bg-red-950/40 border-b border-red-900/60 flex items-start gap-1">
          <AlertCircle size={12} className="mt-0.5 shrink-0" />
          <span className="font-mono break-all">{error}</span>
        </div>
      )}

      {!record && !loading && !error && (
        <div className="flex-1 flex items-center justify-center text-xs text-neutral-500">
          还没有事件
        </div>
      )}

      {record && (
        <div className="flex-1 min-h-0">
          <InspectorBody record={record} onOpenSubAgent={openSubAgent} />
        </div>
      )}

      {/* Stacked sub-agent modals — each renders the same body on its
          own RunRecord. Nested click pushes another modal on top. */}
      {modalStack.map((entry, i) => (
        <div
          key={i}
          className="fixed inset-0 bg-neutral-950 flex flex-col border-l border-neutral-800"
          style={{ zIndex: 1000 + i * 10 }}
        >
          <header className="shrink-0 flex items-center gap-3 border-b border-neutral-800 px-3 py-2">
            <button
              type="button"
              onClick={popModal}
              className="inline-flex items-center gap-1 rounded bg-neutral-900 hover:bg-neutral-800 px-2 py-1 text-xs"
              title="返回 (ESC)"
            >
              <ArrowLeft size={12} />
              返回
            </button>
            <span className="rounded bg-purple-950/60 border border-purple-900 text-purple-300 text-[10px] px-2 py-0.5 font-semibold">
              SUB-AGENT · L{i + 1}
            </span>
            <strong className="text-purple-300 text-sm">{entry.role}</strong>
            <span className="text-neutral-500 text-xs truncate flex-1">
              {entry.task}
            </span>
          </header>
          <div className="flex-1 min-h-0">
            <InspectorBody record={entry.record} onOpenSubAgent={openSubAgent} />
          </div>
        </div>
      ))}
    </div>
  );
}

interface SubAgentEntry {
  record: RunRecord;
  role: string;
  task: string;
}

// ---------------------------------------------------------------------------
// Body: 2-column timeline + detail
// ---------------------------------------------------------------------------

function InspectorBody({
  record,
  onOpenSubAgent,
}: {
  record: RunRecord;
  onOpenSubAgent: (tc: ToolCallRecord) => void;
}) {
  // Each block has a stable string key so we can highlight the selected one.
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [detail, setDetail] = useState<ReactNode | null>(null);

  const select = (key: string, content: ReactNode) => {
    setSelectedKey(key);
    setDetail(content);
  };

  // Reset selection when the record identity changes.
  useEffect(() => {
    setSelectedKey(null);
    setDetail(null);
  }, [record]);

  return (
    <div className="flex h-full min-h-0">
      <div className="w-[280px] shrink-0 overflow-auto border-r border-neutral-800 px-3 py-3">
        <Timeline
          record={record}
          selectedKey={selectedKey}
          onSelect={select}
          onOpenSubAgent={onOpenSubAgent}
        />
      </div>
      <div className="flex-1 min-w-0 overflow-auto p-4 text-xs">
        {detail ?? (
          <div className="text-neutral-500">点击左侧任何 block 查看详情</div>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Timeline (left column)
// ---------------------------------------------------------------------------

function Timeline({
  record,
  selectedKey,
  onSelect,
  onOpenSubAgent,
}: {
  record: RunRecord;
  selectedKey: string | null;
  onSelect: (key: string, content: ReactNode) => void;
  onOpenSubAgent: (tc: ToolCallRecord) => void;
}) {
  const totalTokens = record.total_input_tokens + record.total_output_tokens;
  const totalTools = record.turns.reduce(
    (sum, t) => sum + t.tool_calls.length,
    0,
  );

  let prevInputTokens = 0;

  return (
    <div className="space-y-2">
      {/* Overview block */}
      <Block
        keyId="overview"
        selectedKey={selectedKey}
        accent="cyan"
        onClick={() =>
          onSelect(
            "overview",
            <OverviewDetail record={record} totalTokens={totalTokens} totalTools={totalTools} />,
          )
        }
      >
        <div className="font-medium text-[12px]">Run Overview</div>
        <div className="text-[10px] text-neutral-500 mt-0.5 truncate">
          {record.config_snapshot.model_id || "(no model)"} · {record.turns.length}T ·{" "}
          {totalTokens.toLocaleString()} tok · {fmtMs(record.total_duration_ms)}
        </div>
      </Block>

      {/* Per-turn blocks, with user prompts interleaved before the turn
          they kicked off. A multi-iteration run will show its prompt
          once (before turn 1); a "继续/重试" appearing between turns
          shows up between them. */}
      {record.turns.map((turn) => {
        const promptsBefore = (record.user_messages ?? []).filter(
          (u) => u.before_turn_number === turn.turn_number,
        );
        const block = (
          <TurnTimeline
            key={turn.turn_number}
            turn={turn}
            prevInputTokens={prevInputTokens}
            selectedKey={selectedKey}
            onSelect={onSelect}
            onOpenSubAgent={onOpenSubAgent}
          />
        );
        prevInputTokens = turn.llm_call.input_tokens;
        return (
          <div key={`group-${turn.turn_number}`} className="space-y-2">
            {promptsBefore.map((u, idx) => (
              <UserPromptBlock
                key={`u-${turn.turn_number}-${idx}`}
                msg={u}
                index={record.user_messages
                  ?.findIndex((x) => x === u) ?? idx}
                selectedKey={selectedKey}
                onSelect={onSelect}
              />
            ))}
            {block}
          </div>
        );
      })}

      {/* User prompts that have no following turn yet (cancelled before
          any iteration started, or run still in flight). These attach
          to one past the last turn. */}
      {(record.user_messages ?? [])
        .filter(
          (u) => u.before_turn_number > record.turns.length,
        )
        .map((u, idx) => (
          <UserPromptBlock
            key={`u-trailing-${idx}`}
            msg={u}
            index={(record.user_messages ?? []).indexOf(u)}
            selectedKey={selectedKey}
            onSelect={onSelect}
            trailing
          />
        ))}

      {/* Summary block */}
      <Block
        keyId="summary"
        selectedKey={selectedKey}
        accent="green"
        onClick={() =>
          onSelect(
            "summary",
            <SummaryDetail record={record} totalTokens={totalTokens} totalTools={totalTools} />,
          )
        }
      >
        <div className="grid grid-cols-4 gap-1 text-center">
          <SummaryStat value={String(record.turns.length)} label="Turns" />
          <SummaryStat value={totalTokens.toLocaleString()} label="Tokens" />
          <SummaryStat value={fmtMs(record.total_duration_ms)} label="Time" />
          <SummaryStat value={String(totalTools)} label="Tools" />
        </div>
      </Block>
    </div>
  );
}

function SummaryStat({ value, label }: { value: string; label: string }) {
  return (
    <div>
      <div className="text-sm font-bold">{value}</div>
      <div className="text-[9px] text-neutral-500">{label}</div>
    </div>
  );
}

function TurnTimeline({
  turn,
  prevInputTokens,
  selectedKey,
  onSelect,
  onOpenSubAgent,
}: {
  turn: TurnRecord;
  prevInputTokens: number;
  selectedKey: string | null;
  onSelect: (key: string, content: ReactNode) => void;
  onOpenSubAgent: (tc: ToolCallRecord) => void;
}) {
  const lc = turn.llm_call;
  const turnTokens = lc.input_tokens + lc.output_tokens;
  const tokenDelta = prevInputTokens > 0 ? lc.input_tokens - prevInputTokens : 0;

  const turnKey = `turn-${turn.turn_number}`;
  const reqKey = `${turnKey}-req`;
  const respKey = `${turnKey}-resp`;

  const stopColor = stopReasonColor(lc.stop_reason, !!lc.error_message);

  // Cache hit/miss summary. Anthropic-only — null elsewhere.
  const cacheRead = lc.cache_read_input_tokens ?? 0;
  const cacheCreate = lc.cache_creation_input_tokens ?? 0;
  const hasCacheData =
    lc.cache_read_input_tokens != null ||
    lc.cache_creation_input_tokens != null;

  return (
    <div className="border-l-2 border-purple-700/60 pl-2.5 ml-1">
      <div className="flex items-center justify-between text-[10px] text-neutral-500 mb-1">
        <span className="font-semibold text-neutral-300">Turn {turn.turn_number}</span>
        <span className="font-mono">
          {fmtMs(turn.duration_ms)} · {turnTokens.toLocaleString()} tok
        </span>
      </div>

      {/* P2 marker chips: per-turn config knobs that affect this LLM call. */}
      {(lc.disable_tools ||
        lc.provider_options_applied ||
        (lc.system_prompt_segments ?? 0) > 1 ||
        hasCacheData) && (
        <div className="flex flex-wrap gap-1 mb-1">
          {(lc.system_prompt_segments ?? 0) > 1 && (
            <span
              title={`System prompt split into ${lc.system_prompt_segments} cache segments`}
              className="text-[9px] font-mono rounded px-1.5 py-0.5 bg-cyan-950/50 border border-cyan-900/60 text-cyan-300"
            >
              sp×{lc.system_prompt_segments}
            </span>
          )}
          {hasCacheData && (
            <span
              title={`Cache read ${cacheRead.toLocaleString()} / created ${cacheCreate.toLocaleString()} input tokens`}
              className={`text-[9px] font-mono rounded px-1.5 py-0.5 border ${
                cacheRead > 0
                  ? "bg-emerald-950/50 border-emerald-900/60 text-emerald-300"
                  : "bg-neutral-800/80 border-neutral-700/60 text-neutral-400"
              }`}
            >
              {cacheRead > 0
                ? `cache↓${formatTokens(cacheRead)}`
                : `cache∅`}
              {cacheCreate > 0 && ` +${formatTokens(cacheCreate)}`}
            </span>
          )}
          {lc.disable_tools && (
            <span
              title="Tools were disabled for this turn — request had no `tools` field"
              className="text-[9px] font-mono rounded px-1.5 py-0.5 bg-rose-950/50 border border-rose-900/60 text-rose-300"
            >
              ⊘ tools
            </span>
          )}
          {!lc.disable_tools && (lc.tools_count_sent ?? 0) > 0 && (
            <span
              title={`${lc.tools_count_sent} tool schemas sent`}
              className="text-[9px] font-mono rounded px-1.5 py-0.5 bg-neutral-800/80 border border-neutral-700/60 text-neutral-400"
            >
              {lc.tools_count_sent} tools
            </span>
          )}
          {lc.provider_options_applied && (
            <span
              title="Vendor-specific JSON pass-through (extra_body) merged into request"
              className="text-[9px] font-mono rounded px-1.5 py-0.5 bg-purple-950/50 border border-purple-900/60 text-purple-300"
            >
              {`{...}`}
            </span>
          )}
        </div>
      )}

      {/* LLM Request */}
      <Block
        keyId={reqKey}
        selectedKey={selectedKey}
        accent="blue"
        onClick={() =>
          onSelect(
            reqKey,
            <LlmRequestDetail
              llm={lc}
              turnNumber={turn.turn_number}
              tokenDelta={tokenDelta}
            />,
          )
        }
      >
        <div className="flex items-center justify-between text-[11px]">
          <span className="text-blue-400 font-semibold">LLM Request</span>
          <span className="text-neutral-500 font-mono text-[10px]">
            {lc.messages_sent_count} msgs · {lc.input_tokens.toLocaleString()} in
          </span>
        </div>
        {tokenDelta > 0 && (
          <div className="text-[10px] text-amber-400 mt-0.5">
            +{tokenDelta.toLocaleString()} from prev turn
          </div>
        )}
        {hasCacheData && cacheRead > 0 && (
          <div className="text-[10px] text-emerald-400 mt-0.5">
            cache hit: {cacheRead.toLocaleString()} tok read (~$
            {((cacheRead * 0.9) / 1_000_000).toFixed(4)} saved est.)
          </div>
        )}
      </Block>

      <Arrow />

      {/* LLM Response */}
      <Block
        keyId={respKey}
        selectedKey={selectedKey}
        accent={stopColor}
        onClick={() =>
          onSelect(
            respKey,
            <LlmResponseDetail llm={lc} turnNumber={turn.turn_number} />,
          )
        }
      >
        <div className="flex items-center justify-between text-[11px]">
          <span
            className={`font-semibold ${
              lc.error_message ? "text-red-400" : "text-green-400"
            }`}
          >
            LLM Response
            {lc.error_message ? " — ERROR" : ""}
          </span>
          <span className="text-neutral-500 font-mono text-[10px]">
            {lc.output_tokens.toLocaleString()} out · {fmtMs(lc.duration_ms)}
          </span>
        </div>
        <div className="mt-1">
          <StopReasonBadge stopReason={lc.stop_reason} hasError={!!lc.error_message} />
        </div>
        {lc.error_message && (
          <div className="text-[10px] text-red-300 mt-1 font-mono whitespace-pre-wrap break-all line-clamp-3">
            {lc.error_message}
          </div>
        )}
        {!lc.error_message && (
          <ResponsePreview response={lc.response} />
        )}
      </Block>

      {/* Tool calls */}
      {turn.tool_calls.map((tc, i) => {
        const tcKey = `${turnKey}-tool-${i}`;
        const isSubAgent = tc.tool_call.name === "agent";
        const isErr = tc.is_error;
        const accent: BlockAccent = isErr
          ? "red"
          : isSubAgent
            ? "purple"
            : "orange";
        const args = (tc.tool_call.arguments ?? {}) as Record<string, unknown>;
        const subTask = isSubAgent
          ? ((args.task as string) ?? (args.description as string) ?? "")
          : "";
        const errPreview =
          isErr && tc.result
            ? truncate(formatToolOutput(tc.result), 120)
            : "";
        const hasNestedRun =
          isSubAgent && tc.sub_run && tc.sub_run.turns.length > 0;

        return (
          <div key={tcKey}>
            <Arrow />
            <Block
              keyId={tcKey}
              selectedKey={selectedKey}
              accent={accent}
              onClick={() => {
                if (hasNestedRun) {
                  onOpenSubAgent(tc);
                  return;
                }
                onSelect(
                  tcKey,
                  <ToolCallDetail
                    tc={tc}
                    isSubAgent={isSubAgent}
                  />,
                );
              }}
            >
              <div className="flex items-center justify-between text-[11px]">
                <div className="flex items-center gap-1.5 min-w-0">
                  {isSubAgent ? (
                    <span className="rounded bg-purple-950/60 border border-purple-900 text-purple-300 text-[9px] px-1.5 py-0.5 font-semibold">
                      SUB-AGENT
                    </span>
                  ) : (
                    <span className="rounded bg-amber-950/60 border border-amber-900 text-amber-300 text-[9px] px-1.5 py-0.5 font-semibold">
                      TOOL
                    </span>
                  )}
                  <strong className="font-mono truncate">
                    {tc.tool_call.name}
                  </strong>
                  {isErr ? (
                    <span className="rounded bg-red-950/60 border border-red-900 text-red-300 text-[9px] px-1 py-0.5 font-semibold">
                      ERR
                    </span>
                  ) : (
                    <span className="rounded bg-green-950/60 border border-green-900 text-green-300 text-[9px] px-1 py-0.5 font-semibold">
                      OK
                    </span>
                  )}
                </div>
                <span className="text-neutral-500 font-mono text-[10px] shrink-0 ml-1">
                  {tc.duration_ms}ms
                </span>
              </div>
              {subTask && (
                <div className="text-[10px] text-neutral-500 mt-1 truncate">
                  {subTask}
                </div>
              )}
              {errPreview && (
                <div className="text-[10px] text-red-300 mt-1 font-mono whitespace-pre-wrap break-all line-clamp-2">
                  {errPreview}
                </div>
              )}
              {hasNestedRun && (
                <div className="text-[9px] text-purple-400 mt-1 flex items-center gap-1">
                  <ChevronDown size={10} className="-rotate-90" />
                  点击进入子 Agent 时间线
                </div>
              )}
            </Block>
          </div>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Block primitive
// ---------------------------------------------------------------------------

type BlockAccent =
  | "blue"
  | "green"
  | "orange"
  | "red"
  | "purple"
  | "cyan"
  | "neutral";

/** Inserted in the timeline immediately before the turn it triggered.
 * `index` is the position inside `record.user_messages` so two prompts
 * with the same `before_turn_number` (cancelled-before-iteration case)
 * still get unique selection keys. `trailing=true` styles a "still
 * pending / cancelled" prompt with no following turn. */
function UserPromptBlock({
  msg,
  index,
  selectedKey,
  onSelect,
  trailing = false,
}: {
  msg: UserMessageRecord;
  index: number;
  selectedKey: string | null;
  onSelect: (key: string, content: ReactNode) => void;
  trailing?: boolean;
}) {
  const keyId = `user_prompt:${index}`;
  const preview = msg.text.length > 90 ? `${msg.text.slice(0, 90)}…` : msg.text;
  return (
    <Block
      keyId={keyId}
      selectedKey={selectedKey}
      accent="blue"
      onClick={() =>
        onSelect(
          keyId,
          <DetailSection title="User prompt">
            <div className="text-[10px] text-neutral-500 mb-2">
              {new Date(msg.timestamp_ms).toLocaleString()}
              {trailing ? " · 等待 turn 启动" : ""}
            </div>
            <pre className="text-sm whitespace-pre-wrap break-words bg-neutral-950/40 rounded p-3 border border-neutral-800">
              {msg.text}
            </pre>
          </DetailSection>,
        )
      }
    >
      <div className="font-medium text-[12px] flex items-center gap-2">
        <span className="text-blue-400">→ 用户消息 #{index + 1}</span>
        {trailing && (
          <span className="text-[9px] uppercase tracking-wider text-amber-400">
            pending
          </span>
        )}
      </div>
      <div className="text-[11px] text-neutral-300 mt-1 truncate">
        {preview || "(空)"}
      </div>
    </Block>
  );
}

function Block({
  keyId,
  selectedKey,
  accent,
  onClick,
  children,
}: {
  keyId: string;
  selectedKey: string | null;
  accent: BlockAccent;
  onClick: () => void;
  children: ReactNode;
}) {
  const selected = selectedKey === keyId;
  const borderColor: Record<BlockAccent, string> = {
    blue: "border-l-blue-600",
    green: "border-l-green-600",
    orange: "border-l-amber-500",
    red: "border-l-red-600",
    purple: "border-l-purple-600",
    cyan: "border-l-cyan-500",
    neutral: "border-l-neutral-700",
  };
  return (
    <button
      type="button"
      onClick={onClick}
      className={`w-full text-left rounded border border-neutral-800 border-l-2 bg-neutral-900/40 hover:bg-neutral-900 transition-colors px-2 py-1.5 ${
        borderColor[accent]
      } ${selected ? "ring-1 ring-blue-500/60 bg-neutral-800/60" : ""}`}
    >
      {children}
    </button>
  );
}

function Arrow() {
  return (
    <div className="text-center text-neutral-600 text-[10px] leading-none my-0.5">
      ↓
    </div>
  );
}

function StopReasonBadge({
  stopReason,
  hasError,
}: {
  stopReason: string;
  hasError: boolean;
}) {
  const colorClass = hasError
    ? "bg-red-950/60 border-red-900 text-red-300"
    : stopReason === "tool_use"
      ? "bg-amber-950/60 border-amber-900 text-amber-300"
      : stopReason === "end_turn"
        ? "bg-green-950/60 border-green-900 text-green-300"
        : "bg-neutral-900 border-neutral-800 text-neutral-400";
  return (
    <span
      className={`inline-block rounded border px-1.5 py-0.5 text-[9px] font-mono ${colorClass}`}
    >
      {stopReason}
    </span>
  );
}

function ResponsePreview({ response }: { response: unknown }) {
  const preview = useMemo(() => extractResponsePreview(response), [response]);
  if (!preview) return null;
  return (
    <div className="text-[10px] text-neutral-500 mt-1 font-mono line-clamp-2 break-all">
      {preview}
    </div>
  );
}

function extractResponsePreview(response: unknown): string {
  if (!response || typeof response !== "object") return "";
  const r = response as Record<string, unknown>;
  const content = r.content;
  if (!Array.isArray(content)) return "";
  for (const block of content) {
    if (block && typeof block === "object") {
      const b = block as Record<string, unknown>;
      const text =
        (b.text as string) ?? ((b.Text as Record<string, unknown>)?.text as string);
      if (text) return truncate(text, 80);
      if (b.type === "tool_use" && b.name) {
        return `→ ${b.name as string}(${truncate(
          JSON.stringify(b.input ?? {}),
          50,
        )})`;
      }
    }
  }
  return "";
}

function stopReasonColor(stopReason: string, hasError: boolean): BlockAccent {
  if (hasError) return "red";
  if (stopReason === "end_turn") return "green";
  if (stopReason === "tool_use") return "orange";
  return "red";
}

// ---------------------------------------------------------------------------
// Detail panels
// ---------------------------------------------------------------------------

function DetailSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="mb-5">
      <h4 className="text-[10px] uppercase tracking-wider text-neutral-500 mb-2">
        {title}
      </h4>
      <div>{children}</div>
    </div>
  );
}

function JsonBlock({ value }: { value: unknown }) {
  let text: string;
  try {
    text = JSON.stringify(value, null, 2);
  } catch {
    text = String(value);
  }
  return (
    <pre className="rounded bg-neutral-950 border border-neutral-800 px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all max-h-96 overflow-auto text-neutral-300">
      {text}
    </pre>
  );
}

function OverviewDetail({
  record,
  totalTokens,
  totalTools,
}: {
  record: RunRecord;
  totalTokens: number;
  totalTools: number;
}) {
  const c: ConfigSnapshot = record.config_snapshot;

  // Aggregate prompt-cache stats across all turns. cache_read tokens
  // are the savings; cache_creation tokens are upfront cost. Hit rate
  // uses (read / (read+creation)) as a rough quality signal — purely
  // observability, not load-bearing logic.
  const cacheRead = record.turns.reduce(
    (sum, t) => sum + (t.llm_call.cache_read_input_tokens ?? 0),
    0,
  );
  const cacheCreated = record.turns.reduce(
    (sum, t) => sum + (t.llm_call.cache_creation_input_tokens ?? 0),
    0,
  );
  const cacheTotal = cacheRead + cacheCreated;
  const cacheHitRate = cacheTotal > 0 ? cacheRead / cacheTotal : 0;
  const hasCacheData = record.turns.some(
    (t) =>
      t.llm_call.cache_read_input_tokens != null ||
      t.llm_call.cache_creation_input_tokens != null,
  );

  return (
    <>
      <DetailSection title="Configuration">
        <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-[11px]">
          <KvLine label="Model" value={c.model_id || "(empty)"} />
          <KvLine label="Turns" value={String(record.turns.length)} />
          <KvLine label="Duration" value={fmtMs(record.total_duration_ms)} />
          <KvLine label="Tokens" value={totalTokens.toLocaleString()} />
          <KvLine label="Tools" value={String(c.tool_names.length)} />
          <KvLine label="Tool calls" value={String(totalTools)} />
          <KvLine label="Max iter" value={String(c.max_iterations)} />
          <KvLine
            label="In/Out"
            value={`${record.total_input_tokens.toLocaleString()} / ${record.total_output_tokens.toLocaleString()}`}
          />
        </div>
      </DetailSection>

      {hasCacheData && (
        <DetailSection title="Prompt Cache">
          <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-[11px]">
            <KvLine
              label="Hit rate"
              value={`${(cacheHitRate * 100).toFixed(1)}%`}
            />
            <KvLine label="Read" value={cacheRead.toLocaleString()} />
            <KvLine label="Created" value={cacheCreated.toLocaleString()} />
            <KvLine
              label="Saved (est)"
              value={`~${(cacheRead * 0.9).toLocaleString()} tok`}
            />
          </div>
          <div className="text-[10px] text-neutral-500 mt-2">
            Anthropic 给 cache_read 约 90% 折扣;hit rate 越高越好。
            cache 主要受 system prompt / tools / 历史变动影响 ——
            稳定段在请求前部 + last user 末尾打 marker。
          </div>
        </DetailSection>
      )}

      <DetailSection
        title={
          c.system_prompt.length > 1
            ? `System Prompt (${c.system_prompt.length} segments)`
            : "System Prompt"
        }
      >
        {c.system_prompt.length === 0 ? (
          <div className="text-[10px] text-neutral-500">(empty)</div>
        ) : (
          <div className="space-y-2">
            {c.system_prompt.map((seg, idx) => {
              const isLast = idx === c.system_prompt.length - 1;
              const cached = !isLast;
              return (
                <div key={idx}>
                  <div className="flex items-center gap-2 text-[10px] mb-0.5">
                    <span
                      className={`font-mono px-1.5 py-0.5 rounded ${
                        cached
                          ? "bg-emerald-950/50 border border-emerald-900/60 text-emerald-300"
                          : "bg-amber-950/50 border border-amber-900/60 text-amber-300"
                      }`}
                    >
                      {cached ? "stable · cacheable" : "dynamic · per-turn"}
                    </span>
                    <span className="text-neutral-500 font-mono">
                      seg #{idx + 1} · {seg.length.toLocaleString()} chars
                    </span>
                  </div>
                  <pre className="rounded bg-neutral-950 border border-neutral-800 px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all max-h-48 overflow-auto text-neutral-300">
                    {seg}
                  </pre>
                </div>
              );
            })}
          </div>
        )}
      </DetailSection>

      {c.tool_definitions && c.tool_definitions.length > 0 && (
        <DetailSection title={`Tool Definitions (${c.tool_definitions.length})`}>
          <div className="space-y-1">
            {c.tool_definitions.map((td, i) => (
              <ToolDefinitionRow key={i} td={td} />
            ))}
          </div>
        </DetailSection>
      )}

      {c.plugin_names.length > 0 && (
        <DetailSection title={`Plugins (${c.plugin_names.length})`}>
          <ChipList items={c.plugin_names} />
        </DetailSection>
      )}

      {c.middleware_names.length > 0 && (
        <DetailSection title={`Middleware Stack (${c.middleware_names.length})`}>
          <ChipList items={c.middleware_names} />
        </DetailSection>
      )}

      {(c.direct_middleware_names?.length ?? 0) > 0 && (
        <DetailSection title={`Direct Middleware (${c.direct_middleware_names?.length ?? 0})`}>
          <ChipList items={c.direct_middleware_names ?? []} />
        </DetailSection>
      )}

      {c.plugin_assembly && c.plugin_assembly.length > 0 && (
        <DetailSection title={`Plugin Contributions (${c.plugin_assembly.length})`}>
          <div className="space-y-1">
            {c.plugin_assembly.map((p) => {
              const registered = p.registered_tool_names.length;
              const finalized = p.finalized_tool_names.length;
              const middleware = p.middleware_names.length;
              const phases = p.phase_contribution_names?.length ?? 0;
              const commands = p.command_names.length;
              const prompts = p.system_prompt_fragments;
              return (
                <div
                  key={p.name}
                  className="rounded border border-neutral-800 bg-neutral-900/40 px-2 py-1.5"
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="font-mono text-[11px] text-neutral-200">{p.name}</div>
                    <div className="text-[10px] text-neutral-500">
                      tools {registered + finalized} · phase {phases} · mw {middleware} · cmd {commands} · prompt {prompts}
                    </div>
                  </div>
                  {p.description && (
                    <div className="mt-1 text-[10px] text-neutral-500">{p.description}</div>
                  )}
                </div>
              );
            })}
          </div>
        </DetailSection>
      )}
    </>
  );
}

function ToolDefinitionRow({ td }: { td: unknown }) {
  const [open, setOpen] = useState(false);
  const t = (td ?? {}) as Record<string, unknown>;
  const name = (t.name as string) ?? "(unnamed)";
  const description = (t.description as string) ?? "";
  return (
    <div className="rounded border border-neutral-800 bg-neutral-900/40">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full text-left px-2 py-1.5 flex items-start gap-1.5"
      >
        <ChevronDown
          size={10}
          className={`mt-0.5 shrink-0 transition-transform ${open ? "" : "-rotate-90"}`}
        />
        <span className="flex-1 min-w-0">
          <span className="block text-[11px] font-mono font-semibold">
            {name}
          </span>
          <span className="block text-[10px] text-neutral-500 truncate">
            {description}
          </span>
        </span>
      </button>
      {open && (
        <div className="px-2 pb-2">
          <JsonBlock value={t.parameters ?? {}} />
        </div>
      )}
    </div>
  );
}

function ChipList({ items }: { items: string[] }) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((it) => (
        <span
          key={it}
          className="rounded bg-neutral-800 border border-neutral-700 px-1.5 py-0.5 text-[10px] font-mono text-neutral-300"
        >
          {it}
        </span>
      ))}
    </div>
  );
}

function KvLine({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span className="text-neutral-500">{label}:</span>{" "}
      <strong className="text-neutral-200">{value}</strong>
    </div>
  );
}

function LlmRequestDetail({
  llm,
  turnNumber,
  tokenDelta,
}: {
  llm: LlmCallRecord;
  turnNumber: number;
  tokenDelta: number;
}) {
  return (
    <>
      <DetailSection title={`LLM Request — Turn ${turnNumber}`}>
        <div className="text-[11px]">
          Messages: <strong>{llm.messages_sent_count}</strong> · Input tokens:{" "}
          <strong>{llm.input_tokens.toLocaleString()}</strong>
        </div>
        {tokenDelta > 0 && (
          <div className="text-[10px] text-amber-400 mt-1">
            +{tokenDelta.toLocaleString()} tokens growth from previous turn
          </div>
        )}
      </DetailSection>
      <DetailSection title={`Messages Sent (${llm.messages_sent_count})`}>
        <JsonBlock value={llm.messages_sent} />
      </DetailSection>
    </>
  );
}

function LlmResponseDetail({
  llm,
  turnNumber,
}: {
  llm: LlmCallRecord;
  turnNumber: number;
}) {
  return (
    <>
      <DetailSection title={`LLM Response — Turn ${turnNumber}`}>
        <div className="text-[11px]">
          Stop:{" "}
          <strong>
            <StopReasonBadge
              stopReason={llm.stop_reason}
              hasError={!!llm.error_message}
            />
          </strong>{" "}
          · Tokens: <strong>{llm.output_tokens.toLocaleString()}</strong> ·
          Duration: <strong>{fmtMs(llm.duration_ms)}</strong>
        </div>
      </DetailSection>

      {llm.error_message && (
        <DetailSection title="Run Error">
          <pre className="rounded bg-red-950/30 border border-red-900/60 px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all text-red-300">
            {llm.error_message}
          </pre>
        </DetailSection>
      )}

      <DetailSection title="Full Response Object">
        <JsonBlock value={llm.response} />
      </DetailSection>
    </>
  );
}

function ToolCallDetail({
  tc,
  isSubAgent,
}: {
  tc: ToolCallRecord;
  isSubAgent: boolean;
}) {
  const output = tc.result ? formatToolOutput(tc.result) : "(no output)";
  return (
    <>
      <DetailSection title={`${isSubAgent ? "Sub-Agent" : "Tool"}: ${tc.tool_call.name}`}>
        <div className="text-[11px]">
          Status:{" "}
          {tc.is_error ? (
            <strong className="text-red-400">ERROR</strong>
          ) : (
            <strong className="text-green-400">OK</strong>
          )}{" "}
          · Duration: <strong>{tc.duration_ms}ms</strong>
        </div>
      </DetailSection>
      <DetailSection title="Input Arguments">
        <JsonBlock value={tc.tool_call.arguments} />
      </DetailSection>
      <DetailSection
        title={`${tc.is_error ? "Error Output" : "Output"} (${output.length} chars)`}
      >
        <pre
          className={`rounded border px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all max-h-96 overflow-auto ${
            tc.is_error
              ? "bg-red-950/30 border-red-900/60 text-red-300"
              : "bg-neutral-950 border-neutral-800 text-neutral-300"
          }`}
        >
          {output}
        </pre>
      </DetailSection>
    </>
  );
}

function SummaryDetail({
  record,
  totalTokens,
  totalTools,
}: {
  record: RunRecord;
  totalTokens: number;
  totalTools: number;
}) {
  return (
    <>
      <DetailSection title="Summary">
        <div className="text-[11px] space-y-0.5">
          <div>Turns: {record.turns.length}</div>
          <div>
            Total tokens: {totalTokens.toLocaleString()} (
            {record.total_input_tokens.toLocaleString()} in +{" "}
            {record.total_output_tokens.toLocaleString()} out)
          </div>
          <div>Duration: {fmtMs(record.total_duration_ms)}</div>
          <div>Tool calls: {totalTools}</div>
        </div>
      </DetailSection>
      {record.turns.length > 1 && (
        <DetailSection title="Per-Turn Token Breakdown">
          <div className="space-y-1">
            {record.turns.map((t) => {
              const tok = t.llm_call.input_tokens + t.llm_call.output_tokens;
              const pct = totalTokens > 0 ? (tok / totalTokens) * 100 : 0;
              return (
                <div
                  key={t.turn_number}
                  className="flex items-center gap-2 text-[10px] font-mono"
                >
                  <span className="w-8 text-neutral-500">T{t.turn_number}</span>
                  <div className="flex-1 h-1.5 bg-neutral-900 rounded overflow-hidden">
                    <div
                      className="h-full bg-blue-600"
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                  <span className="w-14 text-right text-neutral-500">
                    {tok.toLocaleString()}
                  </span>
                </div>
              );
            })}
          </div>
        </DetailSection>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function fmtMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

/** Compact token counter: 1234 → 1.2K, 1_234_567 → 1.2M. Used in
 * cache-hit chips so a 50K cached prefix doesn't wreck the chip width. */
function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return s.slice(0, n) + "…";
}

function formatToolOutput(result: unknown): string {
  if (result == null) return "";
  if (typeof result === "string") return result;
  const r = result as Record<string, unknown>;
  // ToolOutput typically has { content: [{type:"text", text}] | string, is_error }
  if (typeof r.content === "string") return r.content;
  if (Array.isArray(r.content)) {
    return r.content
      .map((b) => {
        if (typeof b === "string") return b;
        if (b && typeof b === "object") {
          const block = b as Record<string, unknown>;
          if (typeof block.text === "string") return block.text;
        }
        return "";
      })
      .join("");
  }
  try {
    return JSON.stringify(result, null, 2);
  } catch {
    return String(result);
  }
}

// ---------------------------------------------------------------------------
// SessionIdBadge
// ---------------------------------------------------------------------------

/**
 * Shows the active session id in the Inspector header, full id visible on
 * hover, one click copies to clipboard so users can paste it into bug reports
 * without hunting through logs.
 */
function SessionIdBadge({ sessionId }: { sessionId: string | null }) {
  const [copied, setCopied] = useState(false);

  if (!sessionId) return null;

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(sessionId);
      setCopied(true);
      // Reset after a beat — long enough for the user to see the confirmation.
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied in some webview contexts; ignore.
    }
  };

  return (
    <button
      type="button"
      onClick={handleCopy}
      title={`${sessionId} — 点击复制`}
      className="min-w-0 flex-1 truncate text-left font-mono normal-case tracking-normal text-[11px] text-neutral-400 hover:text-white transition-colors"
    >
      <span className="text-neutral-600">id:</span>{" "}
      <span>{copied ? "已复制" : sessionId}</span>
    </button>
  );
}
