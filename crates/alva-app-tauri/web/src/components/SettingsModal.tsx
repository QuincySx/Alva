import { useEffect, useState, type ReactNode } from "react";
import {
  Boxes,
  Brain,
  Check,
  CheckCircle2,
  Download,
  Eye,
  Image as ImageIcon,
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
  resolveModelCapabilities,
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
  { value: "gemini", label: "Google Gemini" },
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
  // Global cache of the last-fetched model list for this config. Hydrates
  // the local list on mount so reopening Settings doesn't force a fresh
  // fetch, and receives the fresh result each time the user clicks
  // "获取模型列表". The local state mirrors the cache value for rendering.
  const cachedRemoteModels = useAppStore(
    (s) => s.remoteModelsCache[config.id] ?? null,
  );
  const persistRemoteModels = useAppStore((s) => s.setRemoteModels);

  // User-added models for this provider config. Persisted; merged with
  // the remote list under one rendered list so the picker treats them
  // identically (same row, same edit panel, same chips).
  const manualModels = useAppStore(
    (s) => s.manualModels[config.id] ?? [],
  );
  const addManualModel = useAppStore((s) => s.addManualModel);
  const removeManualModel = useAppStore((s) => s.removeManualModel);

  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(
    null,
  );
  const [testing, setTesting] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [remoteModels, setRemoteModels] = useState<RemoteModelInfo[] | null>(
    cachedRemoteModels,
  );
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [modelQuery, setModelQuery] = useState("");
  const [editingModelId, setEditingModelId] = useState<string | null>(null);

  // Inline manual-add form state. Two text fields + ADD button at the
  // top of the list; on add, model joins the merged list and is
  // indistinguishable from a remote one.
  const [manualIdDraft, setManualIdDraft] = useState("");
  const [manualNameDraft, setManualNameDraft] = useState("");
  const submitManualModel = () => {
    const id = manualIdDraft.trim();
    if (!id) return;
    const display = manualNameDraft.trim();
    addManualModel(config.id, {
      id,
      display_name: display || null,
      owned_by: null,
      capabilities: {
        supports_tools: null,
        is_reasoning: null,
        context_window: null,
        max_output_tokens: null,
      },
    });
    setManualIdDraft("");
    setManualNameDraft("");
  };

  // Merged display list: manual models first (most-recent-on-top, same
  // order they were added), then remote models, de-duped by id (manual
  // wins so the user's display_name is preserved when both exist).
  const allModels: RemoteModelInfo[] = (() => {
    const seen = new Set<string>();
    const out: RemoteModelInfo[] = [];
    for (const m of manualModels) {
      if (!seen.has(m.id)) {
        out.push(m);
        seen.add(m.id);
      }
    }
    for (const m of remoteModels ?? []) {
      if (!seen.has(m.id)) {
        out.push(m);
        seen.add(m.id);
      }
    }
    return out;
  })();
  const isManual = (id: string) => manualModels.some((m) => m.id === id);

  // Legacy migration: a config saved before the standalone Model input
  // was removed may carry an id that's in neither cache. Promote it
  // into manualModels on mount so it shows up in the list (otherwise
  // the user sees no selection and can't tell what's active). Runs
  // exactly once per (configId, modelId) — guard via the manual list.
  useEffect(() => {
    const id = config.model.trim();
    if (!id) return;
    const inManual = manualModels.some((m) => m.id === id);
    const inRemote = (remoteModels ?? []).some((m) => m.id === id);
    if (!inManual && !inRemote) {
      addManualModel(config.id, {
        id,
        display_name: null,
        owned_by: null,
        capabilities: {
          supports_tools: null,
          is_reasoning: null,
          context_window: null,
          max_output_tokens: null,
        },
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [config.id, config.model]);

  // Token-AND match: "gpt 4o" splits to ["gpt","4o"] and both must substring-
  // match somewhere in the haystack. Handles "claude sonnet 4" → matches
  // "anthropic/claude-sonnet-4-20250514" because dash doesn't block substring.
  const filteredAllModels = (() => {
    const q = modelQuery.trim().toLowerCase();
    if (!q) return allModels;
    const tokens = q.split(/\s+/).filter(Boolean);
    return allModels.filter((m) => {
      const text =
        `${m.id} ${m.display_name ?? ""} ${m.owned_by ?? ""}`.toLowerCase();
      return tokens.every((tok) => text.includes(tok));
    });
  })();

  // 测试连接 needs a model to fire a real ping; 获取模型列表 doesn't
  // (it's the chicken-and-egg case where the user wants to discover
  // models before they can pick one).
  const hasKey = config.api_key.trim().length > 0;
  const canTest = hasKey && config.model.trim().length > 0;
  const canFetch = hasKey;

  const runTest = async () => {
    if (!canTest) return;
    setTesting(true);
    setTestResult(null);
    try {
      const result = await testProviderConnection({
        provider: config.provider,
        api_key: config.api_key,
        model: config.model,
        base_url: config.base_url || null,
      });
      setTestResult(result);
    } catch (e) {
      setTestResult({
        ok: false,
        latency_ms: 0,
        message: String(e),
        model: config.model,
        sample_response: null,
        input_tokens: null,
        output_tokens: null,
      });
    } finally {
      setTesting(false);
    }
  };

  const fetchModels = async () => {
    if (!canFetch) return;
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
      persistRemoteModels(config.id, list);
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
              disabled={!canFetch || fetchingModels}
              className="inline-flex items-center gap-1.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 px-3 py-1.5 text-xs"
            >
              <Download size={14} />
              {fetchingModels ? "获取中…" : "获取模型列表"}
            </button>
            {!hasKey && (
              <span className="text-[10px] text-neutral-500">
                先填 API Key
              </span>
            )}
            {hasKey && !config.model.trim() && (
              <span className="text-[10px] text-neutral-500">
                先在下方列表选/加一个模型才能测试
              </span>
            )}
          </div>

          {testResult && <TestResultBadge result={testResult} />}

          {/* Unified model list. Manual + remote sources merged into one
              rendered list — no source badge, both editable via the
              pencil icon, both selectable by click. The list is always
              rendered (even before the user fetches) because manual
              entries can populate it independently. */}
          <div className="rounded border border-neutral-800 bg-neutral-900/50">
            <div className="px-3 py-2 text-[10px] uppercase tracking-wider text-neutral-500 border-b border-neutral-800 flex items-center justify-between">
              <span>
                模型 (
                {filteredAllModels.length}
                {modelQuery.trim() && ` / ${allModels.length}`})
              </span>
              <span className="text-neutral-600">
                点击切换当前模型
              </span>
            </div>

            {/* Inline manual-add row. Always at the very top — id +
                display name + ADD button. Empty id is no-op. */}
            <div className="flex items-center gap-2 border-b border-neutral-800 px-3 py-1.5 bg-neutral-950/40">
              <input
                value={manualIdDraft}
                onChange={(e) => setManualIdDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    submitManualModel();
                  }
                }}
                placeholder="Model ID"
                className="flex-1 min-w-0 bg-neutral-900 border border-neutral-800 rounded px-2 py-1 text-xs font-mono outline-none focus:border-neutral-600"
              />
              <input
                value={manualNameDraft}
                onChange={(e) => setManualNameDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    submitManualModel();
                  }
                }}
                placeholder="显示名(可选)"
                className="flex-1 min-w-0 bg-neutral-900 border border-neutral-800 rounded px-2 py-1 text-xs outline-none focus:border-neutral-600"
              />
              <button
                type="button"
                onClick={submitManualModel}
                disabled={!manualIdDraft.trim()}
                className="inline-flex items-center gap-1 rounded bg-blue-700 hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed px-2.5 py-1 text-[11px] font-medium text-white"
              >
                <Plus size={11} />
                添加
              </button>
            </div>

            {/* Search — only after the list has anything in it */}
            {allModels.length > 0 && (
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
              {allModels.length === 0 ? (
                <div className="px-3 py-3 text-xs text-neutral-500">
                  没有模型。点上方"获取模型列表"，或者直接在上面那行手动添加。
                </div>
              ) : filteredAllModels.length === 0 ? (
                <div className="px-3 py-3 text-xs text-neutral-500">
                  无匹配
                </div>
              ) : (
                filteredAllModels.map((m) => (
                  <ModelRow
                    key={m.id}
                    model={m}
                    isCurrent={m.id === config.model}
                    isEditing={editingModelId === m.id}
                    canDelete={isManual(m.id)}
                    onSelect={() => onUpdate({ model: m.id })}
                    onDelete={() => {
                      removeManualModel(config.id, m.id);
                      // If we just deleted the active selection, blank it
                      // out so the user is forced to pick a new one
                      // (rather than silently keeping a dead reference).
                      if (config.model === m.id) {
                        onUpdate({ model: "" });
                      }
                    }}
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

/** Capability marker. Defaults to the green "supports X" look; pass
 *  `tone="deny"` for a red "explicitly disabled" variant (shown when the
 *  user turned a default-on capability off via the pencil). */
function CapChip({
  icon,
  label,
  tooltip,
  manual = false,
  tone = "allow",
}: {
  icon: React.ReactNode;
  label: string;
  tooltip: string;
  manual?: boolean;
  tone?: "allow" | "deny";
}) {
  const classes =
    tone === "deny"
      ? "border-red-900/60 bg-red-950/40 text-red-300"
      : manual
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
  /** True for user-added entries; surfaces a hover-only ✕ delete. */
  canDelete: boolean;
  onSelect: () => void;
  onDelete: () => void;
  onToggleEdit: () => void;
  onCloseEdit: () => void;
}

function ModelRow({
  model,
  isCurrent,
  isEditing,
  canDelete,
  onSelect,
  onDelete,
  onToggleEdit,
  onCloseEdit,
}: ModelRowProps) {
  const override = useAppStore((s) => s.modelOverrides[model.id]);
  const setModelOverride = useAppStore((s) => s.setModelOverride);

  // Merge: override fields (if set) take precedence over the API-reported caps,
  // then the default-on / default-off resolver fills in the rest.
  const apiCaps = model.capabilities;
  const resolved = resolveModelCapabilities(override, apiCaps);
  const effectiveContext =
    override?.context_window ?? apiCaps.context_window ?? null;
  const effectiveMaxOutput =
    override?.max_output_tokens ?? apiCaps.max_output_tokens ?? null;

  // Chips only surface DEVIATIONS from the default — otherwise every row
  // would show "tools + reasoning" and the chip bar would be noise.
  const showToolsDisabled = resolved.supports_tools === false;
  const showReasoningDisabled = resolved.is_reasoning === false;
  const showVisionEnabled = resolved.vision === true;
  const showImageOutput =
    (override?.image_output ?? apiCaps.image_output ?? false) === true;
  const showEmbedding =
    (override?.embedding ?? apiCaps.embedding ?? false) === true;
  const hasProviderOptions =
    !!override?.provider_options &&
    Object.keys(override.provider_options).length > 0;
  const showContext = effectiveContext !== null;
  const showMaxOutput = effectiveMaxOutput !== null;
  const hasAnyCap =
    showToolsDisabled ||
    showReasoningDisabled ||
    showVisionEnabled ||
    showImageOutput ||
    showEmbedding ||
    hasProviderOptions ||
    showContext ||
    showMaxOutput;

  const toolsFromOverride = override?.supports_tools !== undefined;
  const reasoningFromOverride = override?.is_reasoning !== undefined;
  const visionFromOverride = override?.vision !== undefined;
  const imageOutputFromOverride = override?.image_output !== undefined;
  const embeddingFromOverride = override?.embedding !== undefined;
  const contextFromOverride = override?.context_window !== undefined;
  const maxOutputFromOverride = override?.max_output_tokens !== undefined;

  return (
    <div
      className={`group border-b border-neutral-900 last:border-b-0 ${
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
          className="flex-1 min-w-0 text-left"
        >
          <div className="font-mono truncate">{model.id}</div>
          {model.display_name && (
            <div className="text-[10px] text-neutral-500 truncate">
              {model.display_name}
            </div>
          )}
        </button>

        {hasAnyCap && (
          <span className="flex items-center gap-1 shrink-0">
            {showToolsDisabled && (
              <CapChip
                icon={<Wrench size={10} />}
                label="no-tools"
                tooltip="已禁用工具调用"
                manual={toolsFromOverride}
                tone="deny"
              />
            )}
            {showReasoningDisabled && (
              <CapChip
                icon={<Brain size={10} />}
                label="no-think"
                tooltip="已禁用推理"
                manual={reasoningFromOverride}
                tone="deny"
              />
            )}
            {showVisionEnabled && (
              <CapChip
                icon={<Eye size={10} />}
                label="vision"
                tooltip="支持视觉输入"
                manual={visionFromOverride}
              />
            )}
            {showImageOutput && (
              <CapChip
                icon={<ImageIcon size={10} />}
                label="image-out"
                tooltip="生成图像"
                manual={imageOutputFromOverride}
              />
            )}
            {showEmbedding && (
              <CapChip
                icon={<Boxes size={10} />}
                label="embedding"
                tooltip="Embedding 模型,通常不能用于 chat"
                manual={embeddingFromOverride}
                tone="deny"
              />
            )}
            {hasProviderOptions && (
              <span
                title="存在自定义 Provider Options(JSON)"
                className="rounded px-1.5 py-0.5 text-[9px] font-mono bg-purple-950/40 border border-purple-900/60 text-purple-300"
              >
                {`{...}`}
              </span>
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
            {showMaxOutput && (
              <span
                title={`单次最大输出 ${effectiveMaxOutput!.toLocaleString()} tokens${
                  maxOutputFromOverride ? "(手动)" : ""
                }`}
                className={`rounded px-1.5 py-0.5 text-[9px] font-mono ${
                  maxOutputFromOverride
                    ? "bg-amber-950/40 border border-amber-900/60 text-amber-300"
                    : "bg-neutral-800 text-neutral-400"
                }`}
              >
                ⇡{formatContext(effectiveMaxOutput!)}
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

        {canDelete && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
            title="删除该手动添加的模型"
            className="shrink-0 p-0.5 rounded text-neutral-600 opacity-0 group-hover:opacity-100 hover:bg-neutral-700 hover:text-red-400 transition-all"
          >
            <Trash2 size={11} />
          </button>
        )}

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
  // Local draft so toggling chips / typing numbers is snappy. Hydrated
  // from the persisted override or — first time — seeded from the API
  // caps so the user sees the real values to nudge.
  const [draft, setDraft] = useState<ModelOverride>(
    override ?? {
      supports_tools: apiCaps.supports_tools ?? undefined,
      is_reasoning: apiCaps.is_reasoning ?? undefined,
      vision: apiCaps.vision ?? undefined,
      image_output: apiCaps.image_output ?? undefined,
      embedding: apiCaps.embedding ?? undefined,
      context_window: apiCaps.context_window ?? undefined,
      max_output_tokens: apiCaps.max_output_tokens ?? undefined,
      provider_options: undefined,
    },
  );

  // Free-form JSON textarea. Raw text held locally so the user can be
  // mid-edit (invalid JSON) without losing the draft. Validated only on
  // 保存 click.
  const [providerOptionsText, setProviderOptionsText] = useState(
    override?.provider_options
      ? JSON.stringify(override.provider_options, null, 2)
      : "",
  );
  const [providerOptionsError, setProviderOptionsError] = useState<
    string | null
  >(null);

  const isEmpty =
    draft.supports_tools === undefined &&
    draft.is_reasoning === undefined &&
    draft.vision === undefined &&
    draft.image_output === undefined &&
    draft.embedding === undefined &&
    draft.context_window === undefined &&
    draft.max_output_tokens === undefined &&
    (draft.provider_options === undefined ||
      Object.keys(draft.provider_options).length === 0);

  const handleSave = () => {
    let providerOptions: Record<string, unknown> | undefined;
    const trimmed = providerOptionsText.trim();
    if (trimmed) {
      try {
        const parsed = JSON.parse(trimmed);
        if (
          parsed === null ||
          typeof parsed !== "object" ||
          Array.isArray(parsed)
        ) {
          setProviderOptionsError("必须是 JSON 对象");
          return;
        }
        providerOptions =
          Object.keys(parsed).length > 0 ? parsed : undefined;
      } catch (e) {
        setProviderOptionsError(
          e instanceof Error ? e.message : "JSON 解析失败",
        );
        return;
      }
    }
    setProviderOptionsError(null);
    const next: ModelOverride = { ...draft, provider_options: providerOptions };
    const nowEmpty =
      next.supports_tools === undefined &&
      next.is_reasoning === undefined &&
      next.vision === undefined &&
      next.image_output === undefined &&
      next.embedding === undefined &&
      next.context_window === undefined &&
      next.max_output_tokens === undefined &&
      next.provider_options === undefined;
    onChange(nowEmpty ? null : next);
    onClose();
  };

  return (
    <Modal
      open
      onClose={onClose}
      widthClass="w-[560px] max-w-[calc(100vw-64px)]"
      heightClass="h-auto max-h-[calc(100vh-64px)]"
    >
      <div className="flex items-center gap-3 border-b border-neutral-800 px-5 py-3">
        <div className="flex-1 min-w-0">
          <div className="text-sm font-semibold">模型能力设置</div>
          <div className="font-mono text-[11px] text-neutral-500 truncate">
            {modelId}
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="text-neutral-500 hover:text-white p-1"
          title="关闭"
        >
          <XCircle size={16} />
        </button>
      </div>

      <div className="flex-1 overflow-auto px-5 py-4 space-y-5">
        <section className="space-y-3">
          <div className="text-[10px] uppercase tracking-wider text-neutral-500">
            能力
          </div>
          <div className="grid grid-cols-2 gap-3">
            {/* vision + tools 保留 TriState 因为这两个字段 provider API
                能可靠报告(OpenRouter `tools`/`supported_parameters`)，
                所以"自动"是有意义的——用户没自定义时直接吃 API 值。 */}
            <TriState
              label="工具调用 (Function Calling)"
              value={draft.supports_tools}
              onChange={(v) => setDraft({ ...draft, supports_tools: v })}
            />
            <TriState
              label="视觉输入 (Vision)"
              value={draft.vision}
              onChange={(v) => setDraft({ ...draft, vision: v })}
            />
            {/* 下面三个 API 不可靠报告（推理是 runtime 概念非 capability,
                image_output 和 embedding 是模型分类决策）。AMP/pi-mono
                都把它们当 binary 处理 — 用户必须自己点开/关，不留
                "auto"歧义。 */}
            <BiState
              label="推理 (Reasoning)"
              value={draft.is_reasoning ?? false}
              onChange={(v) => setDraft({ ...draft, is_reasoning: v })}
            />
            <BiState
              label="图像输出 (Image Output)"
              value={draft.image_output ?? false}
              onChange={(v) => setDraft({ ...draft, image_output: v })}
            />
            <BiState
              label="Embedding 模型"
              value={draft.embedding ?? false}
              onChange={(v) => setDraft({ ...draft, embedding: v })}
            />
          </div>
        </section>

        <section className="space-y-3">
          <div className="text-[10px] uppercase tracking-wider text-neutral-500">
            上限
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <div className="text-[10px] text-neutral-500 mb-1">
                上下文窗口 (tokens)
              </div>
              <input
                type="number"
                value={draft.context_window ?? ""}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  setDraft({
                    ...draft,
                    context_window:
                      e.target.value === "" || !Number.isFinite(n)
                        ? undefined
                        : n,
                  });
                }}
                placeholder="如 1000000 (留空=未知)"
                className="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1.5 text-xs outline-none font-mono focus:border-neutral-600"
              />
            </div>
            <div>
              <div className="text-[10px] text-neutral-500 mb-1">
                单次最大输出 (tokens)
              </div>
              <input
                type="number"
                value={draft.max_output_tokens ?? ""}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  setDraft({
                    ...draft,
                    max_output_tokens:
                      e.target.value === "" || !Number.isFinite(n)
                        ? undefined
                        : n,
                  });
                }}
                placeholder="留空=用 API 值或 32000 兜底"
                className="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1.5 text-xs outline-none font-mono focus:border-neutral-600"
              />
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <div className="text-[10px] uppercase tracking-wider text-neutral-500">
            Provider Options (JSON)
          </div>
          <textarea
            value={providerOptionsText}
            onChange={(e) => {
              setProviderOptionsText(e.target.value);
              if (providerOptionsError) setProviderOptionsError(null);
            }}
            placeholder={`{\n  \n}`}
            spellCheck={false}
            rows={5}
            className={`w-full rounded bg-neutral-900 border px-2 py-1.5 text-xs outline-none font-mono resize-y focus:border-neutral-600 ${
              providerOptionsError
                ? "border-red-700"
                : "border-neutral-800"
            }`}
          />
          {providerOptionsError && (
            <div className="text-[10px] text-red-400 font-mono">
              {providerOptionsError}
            </div>
          )}
          <div className="text-[10px] text-neutral-500">
            示例:{" "}
            <code className="font-mono text-neutral-400">
              {`{ "thinking": { "type": "disabled" } }`}
            </code>
            {" — 关闭 doubao-seed-1.8 等模型的推理。"}
          </div>
        </section>
      </div>

      <div className="flex items-center justify-end gap-2 border-t border-neutral-800 px-5 py-3 bg-neutral-950">
        <button
          type="button"
          onClick={() => {
            onChange(null);
            onClose();
          }}
          className="inline-flex items-center gap-1 text-[11px] text-neutral-400 hover:text-white px-2 py-1.5 mr-auto"
        >
          <RotateCcw size={11} />
          清除全部覆盖
        </button>
        <button
          type="button"
          onClick={onClose}
          className="text-[11px] text-neutral-400 hover:text-white px-3 py-1.5"
        >
          取消
        </button>
        <button
          type="button"
          onClick={handleSave}
          disabled={isEmpty && !providerOptionsText.trim()}
          className="rounded bg-blue-700 hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed px-4 py-1.5 text-[11px] font-medium text-white"
        >
          保存
        </button>
      </div>
    </Modal>
  );
}

/** Three-state toggle: unknown / false / true. */
/**
 * Three-state segmented toggle for model capability overrides.
 *
 * Layout: small label + a single rounded pill containing 3 segments.
 * The active segment fills with a soft blue, gets a subtle inner ring,
 * and bumps slightly. Inactive segments are flat / muted. Each segment
 * pairs an icon with one short word so the eye lands on shape first
 * and reads label second — much less wall-of-text than the previous
 * 三按钮等宽 layout.
 *
 *   undefined → 自动   (RotateCcw — "let the system decide")
 *   false     → 关     (X)
 *   true      → 开     (Check)
 */
function TriState({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean | undefined;
  onChange: (v: boolean | undefined) => void;
}) {
  type Segment = {
    v: boolean | undefined;
    icon: ReactNode;
    text: string;
    activeTone: string;
  };
  const segments: Segment[] = [
    {
      v: undefined,
      icon: <RotateCcw size={11} strokeWidth={2.25} />,
      text: "自动",
      activeTone:
        "bg-neutral-700/80 text-neutral-100 ring-1 ring-inset ring-neutral-500/40",
    },
    {
      v: false,
      icon: <XCircle size={11} strokeWidth={2.25} />,
      text: "关",
      activeTone:
        "bg-rose-900/50 text-rose-100 ring-1 ring-inset ring-rose-700/60",
    },
    {
      v: true,
      icon: <Check size={11} strokeWidth={2.5} />,
      text: "开",
      activeTone:
        "bg-blue-700/70 text-white ring-1 ring-inset ring-blue-500/60",
    },
  ];

  return (
    <div>
      <div className="text-[10px] text-neutral-500 mb-1.5">{label}</div>
      <div className="inline-flex w-full p-0.5 rounded-md bg-neutral-900/80 border border-neutral-800">
        {segments.map((seg) => {
          const active = value === seg.v;
          return (
            <button
              key={String(seg.v)}
              type="button"
              onClick={() => onChange(seg.v)}
              className={`flex-1 inline-flex items-center justify-center gap-1 text-[11px] py-1 rounded transition-all ${
                active
                  ? seg.activeTone
                  : "text-neutral-500 hover:text-neutral-200"
              }`}
            >
              {seg.icon}
              <span>{seg.text}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Two-state segmented toggle. Same visual family as `TriState` minus
 * the "auto" segment. Used for capabilities where the API can't tell
 * us reliably (推理 is a runtime mode, image_output / embedding are
 * model-class decisions) — the user picks 关 or 开 explicitly.
 *
 *   false → 关 (XCircle, rose tint when active)
 *   true  → 开 (Check,   blue tint when active)
 */
function BiState({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  type Segment = {
    v: boolean;
    icon: ReactNode;
    text: string;
    activeTone: string;
  };
  const segments: Segment[] = [
    {
      v: false,
      icon: <XCircle size={11} strokeWidth={2.25} />,
      text: "关",
      activeTone:
        "bg-rose-900/50 text-rose-100 ring-1 ring-inset ring-rose-700/60",
    },
    {
      v: true,
      icon: <Check size={11} strokeWidth={2.5} />,
      text: "开",
      activeTone:
        "bg-blue-700/70 text-white ring-1 ring-inset ring-blue-500/60",
    },
  ];
  return (
    <div>
      <div className="text-[10px] text-neutral-500 mb-1.5">{label}</div>
      <div className="inline-flex w-full p-0.5 rounded-md bg-neutral-900/80 border border-neutral-800">
        {segments.map((seg) => {
          const active = value === seg.v;
          return (
            <button
              key={String(seg.v)}
              type="button"
              onClick={() => onChange(seg.v)}
              className={`flex-1 inline-flex items-center justify-center gap-1 text-[11px] py-1 rounded transition-all ${
                active
                  ? seg.activeTone
                  : "text-neutral-500 hover:text-neutral-200"
              }`}
            >
              {seg.icon}
              <span>{seg.text}</span>
            </button>
          );
        })}
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
      <div className="rounded border border-green-900/60 bg-green-950/30 text-green-300 px-3 py-2 text-xs">
        <div className="flex items-center gap-2 mb-1">
          <CheckCircle2 size={14} />
          <span>连接成功</span>
          <span className="text-green-500/70">· {result.latency_ms}ms</span>
          {result.model && (
            <span className="text-green-500/70">· {result.model}</span>
          )}
          {(result.input_tokens !== null || result.output_tokens !== null) && (
            <span className="text-green-500/70">
              · in {result.input_tokens ?? "?"} / out{" "}
              {result.output_tokens ?? "?"}
            </span>
          )}
        </div>
        {result.sample_response && (
          <div className="font-mono text-[11px] text-green-300/80 mt-1 border-l-2 border-green-700/50 pl-2">
            {result.sample_response}
          </div>
        )}
      </div>
    );
  }
  return (
    <div className="rounded border border-red-900/60 bg-red-950/30 text-red-200 px-3 py-2 text-xs">
      <div className="flex items-center gap-2 mb-1">
        <XCircle size={14} />
        <span>连接失败</span>
        {result.model && (
          <span className="text-red-400/80">· {result.model}</span>
        )}
        {result.latency_ms > 0 && (
          <span className="text-red-400/80">· {result.latency_ms}ms</span>
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
                  <button
                    type="button"
                    key={`${s.source_dir}::${s.name}`}
                    onClick={() => toggleSkill(s.name)}
                    className="w-full text-left flex items-start gap-2 px-3 py-2 cursor-pointer hover:bg-neutral-800/50 text-xs"
                  >
                    <span
                      className={`mt-0.5 shrink-0 w-3.5 h-3.5 rounded-[3px] border flex items-center justify-center transition-colors ${
                        on
                          ? "bg-blue-600 border-blue-600"
                          : "border-neutral-600"
                      }`}
                    >
                      {on && (
                        <Check
                          size={9}
                          strokeWidth={3.5}
                          className="text-white"
                        />
                      )}
                    </span>
                    <span className="flex-1 min-w-0">
                      <span className="block font-medium truncate">
                        {s.name}
                      </span>
                      <span className="block text-neutral-500 truncate">
                        {s.description}
                      </span>
                    </span>
                  </button>
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
