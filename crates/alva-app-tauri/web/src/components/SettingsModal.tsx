import { useEffect, useState } from "react";
import {
  Brain,
  CheckCircle2,
  Download,
  Pencil,
  Plug,
  Plus,
  RotateCcw,
  Search,
  Trash2,
  Wrench,
  XCircle,
} from "lucide-react";
import {
  type AgentPreset,
  type ModelOverride,
  type ProviderConfig,
  type ProviderKind,
  useAppStore,
} from "../store/appStore";
import {
  listAllSkills,
  listRemoteModels,
  testProviderConnection,
  type ConnectionTestResult,
  type RemoteModelInfo,
  type SkillInfo,
} from "../agent-bridge";
import { Modal } from "./Modal";

type SettingsTab = "models" | "agents";

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  initialTab?: SettingsTab;
}

export function SettingsModal({
  open,
  onClose,
  initialTab = "models",
}: SettingsModalProps) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);

  return (
    <Modal open={open} onClose={onClose}>
      <div className="flex flex-1 min-h-0">
        {/* Left tab rail */}
        <div className="w-44 shrink-0 border-r border-neutral-800 bg-neutral-950 flex flex-col">
          <div className="px-4 py-3 text-sm font-semibold border-b border-neutral-800">
            设置
          </div>
          <ul className="flex-1 py-2">
            <li>
              <button
                type="button"
                onClick={() => setTab("models")}
                className={`w-full text-left px-4 py-2 text-sm ${
                  tab === "models"
                    ? "bg-neutral-800 text-white"
                    : "text-neutral-300 hover:bg-neutral-900"
                }`}
              >
                模型
              </button>
            </li>
            <li>
              <button
                type="button"
                onClick={() => setTab("agents")}
                className={`w-full text-left px-4 py-2 text-sm ${
                  tab === "agents"
                    ? "bg-neutral-800 text-white"
                    : "text-neutral-300 hover:bg-neutral-900"
                }`}
              >
                我的 Agent
              </button>
            </li>
          </ul>
        </div>

        {/* Tab body */}
        <div className="flex-1 min-w-0 flex flex-col">
          {tab === "models" && <ModelsTab />}
          {tab === "agents" && <AgentsTab />}
        </div>
      </div>

      {/* Footer */}
      <div className="border-t border-neutral-800 px-4 py-3 flex justify-end gap-2">
        <button
          type="button"
          onClick={onClose}
          className="rounded bg-neutral-800 hover:bg-neutral-700 px-4 py-1.5 text-sm"
        >
          关闭
        </button>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Models tab
// ---------------------------------------------------------------------------

const PROVIDER_OPTIONS: { value: ProviderKind; label: string }[] = [
  { value: "anthropic", label: "Anthropic" },
  { value: "openai", label: "OpenAI (Chat Completions)" },
  { value: "openai-responses", label: "OpenAI (Responses)" },
];

function ModelsTab() {
  const configs = useAppStore((s) => s.providerConfigs);
  const activeId = useAppStore((s) => s.activeProviderConfigId);
  const addConfig = useAppStore((s) => s.addProviderConfig);
  const updateConfig = useAppStore((s) => s.updateProviderConfig);
  const deleteConfig = useAppStore((s) => s.deleteProviderConfig);
  const setActive = useAppStore((s) => s.setActiveProviderConfig);

  const [selectedId, setSelectedId] = useState<string | null>(
    activeId ?? configs[0]?.id ?? null,
  );

  const selected = configs.find((c) => c.id === selectedId) ?? null;

  const handleAdd = () => {
    const entry = addConfig({
      name: "新模型配置",
      provider: "anthropic",
      model: "claude-sonnet-4-6",
      api_key: "",
    });
    setSelectedId(entry.id);
  };

  return (
    <div className="flex flex-1 min-h-0">
      {/* Config list */}
      <div className="w-60 shrink-0 border-r border-neutral-800 flex flex-col">
        <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800">
          <span className="text-xs text-neutral-500 uppercase tracking-wider">
            模型配置
          </span>
          <button
            type="button"
            onClick={handleAdd}
            className="text-neutral-400 hover:text-white"
            title="添加"
          >
            <Plus size={16} />
          </button>
        </div>
        <ul className="flex-1 overflow-auto">
          {configs.length === 0 && (
            <li className="px-3 py-4 text-xs text-neutral-500">
              还没有模型配置。点右上角 + 添加。
            </li>
          )}
          {configs.map((c) => (
            <li key={c.id}>
              <button
                type="button"
                onClick={() => setSelectedId(c.id)}
                className={`w-full text-left px-3 py-2 border-b border-neutral-900 ${
                  c.id === selectedId
                    ? "bg-neutral-900"
                    : "hover:bg-neutral-900/50"
                }`}
              >
                <div className="text-sm truncate flex items-center gap-2">
                  {c.name}
                  {c.id === activeId && (
                    <span className="text-[10px] text-blue-400 font-mono">
                      当前
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-neutral-500 truncate">
                  {c.provider} · {c.model}
                </div>
              </button>
            </li>
          ))}
        </ul>
      </div>

      {/* Editor */}
      <div className="flex-1 min-w-0 overflow-auto p-6">
        {selected ? (
          <ProviderEditor
            key={selected.id}
            config={selected}
            isActive={selected.id === activeId}
            onUpdate={(updates) => updateConfig(selected.id, updates)}
            onDelete={() => {
              deleteConfig(selected.id);
              setSelectedId(null);
            }}
            onSetActive={() => setActive(selected.id)}
          />
        ) : (
          <div className="h-full flex items-center justify-center text-neutral-500 text-sm">
            从左边选一个配置来编辑,或点 + 添加新的。
          </div>
        )}
      </div>
    </div>
  );
}

interface ProviderEditorProps {
  config: ProviderConfig;
  isActive: boolean;
  onUpdate: (updates: Partial<Omit<ProviderConfig, "id">>) => void;
  onDelete: () => void;
  onSetActive: () => void;
}

function ProviderEditor({
  config,
  isActive,
  onUpdate,
  onDelete,
  onSetActive,
}: ProviderEditorProps) {
  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(
    null,
  );
  const [testing, setTesting] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [remoteModels, setRemoteModels] = useState<RemoteModelInfo[] | null>(
    null,
  );
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [modelQuery, setModelQuery] = useState("");
  const [editingModelId, setEditingModelId] = useState<string | null>(null);

  // Token-AND match: "gpt 4o" splits to ["gpt","4o"] and both must substring-
  // match somewhere in the haystack. Handles "claude sonnet 4" → matches
  // "anthropic/claude-sonnet-4-20250514" because dash doesn't block substring.
  const filteredRemoteModels = (() => {
    if (!remoteModels) return null;
    const q = modelQuery.trim().toLowerCase();
    if (!q) return remoteModels;
    const tokens = q.split(/\s+/).filter(Boolean);
    return remoteModels.filter((m) => {
      const text =
        `${m.id} ${m.display_name ?? ""} ${m.owned_by ?? ""}`.toLowerCase();
      return tokens.every((tok) => text.includes(tok));
    });
  })();

  const canTest = config.api_key.trim().length > 0;

  const runTest = async () => {
    if (!canTest) return;
    setTesting(true);
    setTestResult(null);
    try {
      const result = await testProviderConnection({
        provider: config.provider,
        api_key: config.api_key,
        base_url: config.base_url || null,
      });
      setTestResult(result);
    } catch (e) {
      setTestResult({
        ok: false,
        latency_ms: 0,
        status: null,
        message: String(e),
        model_count: 0,
      });
    } finally {
      setTesting(false);
    }
  };

  const fetchModels = async () => {
    if (!canTest) return;
    setFetchingModels(true);
    setFetchError(null);
    setRemoteModels(null);
    setModelQuery("");
    try {
      const list = await listRemoteModels({
        provider: config.provider,
        api_key: config.api_key,
        base_url: config.base_url || null,
      });
      setRemoteModels(list);
    } catch (e) {
      setFetchError(String(e));
    } finally {
      setFetchingModels(false);
    }
  };

  return (
    <div className="max-w-xl">
      <div className="flex items-center gap-3 mb-6">
        <input
          value={config.name}
          onChange={(e) => onUpdate({ name: e.target.value })}
          className="flex-1 rounded bg-neutral-900 border border-neutral-800 px-3 py-2 text-base font-medium outline-none focus:border-blue-600"
        />
        <button
          type="button"
          onClick={onDelete}
          className="text-neutral-500 hover:text-red-400"
          title="删除"
        >
          <Trash2 size={16} />
        </button>
      </div>

      <div className="space-y-4">
        <Field label="Provider">
          <select
            value={config.provider}
            onChange={(e) => onUpdate({ provider: e.target.value as ProviderKind })}
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none"
          >
            {PROVIDER_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </Field>

        <Field label="Model">
          <input
            value={config.model}
            onChange={(e) => onUpdate({ model: e.target.value })}
            placeholder="claude-sonnet-4-6 / gpt-4o / ..."
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none"
          />
        </Field>

        <Field label="API Key">
          <input
            type="password"
            value={config.api_key}
            onChange={(e) => onUpdate({ api_key: e.target.value })}
            placeholder="sk-..."
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none font-mono text-xs"
          />
        </Field>

        <Field label="Base URL(可选)">
          <input
            value={config.base_url ?? ""}
            onChange={(e) =>
              onUpdate({ base_url: e.target.value || undefined })
            }
            placeholder="留空用默认 endpoint"
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none font-mono text-xs"
          />
        </Field>

        {/* Connection test + remote model fetch */}
        <div className="pt-4 border-t border-neutral-800 space-y-3">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={runTest}
              disabled={!canTest || testing}
              className="inline-flex items-center gap-1.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 px-3 py-1.5 text-xs"
            >
              <Plug size={14} />
              {testing ? "测试中…" : "测试连接"}
            </button>
            <button
              type="button"
              onClick={fetchModels}
              disabled={!canTest || fetchingModels}
              className="inline-flex items-center gap-1.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 px-3 py-1.5 text-xs"
            >
              <Download size={14} />
              {fetchingModels ? "获取中…" : "获取模型列表"}
            </button>
            {!canTest && (
              <span className="text-[10px] text-neutral-500">
                先填 API Key
              </span>
            )}
          </div>

          {testResult && <TestResultBadge result={testResult} />}

          {remoteModels !== null && (
            <div className="rounded border border-neutral-800 bg-neutral-900/50">
              <div className="px-3 py-2 text-[10px] uppercase tracking-wider text-neutral-500 border-b border-neutral-800 flex items-center justify-between">
                <span>
                  远端模型 (
                  {filteredRemoteModels?.length ?? 0}
                  {modelQuery.trim() && ` / ${remoteModels.length}`})
                </span>
                <span className="text-neutral-600">点击应用到 Model 字段</span>
              </div>

              {/* Search — appears once a list is loaded */}
              {remoteModels.length > 0 && (
                <div className="flex items-center gap-2 border-b border-neutral-800 px-3 py-1.5">
                  <Search size={12} className="text-neutral-500" />
                  <input
                    value={modelQuery}
                    onChange={(e) => setModelQuery(e.target.value)}
                    placeholder="搜索 model id"
                    className="flex-1 bg-transparent outline-none text-xs font-mono"
                  />
                  {modelQuery && (
                    <button
                      type="button"
                      onClick={() => setModelQuery("")}
                      className="text-neutral-500 hover:text-white text-[10px]"
                    >
                      清空
                    </button>
                  )}
                </div>
              )}

              <div className="max-h-60 overflow-auto">
                {remoteModels.length === 0 ? (
                  <div className="px-3 py-3 text-xs text-neutral-500">
                    服务器返回空列表
                  </div>
                ) : filteredRemoteModels && filteredRemoteModels.length === 0 ? (
                  <div className="px-3 py-3 text-xs text-neutral-500">
                    无匹配
                  </div>
                ) : (
                  filteredRemoteModels?.map((m) => (
                    <ModelRow
                      key={m.id}
                      model={m}
                      isCurrent={m.id === config.model}
                      isEditing={editingModelId === m.id}
                      onSelect={() => onUpdate({ model: m.id })}
                      onToggleEdit={() =>
                        setEditingModelId((prev) =>
                          prev === m.id ? null : m.id,
                        )
                      }
                      onCloseEdit={() => setEditingModelId(null)}
                    />
                  ))
                )}
              </div>
            </div>
          )}

          {fetchError && (
            <div className="rounded border border-red-900/50 bg-red-950/30 text-red-200 px-3 py-2 text-xs">
              <div className="font-medium mb-1">获取失败</div>
              <div className="font-mono text-[11px] whitespace-pre-wrap break-all">
                {fetchError}
              </div>
            </div>
          )}
        </div>

        <div className="pt-4 border-t border-neutral-800">
          <button
            type="button"
            onClick={onSetActive}
            disabled={isActive}
            className="rounded bg-blue-700 hover:bg-blue-600 disabled:opacity-40 px-4 py-2 text-sm"
          >
            {isActive ? "当前使用中" : "设为当前模型"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Positive-only capability marker. The parent decides whether to render
 *  it — we never show a "doesn't support" state, following the "明确的就显示,
 *  不明确的就不显示" rule. */
function CapChip({
  icon,
  label,
  tooltip,
  manual = false,
}: {
  icon: React.ReactNode;
  label: string;
  tooltip: string;
  manual?: boolean;
}) {
  const classes = manual
    ? "border-amber-900/60 bg-amber-950/40 text-amber-300"
    : "border-green-900/60 bg-green-950/40 text-green-300";
  return (
    <span
      title={manual ? `${tooltip}(手动)` : tooltip}
      className={`inline-flex items-center gap-0.5 rounded border px-1 py-0.5 text-[9px] font-mono ${classes}`}
      aria-label={label}
    >
      {icon}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Per-model row with inline capability override editor
// ---------------------------------------------------------------------------

interface ModelRowProps {
  model: RemoteModelInfo;
  isCurrent: boolean;
  isEditing: boolean;
  onSelect: () => void;
  onToggleEdit: () => void;
  onCloseEdit: () => void;
}

function ModelRow({
  model,
  isCurrent,
  isEditing,
  onSelect,
  onToggleEdit,
  onCloseEdit,
}: ModelRowProps) {
  const override = useAppStore((s) => s.modelOverrides[model.id]);
  const setModelOverride = useAppStore((s) => s.setModelOverride);

  // Merge: override fields (if set) take precedence over the API-reported caps.
  const apiCaps = model.capabilities;
  const effectiveTools =
    override?.supports_tools ?? apiCaps.supports_tools ?? null;
  const effectiveReasoning =
    override?.is_reasoning ?? apiCaps.is_reasoning ?? null;
  const effectiveContext =
    override?.context_window ?? apiCaps.context_window ?? null;

  const showTools = effectiveTools === true;
  const showReasoning = effectiveReasoning === true;
  const showContext = effectiveContext !== null;
  const hasAnyCap = showTools || showReasoning || showContext;

  const toolsFromOverride = override?.supports_tools !== undefined;
  const reasoningFromOverride = override?.is_reasoning !== undefined;
  const contextFromOverride = override?.context_window !== undefined;

  return (
    <div
      className={`border-b border-neutral-900 last:border-b-0 ${
        isCurrent ? "bg-blue-950/60" : ""
      }`}
    >
      <div
        className={`flex items-center gap-2 px-3 py-1.5 text-xs ${
          isCurrent
            ? "text-blue-200"
            : "hover:bg-neutral-800 text-neutral-300"
        }`}
      >
        <button
          type="button"
          onClick={onSelect}
          className="font-mono truncate flex-1 min-w-0 text-left"
        >
          {model.id}
        </button>

        {hasAnyCap && (
          <span className="flex items-center gap-1 shrink-0">
            {showTools && (
              <CapChip
                icon={<Wrench size={10} />}
                label="tools"
                tooltip="支持工具调用"
                manual={toolsFromOverride}
              />
            )}
            {showReasoning && (
              <CapChip
                icon={<Brain size={10} />}
                label="think"
                tooltip="思考模型"
                manual={reasoningFromOverride}
              />
            )}
            {showContext && (
              <span
                title={`上下文窗口 ${effectiveContext!.toLocaleString()} tokens${
                  contextFromOverride ? "(手动)" : ""
                }`}
                className={`rounded px-1.5 py-0.5 text-[9px] font-mono ${
                  contextFromOverride
                    ? "bg-amber-950/40 border border-amber-900/60 text-amber-300"
                    : "bg-neutral-800 text-neutral-400"
                }`}
              >
                {formatContext(effectiveContext!)}
              </span>
            )}
          </span>
        )}

        <button
          type="button"
          onClick={onToggleEdit}
          title="编辑该模型的能力"
          className={`shrink-0 p-0.5 rounded ${
            isEditing
              ? "text-blue-300 bg-blue-900/40"
              : "text-neutral-500 hover:text-white hover:bg-neutral-700"
          }`}
        >
          <Pencil size={11} />
        </button>

        {isCurrent && (
          <CheckCircle2 size={12} className="text-blue-400 shrink-0" />
        )}
      </div>

      {isEditing && (
        <ModelOverrideEditor
          modelId={model.id}
          apiCaps={apiCaps}
          override={override}
          onChange={(next) => setModelOverride(model.id, next)}
          onClose={onCloseEdit}
        />
      )}
    </div>
  );
}

function ModelOverrideEditor({
  modelId,
  apiCaps,
  override,
  onChange,
  onClose,
}: {
  modelId: string;
  apiCaps: RemoteModelInfo["capabilities"];
  override: ModelOverride | undefined;
  onChange: (next: ModelOverride | null) => void;
  onClose: () => void;
}) {
  // Local draft so toggling checkboxes / typing numbers is snappy.
  const [draft, setDraft] = useState<ModelOverride>(
    override ?? {
      supports_tools: apiCaps.supports_tools ?? undefined,
      is_reasoning: apiCaps.is_reasoning ?? undefined,
      context_window: apiCaps.context_window ?? undefined,
    },
  );

  const isEmpty =
    draft.supports_tools === undefined &&
    draft.is_reasoning === undefined &&
    draft.context_window === undefined;

  return (
    <div className="px-3 py-3 bg-neutral-950/80 border-t border-neutral-900 space-y-3">
      <div className="text-[10px] uppercase tracking-wider text-neutral-500 flex items-center gap-2">
        手动能力设置
        <span className="font-mono text-neutral-600 normal-case truncate">
          {modelId}
        </span>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <TriState
          label="工具调用"
          value={draft.supports_tools}
          onChange={(v) => setDraft({ ...draft, supports_tools: v })}
        />
        <TriState
          label="思考模型"
          value={draft.is_reasoning}
          onChange={(v) => setDraft({ ...draft, is_reasoning: v })}
        />
        <div>
          <div className="text-[10px] text-neutral-500 mb-1">上下文窗口</div>
          <input
            type="number"
            value={draft.context_window ?? ""}
            onChange={(e) => {
              const n = Number(e.target.value);
              setDraft({
                ...draft,
                context_window:
                  e.target.value === "" || !Number.isFinite(n) ? undefined : n,
              });
            }}
            placeholder="tokens(留空=未知)"
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 text-xs outline-none font-mono"
          />
        </div>
      </div>

      <div className="flex items-center justify-end gap-2 pt-1">
        <button
          type="button"
          onClick={() => {
            onChange(null);
            onClose();
          }}
          className="inline-flex items-center gap-1 text-[11px] text-neutral-400 hover:text-white px-2 py-1"
        >
          <RotateCcw size={11} />
          清除覆盖
        </button>
        <button
          type="button"
          onClick={onClose}
          className="text-[11px] text-neutral-400 hover:text-white px-2 py-1"
        >
          取消
        </button>
        <button
          type="button"
          onClick={() => {
            onChange(isEmpty ? null : draft);
            onClose();
          }}
          className="rounded bg-blue-700 hover:bg-blue-600 px-3 py-1 text-[11px] font-medium"
        >
          保存
        </button>
      </div>
    </div>
  );
}

/** Three-state toggle: unknown / false / true. */
function TriState({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean | undefined;
  onChange: (v: boolean | undefined) => void;
}) {
  const btn = (v: boolean | undefined, text: string) => {
    const active = value === v;
    return (
      <button
        type="button"
        onClick={() => onChange(v)}
        className={`flex-1 text-[10px] py-1 border transition-colors ${
          active
            ? "bg-blue-700 border-blue-700 text-white"
            : "bg-neutral-900 border-neutral-800 text-neutral-400 hover:border-neutral-700"
        }`}
      >
        {text}
      </button>
    );
  };
  return (
    <div>
      <div className="text-[10px] text-neutral-500 mb-1">{label}</div>
      <div className="flex">
        <div className="flex-1 [&>button]:rounded-none [&>button:first-child]:rounded-l [&>button:last-child]:rounded-r">
          {btn(undefined, "未知")}
          {btn(false, "不支持")}
          {btn(true, "支持")}
        </div>
      </div>
    </div>
  );
}

function formatContext(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(0)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
  return String(n);
}

function TestResultBadge({ result }: { result: ConnectionTestResult }) {
  if (result.ok) {
    return (
      <div className="rounded border border-green-900/60 bg-green-950/30 text-green-300 px-3 py-2 text-xs flex items-center gap-2">
        <CheckCircle2 size={14} />
        <span>连接成功</span>
        <span className="text-green-500/70">· {result.latency_ms}ms</span>
        {result.model_count > 0 && (
          <span className="text-green-500/70">
            · {result.model_count} 个模型
          </span>
        )}
      </div>
    );
  }
  return (
    <div className="rounded border border-red-900/60 bg-red-950/30 text-red-200 px-3 py-2 text-xs">
      <div className="flex items-center gap-2 mb-1">
        <XCircle size={14} />
        <span>连接失败</span>
        {result.status && (
          <span className="text-red-400/80">· HTTP {result.status}</span>
        )}
      </div>
      {result.message && (
        <div className="font-mono text-[11px] whitespace-pre-wrap break-all text-red-300/80">
          {result.message}
        </div>
      )}
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <div className="text-xs text-neutral-400 mb-1">{label}</div>
      {children}
    </label>
  );
}

// ---------------------------------------------------------------------------
// Agents tab
// ---------------------------------------------------------------------------

function AgentsTab() {
  const agents = useAppStore((s) => s.agents);
  const addAgent = useAppStore((s) => s.addAgent);
  const updateAgent = useAppStore((s) => s.updateAgent);
  const deleteAgent = useAppStore((s) => s.deleteAgent);

  const [selectedId, setSelectedId] = useState<string | null>(
    agents[0]?.id ?? null,
  );
  const [availableSkills, setAvailableSkills] = useState<SkillInfo[]>([]);

  useEffect(() => {
    listAllSkills()
      .then(setAvailableSkills)
      .catch(() => {});
  }, []);

  const selected = agents.find((a) => a.id === selectedId) ?? null;

  const handleAdd = () => {
    const entry = addAgent({
      name: "新 Agent",
      system_prompt: "You are a helpful assistant.",
      preset_skills: [],
      preset_tools: [],
    });
    setSelectedId(entry.id);
  };

  return (
    <div className="flex flex-1 min-h-0">
      <div className="w-60 shrink-0 border-r border-neutral-800 flex flex-col">
        <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800">
          <span className="text-xs text-neutral-500 uppercase tracking-wider">
            Agent 列表
          </span>
          <button
            type="button"
            onClick={handleAdd}
            className="text-neutral-400 hover:text-white"
            title="添加"
          >
            <Plus size={16} />
          </button>
        </div>
        <ul className="flex-1 overflow-auto">
          {agents.length === 0 && (
            <li className="px-3 py-4 text-xs text-neutral-500">
              还没有自定义 Agent。点右上角 + 添加。
            </li>
          )}
          {agents.map((a) => (
            <li key={a.id}>
              <button
                type="button"
                onClick={() => setSelectedId(a.id)}
                className={`w-full text-left px-3 py-2 border-b border-neutral-900 ${
                  a.id === selectedId
                    ? "bg-neutral-900"
                    : "hover:bg-neutral-900/50"
                }`}
              >
                <div className="text-sm truncate">{a.name}</div>
                <div className="text-[10px] text-neutral-500 truncate">
                  {a.preset_skills.length} 技能 · {a.preset_tools.length} 工具
                </div>
              </button>
            </li>
          ))}
        </ul>
      </div>

      <div className="flex-1 min-w-0 overflow-auto p-6">
        {selected ? (
          <AgentEditor
            key={selected.id}
            agent={selected}
            availableSkills={availableSkills}
            onUpdate={(updates) => updateAgent(selected.id, updates)}
            onDelete={() => {
              deleteAgent(selected.id);
              setSelectedId(null);
            }}
          />
        ) : (
          <div className="h-full flex items-center justify-center text-neutral-500 text-sm">
            从左边选一个 Agent 来编辑,或点 + 添加新的。
          </div>
        )}
      </div>
    </div>
  );
}

interface AgentEditorProps {
  agent: AgentPreset;
  availableSkills: SkillInfo[];
  onUpdate: (updates: Partial<Omit<AgentPreset, "id">>) => void;
  onDelete: () => void;
}

function AgentEditor({
  agent,
  availableSkills,
  onUpdate,
  onDelete,
}: AgentEditorProps) {
  const toggleSkill = (name: string) => {
    if (agent.preset_skills.includes(name)) {
      onUpdate({ preset_skills: agent.preset_skills.filter((n) => n !== name) });
    } else {
      onUpdate({ preset_skills: [...agent.preset_skills, name] });
    }
  };

  return (
    <div className="max-w-2xl">
      <div className="flex items-center gap-3 mb-6">
        <input
          value={agent.name}
          onChange={(e) => onUpdate({ name: e.target.value })}
          className="flex-1 rounded bg-neutral-900 border border-neutral-800 px-3 py-2 text-base font-medium outline-none focus:border-blue-600"
        />
        <button
          type="button"
          onClick={onDelete}
          className="text-neutral-500 hover:text-red-400"
          title="删除"
        >
          <Trash2 size={16} />
        </button>
      </div>

      <div className="space-y-4">
        <Field label="System Prompt">
          <textarea
            value={agent.system_prompt}
            onChange={(e) => onUpdate({ system_prompt: e.target.value })}
            rows={6}
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none resize-y text-sm font-mono"
          />
        </Field>

        <Field label={`预置技能 (${agent.preset_skills.length})`}>
          <div className="rounded bg-neutral-900 border border-neutral-800 max-h-48 overflow-auto">
            {availableSkills.length === 0 ? (
              <div className="px-3 py-2 text-xs text-neutral-500">
                没发现可用技能
              </div>
            ) : (
              availableSkills.map((s) => {
                const on = agent.preset_skills.includes(s.name);
                return (
                  <label
                    key={`${s.source_dir}::${s.name}`}
                    className="flex items-start gap-2 px-3 py-2 cursor-pointer hover:bg-neutral-800/50 text-xs"
                  >
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={() => toggleSkill(s.name)}
                      className="mt-0.5"
                    />
                    <span className="flex-1 min-w-0">
                      <span className="block font-medium truncate">
                        {s.name}
                      </span>
                      <span className="block text-neutral-500 truncate">
                        {s.description}
                      </span>
                    </span>
                  </label>
                );
              })
            )}
          </div>
        </Field>

        <Field label="预置工具(逗号分隔)">
          <input
            value={agent.preset_tools.join(", ")}
            onChange={(e) =>
              onUpdate({
                preset_tools: e.target.value
                  .split(",")
                  .map((s) => s.trim())
                  .filter(Boolean),
              })
            }
            placeholder="file_read, shell, internet_search, ..."
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none font-mono text-xs"
          />
        </Field>

        <Field label="备注(可选)">
          <textarea
            value={agent.notes ?? ""}
            onChange={(e) => onUpdate({ notes: e.target.value })}
            rows={2}
            className="w-full rounded bg-neutral-900 border border-neutral-800 px-3 py-2 outline-none resize-y text-xs"
          />
        </Field>
      </div>
    </div>
  );
}
