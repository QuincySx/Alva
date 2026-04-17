import { useEffect, useRef, useState } from "react";
import {
  type AgentEventEnvelope,
  type ChatEntry,
  type SessionInfo,
  createSession,
  listSessionEvents,
  listSessions,
  openInspectorWindow,
  openSessionWorkspace,
  sendMessage,
  setSessionWorkspace,
  subscribeAgentEvents,
  switchSession,
} from "../agent-bridge";
import {
  Brain,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Folder,
  Maximize2,
  Minimize2,
  Plug,
  Puzzle,
  Settings2,
  Sparkles,
  Wrench,
  X,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Inspector } from "../components/Inspector";
import { ModelPicker } from "../components/ModelPicker";
import { SkillPicker } from "../components/SkillPicker";
import { ToolPicker } from "../components/ToolPicker";
import { useActiveProviderConfig, useAppStore } from "../store/appStore";

export default function Home() {
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const setActiveSessionId = useAppStore((s) => s.setActiveSessionId);
  const bumpSessionList = useAppStore((s) => s.bumpSessionList);
  const sessionListNonce = useAppStore((s) => s.sessionListNonce);

  const [activeSessionInfo, setActiveSessionInfo] = useState<SessionInfo | null>(
    null,
  );

  const activeIdRef = useRef<string | null>(null);
  useEffect(() => {
    activeIdRef.current = activeSessionId;
  }, [activeSessionId]);

  const [messages, setMessages] = useState<ChatEntry[]>([]);
  const [input, setInput] = useState("");
  const [running, setRunning] = useState(false);
  const [showInspector, setShowInspector] = useState(false);
  const [inspectorTab, setInspectorTab] = useState<"projection" | "events">(
    "projection",
  );
  const [inspectorNonce, setInspectorNonce] = useState(0);
  const [inspectorFullscreen, setInspectorFullscreen] = useState(false);
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);

  const providerConfig = useActiveProviderConfig();
  const openSettings = useAppStore((s) => s.openSettings);
  const toolsAutoMode = useAppStore((s) => s.toolsAutoMode);
  const storeSelectedTools = useAppStore((s) => s.selectedTools);

  // Raw events come straight from the Rust session store via
  // `list_session_events`, so Raw Events tab survives: Home unmount,
  // route changes, drawer toggles, AND process restart. Fetched on
  // session switch + on AgentEnd (via inspectorNonce).
  const [events, setEvents] = useState<unknown[]>([]);

  // Streaming state — held in refs so listeners stay stable and StrictMode's
  // double-invoked state updaters stay pure. Reset on MessageStart / End /
  // ToolExec / AgentEnd so each LLM turn starts fresh.
  const assistantStreamOpenRef = useRef(false);
  const thinkingStreamOpenRef = useRef(false);

  const scrollRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);

  // No auto-select on mount. User picks a task from the sidebar
  // or clicks "新建任务". Until then, the empty-state landing shows.

  // When activeSessionId changes (from NavSidebar click or create), fetch
  // the history and refresh the header title.
  useEffect(() => {
    if (!activeSessionId) {
      setMessages([]);
      setActiveSessionInfo(null);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const [entries, all] = await Promise.all([
          switchSession(activeSessionId),
          listSessions(),
        ]);
        if (cancelled) return;
        setMessages(entries);
        const info = all.find((s) => s.id === activeSessionId) ?? null;
        setActiveSessionInfo(info);
      } catch (e) {
        console.error("switch session failed", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeSessionId]);

  // Keep the header title in sync when the backend updates the session
  // (e.g. title changed on first user message after AgentEnd bumps).
  useEffect(() => {
    if (!activeSessionId) return;
    let cancelled = false;
    (async () => {
      try {
        const all = await listSessions();
        if (cancelled) return;
        const info = all.find((s) => s.id === activeSessionId) ?? null;
        if (info) setActiveSessionInfo(info);
      } catch {
        /* ignore */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionListNonce, activeSessionId]);

  // Hydrate Raw Events from the Rust session store. Triggered on session
  // switch (new id) AND on AgentEnd (inspectorNonce bumped). Since these
  // live in the sqlite-backed session store, they survive process restart
  // and stay correct across Home unmounts.
  useEffect(() => {
    if (!activeSessionId) {
      setEvents([]);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const fetched = await listSessionEvents(activeSessionId);
        if (!cancelled) setEvents(fetched);
      } catch (e) {
        if (!cancelled) {
          console.error("listSessionEvents failed", e);
          setEvents([]);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeSessionId, inspectorNonce]);

  useEffect(() => {
    const appendAssistant = (text: string) => {
      if (!assistantStreamOpenRef.current) {
        assistantStreamOpenRef.current = true;
        setMessages((prev) => [...prev, { type: "assistant", text }]);
      } else {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.type === "assistant") {
            return [...prev.slice(0, -1), { ...last, text: last.text + text }];
          }
          return prev;
        });
      }
    };

    const appendThinking = (text: string) => {
      if (!thinkingStreamOpenRef.current) {
        thinkingStreamOpenRef.current = true;
        setMessages((prev) => [...prev, { type: "thinking", text }]);
      } else {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.type === "thinking") {
            return [...prev.slice(0, -1), { ...last, text: last.text + text }];
          }
          return prev;
        });
      }
    };

    const closeStreams = () => {
      assistantStreamOpenRef.current = false;
      thinkingStreamOpenRef.current = false;
    };

    const p = subscribeAgentEvents((envelope: AgentEventEnvelope) => {
      if (envelope.session_id !== activeIdRef.current) return;
      const ev = envelope.event;

      switch (ev.type) {
        case "MessageStart":
          closeStreams();
          break;

        case "MessageUpdate": {
          const delta = (ev as { delta: unknown }).delta;
          if (!delta || typeof delta !== "object") break;
          const d = delta as Record<string, unknown>;

          const textDelta = d.TextDelta as { text?: string } | undefined;
          if (textDelta?.text) {
            // Switching from thinking → text closes the thinking stream
            // so the next thinking delta starts a fresh block.
            thinkingStreamOpenRef.current = false;
            appendAssistant(textDelta.text);
            break;
          }

          const reasoningDelta = d.ReasoningDelta as { text?: string } | undefined;
          if (reasoningDelta?.text) {
            assistantStreamOpenRef.current = false;
            appendThinking(reasoningDelta.text);
            break;
          }
          break;
        }

        case "MessageEnd":
          closeStreams();
          break;

        case "MessageError":
          setMessages((prev) => [
            ...prev,
            { type: "error", text: String(ev.error) },
          ]);
          closeStreams();
          break;

        case "ToolExecutionStart": {
          const tc = (ev as { tool_call: Record<string, unknown> }).tool_call;
          const id = (tc?.id as string) ?? "";
          const name = (tc?.name as string) ?? "tool";
          // Different backends name the arg field differently; accept both.
          const args =
            (tc?.arguments as unknown) ?? (tc?.input as unknown) ?? {};
          setMessages((prev) => [
            ...prev,
            {
              type: "tool_call",
              id,
              name,
              arguments: args,
              result: null,
              is_error: false,
            },
          ]);
          closeStreams();
          break;
        }

        case "ToolExecutionEnd": {
          const tc = (ev as { tool_call: Record<string, unknown> }).tool_call;
          const id = (tc?.id as string) ?? "";
          const result = (ev as { result: unknown }).result;
          const flattened = flattenToolOutput(result);
          const isError = extractIsError(result);
          setMessages((prev) => {
            // Walk from the end to find the matching pending ToolCall entry.
            for (let i = prev.length - 1; i >= 0; i--) {
              const entry = prev[i];
              if (entry.type === "tool_call" && entry.id === id) {
                const next = prev.slice();
                next[i] = { ...entry, result: flattened, is_error: isError };
                return next;
              }
            }
            return prev;
          });
          break;
        }

        case "AgentEnd":
          if (ev.error) {
            setMessages((prev) => [
              ...prev,
              { type: "error", text: `agent ended: ${ev.error}` },
            ]);
          }
          closeStreams();
          setRunning(false);
          setInspectorNonce((n) => n + 1);
          bumpSessionList();
          // Refetch canonical history from Rust so any server-side
          // projection rules (e.g. tool_use + tool_result grouping) win
          // over whatever we built up from the live stream.
          if (activeIdRef.current) {
            const sid = activeIdRef.current;
            switchSession(sid)
              .then((entries) => {
                if (activeIdRef.current === sid) setMessages(entries);
              })
              .catch(() => {});
          }
          break;

        case "RunChannelClosed":
          closeStreams();
          setRunning(false);
          break;
      }
    });
    return () => {
      p.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages, events]);

  const submit = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!input.trim() || running || !activeSessionId) return;
    if (!providerConfig) {
      openSettings();
      return;
    }
    const text = input.trim();
    setInput("");
    setMessages((prev) => [...prev, { type: "user", text }]);
    setRunning(true);
    try {
      await sendMessage({
        provider: providerConfig.provider,
        model: providerConfig.model,
        api_key: providerConfig.api_key || null,
        base_url: providerConfig.base_url || null,
        session_id: activeSessionId,
        skill_names: selectedSkills.length > 0 ? selectedSkills : null,
        tool_names: toolsAutoMode ? null : storeSelectedTools,
        text,
      });
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        { type: "error", text: `send_message failed: ${String(err)}` },
      ]);
      setRunning(false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  // Folder button next to ToolPicker. Behaviour depends on whether the
  // session has started sending messages yet:
  //   - new session (no messages): open native folder picker → set workspace
  //   - active session (has messages): open the workspace folder in the OS
  //     file manager
  const handleFolderButton = async () => {
    if (!activeSessionId) return;
    if (messages.length === 0 && !running) {
      try {
        const selected = await openDialog({
          directory: true,
          multiple: false,
          defaultPath: activeSessionInfo?.workspace_path ?? undefined,
          title: "选择该任务的工作目录",
        });
        if (typeof selected === "string" && selected) {
          await setSessionWorkspace(activeSessionId, selected);
          bumpSessionList();
        }
      } catch (e) {
        console.error("pick/set workspace failed", e);
      }
    } else {
      try {
        await openSessionWorkspace(activeSessionId);
      } catch (e) {
        console.error("open workspace failed", e);
      }
    }
  };

  const handleNewTask = async () => {
    if (running) return;
    try {
      const fresh = await createSession();
      setActiveSessionId(fresh.id);
      setMessages([]);
      bumpSessionList();
      composerRef.current?.focus();
    } catch (e) {
      console.error("create session failed", e);
    }
  };

  const isEmpty = messages.length === 0;

  // No active session — empty-state landing with quick-access shortcuts
  if (!activeSessionId) {
    return (
      <div className="flex h-full flex-col items-center justify-center bg-neutral-950 text-neutral-100 px-6">
        <div className="max-w-md w-full space-y-10">
          <div className="text-center space-y-2">
            <div className="text-2xl font-semibold">Alva Agent</div>
            <div className="text-sm text-neutral-500">选择左侧任务继续，或新建一个</div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <ShortcutCard icon={<Settings2 size={16} />} label="模型设置" onClick={openSettings} />
            <ShortcutCard icon={<Puzzle size={16} />} label="插件管理" onClick={() => {/* TODO: navigate to skills */}} />
            <ShortcutCard icon={<Plug size={16} />} label="MCP 服务" onClick={() => {/* TODO: navigate to mcp */}} />
            <ShortcutCard icon={<Sparkles size={16} />} label="技能" onClick={() => {/* TODO: navigate to skills tab */}} />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-neutral-950 text-neutral-100">
      {/* Header: model picker top-left, task title middle, actions top-right */}
      <header className="flex items-center gap-3 border-b border-neutral-900 px-4 py-2">
        <ModelPicker />
        <span className="flex-1" />
        <span className="text-xs text-neutral-500 truncate max-w-xs">
          {activeSessionInfo?.title ?? "新建任务"}
        </span>
        <button
          type="button"
          onClick={handleNewTask}
          disabled={running}
          className="rounded bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-3 py-1.5 text-xs disabled:opacity-40"
        >
          新任务
        </button>
        <button
          type="button"
          onClick={() => setShowInspector((v) => !v)}
          className="rounded bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-3 py-1.5 text-xs"
        >
          {showInspector ? "隐藏调试" : "调试"}
        </button>
      </header>

      {/* Body: empty-state landing or conversation */}
      <div className="flex flex-1 overflow-hidden">
        <main
          ref={scrollRef}
          className={`flex-1 overflow-auto ${
            isEmpty
              ? "flex items-center justify-center"
              : "px-6 py-6 space-y-3"
          }`}
        >
          {isEmpty ? (
            <div className="flex flex-col items-center text-center max-w-xl px-6">
              <div className="text-3xl font-semibold mb-2">开始协作</div>
              <div className="text-sm text-neutral-500 mb-8">
                7×24 小时帮你干活的全场景个人助理 Agent
              </div>
            </div>
          ) : (
            messages.map((m, i) => <ChatEntryView key={i} entry={m} />)
          )}
        </main>

        {showInspector && (
          <aside
            className={
              inspectorFullscreen
                ? "fixed inset-0 z-40 flex flex-col bg-neutral-950"
                : "w-[760px] border-l border-neutral-800 flex flex-col bg-neutral-950"
            }
          >
            {/* Tabs */}
            <div className="flex items-center border-b border-neutral-800 shrink-0">
              <button
                type="button"
                onClick={() => setInspectorTab("projection")}
                className={`px-4 py-2 text-[11px] ${
                  inspectorTab === "projection"
                    ? "bg-neutral-900 text-white border-b-2 border-blue-600"
                    : "text-neutral-400 hover:text-white"
                }`}
              >
                Inspector
              </button>
              <button
                type="button"
                onClick={() => setInspectorTab("events")}
                className={`px-4 py-2 text-[11px] ${
                  inspectorTab === "events"
                    ? "bg-neutral-900 text-white border-b-2 border-blue-600"
                    : "text-neutral-400 hover:text-white"
                }`}
              >
                Raw Events ({events.length})
              </button>
              <span className="flex-1" />
              <button
                type="button"
                onClick={async () => {
                  if (!activeSessionId) return;
                  try {
                    localStorage.setItem(
                      "alva.inspector.session_id",
                      activeSessionId,
                    );
                    await openInspectorWindow();
                  } catch (e) {
                    console.error("open inspector window failed", e);
                  }
                }}
                disabled={!activeSessionId}
                title="在独立窗口打开 Inspector"
                className="px-3 py-2 text-neutral-500 hover:text-white disabled:opacity-40"
              >
                <ExternalLink size={12} />
              </button>
              <button
                type="button"
                onClick={() => setInspectorFullscreen((v) => !v)}
                title={inspectorFullscreen ? "退出全屏" : "全屏"}
                className="px-3 py-2 text-neutral-500 hover:text-white"
              >
                {inspectorFullscreen ? (
                  <Minimize2 size={12} />
                ) : (
                  <Maximize2 size={12} />
                )}
              </button>
              <button
                type="button"
                onClick={() => {
                  setShowInspector(false);
                  setInspectorFullscreen(false);
                }}
                title="关闭"
                className="px-3 py-2 text-neutral-500 hover:text-white"
              >
                <X size={12} />
              </button>
            </div>

            <div className="flex-1 min-h-0 overflow-hidden">
              {inspectorTab === "projection" ? (
                <Inspector
                  sessionId={activeSessionId}
                  refreshNonce={inspectorNonce}
                />
              ) : (
                <div className="h-full overflow-auto p-2 text-[11px] font-mono">
                  {events.length === 0 ? (
                    <div className="text-neutral-500">还没有事件</div>
                  ) : (
                    events.map((ev, i) => (
                      <pre
                        key={i}
                        className="mb-1 whitespace-pre-wrap break-all text-neutral-400"
                      >
                        {JSON.stringify(ev)}
                      </pre>
                    ))
                  )}
                </div>
              )}
            </div>
          </aside>
        )}
      </div>

      {/* Composer */}
      <footer className="border-t border-neutral-900 px-4 py-3">
        <form onSubmit={submit}>
          <div className="rounded-xl border border-neutral-800 bg-neutral-900 focus-within:border-neutral-700 transition-colors">
            <textarea
              ref={composerRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={onKeyDown}
              placeholder={
                !providerConfig
                  ? "先在设置里添加一个模型配置 →"
                  : running
                    ? "运行中…"
                    : !activeSessionId
                      ? "准备中…"
                      : "告诉 Alva 你想做什么。Enter 发送,Shift+Enter 换行。"
              }
              disabled={running || !activeSessionId}
              rows={3}
              className="w-full bg-transparent px-4 py-3 outline-none resize-none text-sm disabled:opacity-50"
            />
            <div className="flex items-center gap-2 border-t border-neutral-800 px-3 py-2">
              <ToolPicker />
              <SkillPicker
                selected={selectedSkills}
                onChange={setSelectedSkills}
              />
              <button
                type="button"
                onClick={handleFolderButton}
                disabled={!activeSessionId}
                title={
                  !activeSessionId
                    ? "没有活动任务"
                    : messages.length === 0 && !running
                      ? `选择工作目录(当前: ${activeSessionInfo?.workspace_path ?? "未设置"})`
                      : `打开工作目录: ${activeSessionInfo?.workspace_path ?? "未设置"}`
                }
                className="flex items-center gap-1.5 rounded-md bg-neutral-800/50 hover:bg-neutral-800 disabled:opacity-40 px-2 py-1 text-xs"
              >
                <Folder size={12} />
                {messages.length === 0 && !running ? "目录" : "打开"}
              </button>
              {selectedSkills.map((name) => (
                <span
                  key={name}
                  className="inline-flex items-center gap-1 rounded-full bg-blue-950/60 border border-blue-900/60 px-2 py-0.5 text-[10px] text-blue-200"
                >
                  {name}
                  <button
                    type="button"
                    onClick={() =>
                      setSelectedSkills(
                        selectedSkills.filter((n) => n !== name),
                      )
                    }
                    className="text-blue-400 hover:text-white"
                  >
                    ×
                  </button>
                </span>
              ))}
              <span className="flex-1" />
              <button
                type="submit"
                disabled={running || !input.trim() || !activeSessionId}
                className="rounded-md bg-blue-700 hover:bg-blue-600 disabled:opacity-40 px-4 py-1.5 text-sm font-medium"
              >
                {running ? "…" : "发送"}
              </button>
            </div>
          </div>
        </form>
      </footer>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Chat entry renderers
// ---------------------------------------------------------------------------

function ChatEntryView({ entry }: { entry: ChatEntry }) {
  switch (entry.type) {
    case "user":
      return (
        <div className="rounded-lg px-4 py-3 bg-blue-950/40 border border-blue-900/50">
          <div className="text-[10px] uppercase tracking-wider text-neutral-500 mb-1">
            你
          </div>
          <div className="whitespace-pre-wrap">{entry.text}</div>
        </div>
      );
    case "assistant":
      return (
        <div className="rounded-lg px-4 py-3 bg-neutral-900 border border-neutral-800">
          <div className="text-[10px] uppercase tracking-wider text-neutral-500 mb-1">
            Alva
          </div>
          <div className="whitespace-pre-wrap">{entry.text}</div>
        </div>
      );
    case "system":
      return (
        <div className="rounded-lg px-4 py-3 bg-neutral-900/50 border border-neutral-800 text-neutral-400">
          <div className="text-[10px] uppercase tracking-wider text-neutral-500 mb-1">
            system
          </div>
          <div className="whitespace-pre-wrap text-xs">{entry.text}</div>
        </div>
      );
    case "error":
      return (
        <div className="rounded-lg px-4 py-3 bg-red-950/40 border border-red-900/50 text-red-200">
          <div className="text-[10px] uppercase tracking-wider text-red-400 mb-1">
            error
          </div>
          <div className="whitespace-pre-wrap">{entry.text}</div>
        </div>
      );
    case "thinking":
      return <ThinkingBubble text={entry.text} />;
    case "tool_call":
      return <ToolCallBubble entry={entry} />;
  }
}

function ThinkingBubble({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="rounded-lg border border-purple-900/40 bg-purple-950/15 px-3 py-2">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 text-[11px] text-purple-300 hover:text-purple-200"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Brain size={12} />
        <span>思考</span>
        <span className="text-purple-500/60 font-mono text-[10px]">
          {text.length} 字
        </span>
      </button>
      {open && (
        <div className="mt-2 text-[11px] text-purple-200/90 whitespace-pre-wrap leading-relaxed">
          {text}
        </div>
      )}
    </div>
  );
}

function ToolCallBubble({
  entry,
}: {
  entry: Extract<ChatEntry, { type: "tool_call" }>;
}) {
  const [open, setOpen] = useState(false);
  const statusLabel =
    entry.result === null ? "运行中…" : entry.is_error ? "错误" : "完成";
  const statusClass =
    entry.result === null
      ? "text-amber-400"
      : entry.is_error
        ? "text-red-300"
        : "text-green-400";
  const argsJson = (() => {
    try {
      return JSON.stringify(entry.arguments, null, 2);
    } catch {
      return String(entry.arguments);
    }
  })();
  return (
    <div
      className={`rounded-lg border px-3 py-2 ${
        entry.is_error
          ? "border-red-900/50 bg-red-950/15"
          : "border-amber-900/40 bg-amber-950/15"
      }`}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 text-[11px] w-full text-left"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Wrench size={12} className="text-amber-400" />
        <span className="font-mono font-semibold text-amber-200">
          {entry.name}
        </span>
        <span className="flex-1" />
        <span className={`text-[10px] ${statusClass}`}>{statusLabel}</span>
      </button>
      {open && (
        <div className="mt-2 space-y-2">
          <div>
            <div className="text-[10px] text-amber-400/70 mb-1">参数</div>
            <pre className="rounded bg-neutral-950 border border-neutral-800 px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all max-h-48 overflow-auto text-neutral-300">
              {argsJson}
            </pre>
          </div>
          {entry.result !== null && (
            <div>
              <div className="text-[10px] text-amber-400/70 mb-1">
                输出{entry.is_error ? "(错误)" : ""}
              </div>
              <pre
                className={`rounded border px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all max-h-60 overflow-auto ${
                  entry.is_error
                    ? "bg-red-950/30 border-red-900/60 text-red-200"
                    : "bg-neutral-950 border-neutral-800 text-neutral-300"
                }`}
              >
                {entry.result}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Streaming helpers: flatten live ToolOutput (from ToolExecutionEnd event)
// ---------------------------------------------------------------------------

function flattenToolOutput(result: unknown): string {
  if (result == null) return "";
  if (typeof result === "string") return result;
  const r = result as Record<string, unknown>;
  if (typeof r.content === "string") return r.content;
  if (Array.isArray(r.content)) {
    return r.content
      .map((block) => {
        if (typeof block === "string") return block;
        if (block && typeof block === "object") {
          const b = block as Record<string, unknown>;
          if (typeof b.text === "string") return b.text;
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

function extractIsError(result: unknown): boolean {
  if (!result || typeof result !== "object") return false;
  const r = result as Record<string, unknown>;
  return r.is_error === true;
}

// ---------------------------------------------------------------------------
// Shortcut card for empty-state landing
// ---------------------------------------------------------------------------

function ShortcutCard({
  icon,
  label,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex flex-col items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900/40 hover:bg-neutral-900 hover:border-neutral-700 transition-colors p-5"
    >
      <div className="w-9 h-9 rounded-lg bg-neutral-800 flex items-center justify-center text-neutral-400">
        {icon}
      </div>
      <span className="text-xs text-neutral-300">{label}</span>
    </button>
  );
}
