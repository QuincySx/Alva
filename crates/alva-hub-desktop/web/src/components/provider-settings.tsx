// INPUT:  provider profiles, backend model routes/default target, save callbacks, shared UI controls
// OUTPUT: Cindy-style provider card management, clean credential editing, model discovery and route controls
// POS:    Settings domain surface for model providers, model discovery, and default route binding

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  Check,
  Eye,
  EyeOff,
  FlaskConical,
  KeyRound,
  Plus,
  RefreshCw,
  Search,
  ServerCog,
  Trash2,
} from "lucide-react";
import type {
  DiscoveredProviderModel,
  EngineTarget,
  ModelRoute,
  ProviderDiagnosticResult,
  ProviderModel,
  ProviderProfile,
  SaveProviderProfile,
} from "../bridge";
import { localizeSystemText, useI18n } from "../i18n";
import {
  Badge,
  Button,
  DialogContent,
  DialogRoot,
  DialogTrigger,
  EmptyState,
  FieldFrame,
  Notice,
  PanelSection,
  SelectField,
  Switch,
  TextField,
} from "../ui";

const DISCOVERY_PREVIEW_LIMIT = 100;

interface ProviderPreset {
  id: string;
  name: string;
  kind: "openai" | "anthropic" | "gemini";
  baseUrl: string;
  placeholderKey: string;
  defaultModels: Array<{ id: string; name: string }>;
}

const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: "openai",
    name: "OpenAI",
    kind: "openai",
    baseUrl: "https://api.openai.com/v1",
    placeholderKey: "sk-proj-...",
    defaultModels: [
      { id: "gpt-4o", name: "GPT-4o" },
      { id: "gpt-4o-mini", name: "GPT-4o mini" },
      { id: "o1", name: "o1" },
      { id: "o3-mini", name: "o3-mini" },
    ],
  },
  {
    id: "anthropic",
    name: "Anthropic",
    kind: "anthropic",
    baseUrl: "https://api.anthropic.com",
    placeholderKey: "sk-ant-...",
    defaultModels: [
      { id: "claude-3-7-sonnet-20250219", name: "Claude 3.7 Sonnet" },
      { id: "claude-3-5-sonnet-20241022", name: "Claude 3.5 Sonnet" },
      { id: "claude-3-5-haiku-20241022", name: "Claude 3.5 Haiku" },
    ],
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    kind: "openai",
    baseUrl: "https://api.deepseek.com/v1",
    placeholderKey: "sk-...",
    defaultModels: [
      { id: "deepseek-chat", name: "DeepSeek-V3" },
      { id: "deepseek-reasoner", name: "DeepSeek-R1" },
    ],
  },
  {
    id: "siliconflow",
    name: "SiliconFlow (硅基流动)",
    kind: "openai",
    baseUrl: "https://api.siliconflow.cn/v1",
    placeholderKey: "sk-...",
    defaultModels: [
      { id: "deepseek-ai/DeepSeek-V3", name: "DeepSeek-V3" },
      { id: "deepseek-ai/DeepSeek-R1", name: "DeepSeek-R1" },
      { id: "Qwen/Qwen2.5-72B-Instruct", name: "Qwen 2.5 72B" },
    ],
  },
  {
    id: "ollama",
    name: "Ollama (本地部署)",
    kind: "openai",
    baseUrl: "http://127.0.0.1:11434/v1",
    placeholderKey: "ollama (不需要密钥)",
    defaultModels: [
      { id: "qwen2.5-coder:32b", name: "Qwen 2.5 Coder 32B" },
      { id: "deepseek-r1:32b", name: "DeepSeek-R1 32B" },
      { id: "llama3.3:70b", name: "Llama 3.3 70B" },
    ],
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    kind: "openai",
    baseUrl: "https://openrouter.ai/api/v1",
    placeholderKey: "sk-or-...",
    defaultModels: [
      { id: "anthropic/claude-3.7-sonnet", name: "Claude 3.7 Sonnet" },
      { id: "deepseek/deepseek-r1", name: "DeepSeek R1" },
      { id: "openai/gpt-4o", name: "GPT-4o" },
    ],
  },
];

export function ProviderSettings({
  providers,
  routes,
  defaultTarget,
  loading,
  onSave,
  onSetDefault,
  onDiscover,
  onTestRoute,
}: {
  providers: ProviderProfile[];
  routes: ModelRoute[];
  defaultTarget: EngineTarget | null;
  loading: boolean;
  onSave: (provider: SaveProviderProfile) => Promise<void>;
  onSetDefault: (target: EngineTarget) => Promise<void>;
  onDiscover: (providerId: string) => Promise<ProviderDiagnosticResult>;
  onTestRoute: (target: EngineTarget) => Promise<ProviderDiagnosticResult>;
}) {
  const { locale, localize, t } = useI18n();
  const localizeDiagnostic = (message: string) =>
    localizeSystemText(locale, message, t("system.diagnosticUnavailable"));

  const [selectedId, setSelectedId] = useState<string>("");
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<ProviderProfile | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [clearApiKey, setClearApiKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [settingDefault, setSettingDefault] = useState<string | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [discoveredModels, setDiscoveredModels] = useState<DiscoveredProviderModel[]>([]);
  const [testingRoute, setTestingRoute] = useState<string | null>(null);
  const [diagnostic, setDiagnostic] = useState<{ ok: boolean; heading: string; message: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [addModalOpen, setAddModalOpen] = useState(false);

  // New Provider Draft State
  const [newPreset, setNewPreset] = useState("openai");
  const [newCustomName, setNewCustomName] = useState("");
  const [newEndpoint, setNewEndpoint] = useState("https://api.openai.com/v1");
  const [newApiKey, setNewApiKey] = useState("");
  const [addingProvider, setAddingProvider] = useState(false);

  const discoveryToken = useRef(0);
  const draftRef = useRef<ProviderProfile | null>(null);
  draftRef.current = draft;

  const activeProvider = useMemo(() => {
    if (!providers || providers.length === 0) return null;
    return providers.find((p) => p.id === selectedId) ?? providers[0];
  }, [providers, selectedId]);

  useEffect(() => {
    if (activeProvider && activeProvider.id !== draft?.id) {
      setSelectedId(activeProvider.id);
      setDraft(structuredClone(activeProvider));
      setApiKey("");
      setClearApiKey(false);
      setShowPassword(false);
      setDiscoveredModels([]);
      setDiagnostic(null);
      setError(null);
    }
  }, [activeProvider, draft?.id]);

  const visibleModels = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!draft || !needle) return draft?.models ?? [];
    return draft.models.filter((model) =>
      `${model.display_name} ${model.id}`.toLowerCase().includes(needle),
    );
  }, [draft, query]);

  const matchingDiscoveredModels = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return discoveredModels;
    return discoveredModels.filter((model) =>
      `${model.display_name ?? ""} ${model.id}`.toLowerCase().includes(needle),
    );
  }, [discoveredModels, query]);
  const visibleDiscoveredModels = matchingDiscoveredModels.slice(0, DISCOVERY_PREVIEW_LIMIT);

  function updateModel(id: string, update: Partial<ProviderModel>) {
    setDraft((current) =>
      current
        ? {
            ...current,
            models: current.models.map((model) =>
              model.id === id ? { ...model, ...update } : model,
            ),
          }
        : current,
    );
  }

  function addModel() {
    if (!draft) return;
    const id = `model-${draft.models.length + 1}`;
    setDraft({
      ...draft,
      models: [
        ...draft.models,
        {
          id,
          display_name: t("provider.newModel"),
          enabled: true,
          registry_id: null,
          context_window: null,
          capabilities: [],
          supported_engines: [...draft.supported_engines],
        },
      ],
    });
  }

  async function setDefaultRoute(route: ModelRoute) {
    const identity = targetKey(route.target);
    setError(null);
    setSettingDefault(identity);
    try {
      await onSetDefault(route.target);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSettingDefault(null);
    }
  }

  async function discoverModels() {
    if (!draft) return;
    const providerId = draft.id;
    const token = ++discoveryToken.current;
    setError(null);
    setDiagnostic(null);
    setDiscovering(true);
    try {
      const result = await onDiscover(providerId);
      if (token !== discoveryToken.current || draftRef.current?.id !== providerId) return;
      if (!result.ok) {
        setDiagnostic({
          ok: false,
          heading: t("provider.discoveryFailed"),
          message: localizeDiagnostic(result.message),
        });
        return;
      }
      const current = draftRef.current;
      if (!current) return;
      const candidates = discoveryCandidates(current, result.models);
      setDiscoveredModels(candidates);
      setDiagnostic({
        ok: true,
        heading: t("provider.discoveryCompleted"),
        message: t(
          candidates.length === 1
            ? "provider.discoverySummaryOne"
            : "provider.discoverySummaryMany",
          { found: result.models.length, added: candidates.length },
        ),
      });
    } catch (cause) {
      if (token === discoveryToken.current && draftRef.current?.id === providerId) {
        setDiagnostic({
          ok: false,
          heading: t("provider.discoveryFailed"),
          message: cause instanceof Error ? cause.message : String(cause),
        });
      }
    } finally {
      if (token === discoveryToken.current) setDiscovering(false);
    }
  }

  function addDiscovered(candidates: DiscoveredProviderModel[]) {
    if (candidates.length === 0) return;
    setDiagnostic(null);
    setDraft((current) =>
      current
        ? {
            ...current,
            models: appendDiscoveredModels(current, candidates),
          }
        : current,
    );
    const addedIds = new Set(candidates.map((candidate) => candidate.id.trim()));
    setDiscoveredModels((current) =>
      current.filter((candidate) => !addedIds.has(candidate.id.trim())),
    );
  }

  async function testRoute(route: ModelRoute) {
    const identity = targetKey(route.target);
    setError(null);
    setDiagnostic(null);
    setTestingRoute(identity);
    try {
      const result = await onTestRoute(route.target);
      setDiagnostic({
        ok: result.ok,
        heading: result.ok ? t("provider.testPassed") : t("provider.testFailed"),
        message: result.ok
          ? t("provider.testSummary", { latency: result.latency_ms })
          : localizeDiagnostic(result.message),
      });
    } catch (cause) {
      setDiagnostic({
        ok: false,
        heading: t("provider.testFailed"),
        message: cause instanceof Error ? cause.message : String(cause),
      });
    } finally {
      setTestingRoute(null);
    }
  }

  async function save() {
    if (!draft || draft.managed) return;
    const nextKeyConfigured = clearApiKey
      ? false
      : apiKey.length > 0 || draft.api_key_configured;
    setSaving(true);
    setError(null);
    try {
      await onSave({
        id: draft.id,
        display_name: draft.display_name,
        base_url: draft.base_url,
        enabled: draft.enabled,
        api_key: clearApiKey ? null : apiKey || null,
        clear_api_key: clearApiKey,
        max_output_tokens: draft.max_output_tokens,
        models: draft.models,
      });
      setDraft((current) =>
        current?.id === draft.id
          ? {
              ...current,
              api_key_configured: nextKeyConfigured,
            }
          : current,
      );
      setApiKey("");
      setClearApiKey(false);
      setDiscoveredModels([]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateNewProvider() {
    const preset = PROVIDER_PRESETS.find((p) => p.id === newPreset);
    const id = newPreset === "custom" ? `custom-${Date.now()}` : `${newPreset}-${Date.now()}`;
    const name = newPreset === "custom" ? (newCustomName.trim() || "Custom Provider") : (preset?.name ?? "Provider");
    const baseUrl = newPreset === "custom" ? newEndpoint.trim() : (preset?.baseUrl ?? newEndpoint.trim());
    const models = preset
      ? preset.defaultModels.map((m) => ({
          id: m.id,
          display_name: m.name,
          enabled: true,
          registry_id: null,
          context_window: null,
          capabilities: [],
          supported_engines: ["alva", "claude", "codex", "pi"],
        }))
      : [
          {
            id: "default-model",
            display_name: "Default Model",
            enabled: true,
            registry_id: null,
            context_window: null,
            capabilities: [],
            supported_engines: ["alva", "claude", "codex", "pi"],
          },
        ];

    setAddingProvider(true);
    try {
      await onSave({
        id,
        display_name: name,
        base_url: baseUrl,
        enabled: true,
        api_key: newApiKey.trim() ? newApiKey.trim() : null,
        clear_api_key: false,
        max_output_tokens: 8192,
        models,
      });
      setAddModalOpen(false);
      setSelectedId(id);
      setNewApiKey("");
      setNewCustomName("");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAddingProvider(false);
    }
  }

  if (loading && providers.length === 0) {
    return <EmptyState title={t("provider.loading")} description={t("provider.loadingDescription")} />;
  }
  if (!draft) {
    return <EmptyState title={t("provider.none")} description={t("provider.noneDescription")} />;
  }

  const isReady =
    routes.some(
      (route) => route.target.provider_id === draft.id && route.available,
    ) && (!draft.requires_api_key || draft.api_key_configured);

  return (
    <div className="hub-provider-studio">
      {/* ── Top Provider Cards Flow (No Nested Sidebar) ────────────────── */}
      <div className="hub-provider-grid-header">
        <div className="hub-provider-grid-header__info">
          <strong style={{ fontSize: "14px", fontWeight: 600 }}>
            {locale === "zh-CN" ? "供应商列表" : "Configured Providers"}
          </strong>
          <span style={{ fontSize: "12px", color: "var(--color-text-subtle)" }}>
            {locale === "zh-CN"
              ? "点击选择供应商卡片以查看/编辑模型与连接密钥"
              : "Select a provider card to manage endpoints and API keys"}
          </span>
        </div>
        <DialogRoot open={addModalOpen} onOpenChange={setAddModalOpen}>
          <DialogTrigger asChild>
            <Button leadingIcon={<Plus aria-hidden="true" size={15} />} variant="primary">
              {locale === "zh-CN" ? "＋ 添加供应商" : "Add Provider"}
            </Button>
          </DialogTrigger>
          <DialogContent
            title={locale === "zh-CN" ? "🏢 添加模型供应商" : "Add Model Provider"}
            description={
              locale === "zh-CN"
                ? "从常用厂商预设快速配置，或添加兼容 OpenAI/Anthropic 协议的自定义端点。"
                : "Choose a provider template or configure a custom API endpoint."
            }
          >
            <div className="hub-form-stack" style={{ marginTop: "12px" }}>
              <FieldFrame label={locale === "zh-CN" ? "选择厂商预设" : "Provider Preset"}>
                <SelectField
                  ariaLabel="Provider Preset"
                  value={newPreset}
                  onValueChange={(val) => {
                    setNewPreset(val);
                    const p = PROVIDER_PRESETS.find((item) => item.id === val);
                    if (p) setNewEndpoint(p.baseUrl);
                  }}
                  options={[
                    ...PROVIDER_PRESETS.map((p) => ({ value: p.id, label: p.name })),
                    { value: "custom", label: locale === "zh-CN" ? "🛠 自定义端点 (Custom API)" : "Custom API Endpoint" },
                  ]}
                />
              </FieldFrame>

              {newPreset === "custom" ? (
                <>
                  <FieldFrame label={locale === "zh-CN" ? "供应商名称" : "Provider Name"}>
                    <TextField
                      placeholder="e.g. My Private vLLM"
                      value={newCustomName}
                      onChange={(e) => setNewCustomName(e.target.value)}
                    />
                  </FieldFrame>
                  <FieldFrame label={locale === "zh-CN" ? "API Base URL 端点" : "Base URL"}>
                    <TextField
                      placeholder="https://api.example.com/v1"
                      value={newEndpoint}
                      onChange={(e) => setNewEndpoint(e.target.value)}
                    />
                  </FieldFrame>
                </>
              ) : null}

              <FieldFrame
                label={locale === "zh-CN" ? "API 访问密钥 (API Key)" : "API Key"}
                description={
                  newPreset === "ollama"
                    ? (locale === "zh-CN" ? "Ollama 本地实例通常无需密钥" : "Local instance doesn't require a key")
                    : (locale === "zh-CN" ? "密钥加密保存在本地设备，不会泄露" : "Key is stored securely on this device")
                }
              >
                <TextField
                  type="password"
                  placeholder={
                    PROVIDER_PRESETS.find((p) => p.id === newPreset)?.placeholderKey ?? "sk-..."
                  }
                  value={newApiKey}
                  onChange={(e) => setNewApiKey(e.target.value)}
                />
              </FieldFrame>

              <div className="hub-dialog-actions" style={{ marginTop: "16px" }}>
                <Button variant="ghost" onClick={() => setAddModalOpen(false)}>
                  {t("common.cancel")}
                </Button>
                <Button
                  variant="primary"
                  disabled={addingProvider}
                  onClick={() => void handleCreateNewProvider()}
                >
                  {addingProvider
                    ? (locale === "zh-CN" ? "保存中…" : "Saving…")
                    : (locale === "zh-CN" ? "✓ 确认添加" : "Add Provider")}
                </Button>
              </div>
            </div>
          </DialogContent>
        </DialogRoot>
      </div>

      {/* ── Provider Cards Strip (Flat, High-Density Cards) ────────────── */}
      <div className="hub-provider-cards-strip">
        {providers.map((p) => {
          const isSelected = p.id === draft.id;
          const hasReadyRoute =
            routes.some((r) => r.target.provider_id === p.id && r.available) &&
            (!p.requires_api_key || p.api_key_configured);
          return (
            <button
              type="button"
              key={p.id}
              onClick={() => {
                discoveryToken.current += 1;
                setDiscovering(false);
                setDiscoveredModels([]);
                setSelectedId(p.id);
                setDraft(structuredClone(p));
                setApiKey("");
                setClearApiKey(false);
                setDiagnostic(null);
              }}
              className={`hub-provider-tab-card ${isSelected ? "hub-provider-tab-card--active" : ""}`}
            >
              <div className="hub-provider-tab-card__header">
                <span
                  className={`hub-provider-dot ${hasReadyRoute ? "hub-provider-dot--ready" : ""}`}
                />
                <strong>{p.display_name}</strong>
              </div>
              <div className="hub-provider-tab-card__meta">
                <span>{t("provider.modelCount", { count: p.models.length })}</span>
                {p.managed ? (
                  <Badge tone="neutral">{locale === "zh-CN" ? "系统托管" : "Managed"}</Badge>
                ) : hasReadyRoute ? (
                  <Badge tone="success">{t("provider.available")}</Badge>
                ) : (
                  <Badge tone="warning">{t("provider.setupNeeded")}</Badge>
                )}
              </div>
            </button>
          );
        })}
      </div>

      {/* ── Active Provider Detail Surface ─────────────────────────────── */}
      <div className="hub-provider-active-surface">
        {error ? (
          <Notice
            heading={t("provider.settingsNotSaved")}
            description={localizeDiagnostic(error)}
            tone="danger"
          />
        ) : null}
        {diagnostic ? (
          <Notice
            heading={diagnostic.heading}
            description={
              diagnostic.ok ? diagnostic.message : localizeDiagnostic(diagnostic.message)
            }
            tone={diagnostic.ok ? "info" : "danger"}
          />
        ) : null}

        {/* Header Summary */}
        <header className="hub-provider-surface-header">
          <div className="hub-provider-surface-header__title">
            <span className="hub-provider-surface-header__icon">
              <ServerCog aria-hidden="true" size={22} />
            </span>
            <div>
              <h2>{draft.display_name}</h2>
              <p>{t("provider.compatible", { engines: draft.supported_engines.join(" · ") })}</p>
            </div>
          </div>
          <div className="hub-provider-surface-header__actions">
            <Badge tone={isReady ? "success" : "warning"}>
              {isReady ? t("provider.available") : t("provider.setupNeeded")}
            </Badge>
            {!draft.managed ? (
              <Switch
                checked={draft.enabled}
                label={t("provider.enabled")}
                onCheckedChange={(checked) =>
                  setDraft({ ...draft, enabled: checked === true })
                }
              />
            ) : null}
          </div>
        </header>

        {/* Credentials & Connection Section */}
        {draft.managed ? (
          <Notice
            heading={t("provider.managed")}
            description={t("provider.managedDescription")}
            tone="info"
          />
        ) : (
          <PanelSection
            heading={t("provider.connection")}
            description={t("provider.connectionDescription")}
          >
            <div className="hub-provider-form-grid">
              {draft.endpoint_editable ? (
                <FieldFrame label={t("provider.endpoint")}>
                  <TextField
                    value={draft.base_url}
                    onChange={(e) => setDraft({ ...draft, base_url: e.target.value })}
                  />
                </FieldFrame>
              ) : null}

              {draft.requires_api_key ? (
                <FieldFrame
                  label={t("provider.apiKey")}
                  description={
                    clearApiKey
                      ? t("provider.keyWillClear")
                      : draft.api_key_configured
                        ? t("provider.keySaved")
                        : t("provider.keyRequired")
                  }
                >
                  <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
                    <TextField
                      aria-label={t("provider.apiKey")}
                      autoComplete="off"
                      placeholder={
                        clearApiKey
                          ? t("provider.noKeySaved")
                          : draft.api_key_configured
                            ? t("provider.savedSecurely")
                            : t("provider.pasteKey")
                      }
                      type={showPassword ? "text" : "password"}
                      value={apiKey}
                      onChange={(e) => {
                        setApiKey(e.target.value);
                        if (e.target.value.length > 0) setClearApiKey(false);
                      }}
                    />
                    <Button
                      variant="ghost"
                      size="small"
                      onClick={() => setShowPassword((v) => !v)}
                      title={showPassword ? "Hide password" : "Show password"}
                    >
                      {showPassword ? <EyeOff size={15} /> : <Eye size={15} />}
                    </Button>
                    {draft.api_key_configured ? (
                      <Button
                        onClick={() => {
                          setApiKey("");
                          setClearApiKey((current) => !current);
                        }}
                        size="small"
                        variant="ghost"
                      >
                        {clearApiKey ? t("provider.keepKey") : t("provider.clearKey")}
                      </Button>
                    ) : null}
                  </div>
                </FieldFrame>
              ) : (
                <Notice
                  heading={t("provider.localSubscription")}
                  description={t("provider.localSubscriptionDescription")}
                  tone="neutral"
                />
              )}
            </div>
          </PanelSection>
        )}

        {/* Models Management Section */}
        <PanelSection
          actions={
            !draft.managed ? (
              <div className="hub-provider-toolbar-actions">
                <Button
                  disabled={
                    discovering ||
                    saving ||
                    !draft.enabled ||
                    (draft.requires_api_key && (!draft.api_key_configured || clearApiKey))
                  }
                  leadingIcon={<RefreshCw size={14} />}
                  onClick={() => void discoverModels()}
                  size="small"
                  title={
                    !draft.api_key_configured && draft.requires_api_key
                      ? t("provider.saveCredentialsFirst")
                      : undefined
                  }
                  variant="ghost"
                >
                  {discovering ? t("provider.discovering") : t("provider.discover")}
                </Button>
                <Button
                  leadingIcon={<Plus size={14} />}
                  onClick={addModel}
                  size="small"
                  variant="ghost"
                >
                  {t("provider.addModel")}
                </Button>
              </div>
            ) : undefined
          }
          description={t("provider.modelsDescription")}
          heading={t("provider.models")}
          meta={t("provider.enabledCount", {
            count: draft.models.filter((model) => model.enabled).length,
          })}
        >
          {/* Search bar */}
          <label className="hub-provider-search-bar">
            <Search aria-hidden="true" size={16} />
            <TextField
              aria-label={t("provider.searchModels")}
              placeholder={t("modelPicker.searchPlaceholder")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>

          {/* Staged Discovered Models Notice */}
          {discoveredModels.length > 0 ? (
            <Notice
              actions={
                <Button
                  onClick={() => addDiscovered(discoveredModels)}
                  size="small"
                  variant="secondary"
                >
                  {t("provider.addAllDiscovered")}
                </Button>
              }
              description={t("provider.discoveryReviewDescription", {
                count: discoveredModels.length,
              })}
              heading={t("provider.discoveryReview")}
              tone="info"
            >
              <div className="hub-provider-models-list">
                {visibleDiscoveredModels.map((model) => {
                  const label = model.display_name?.trim() || model.id;
                  return (
                    <div className="hub-provider-model-row" key={model.id}>
                      <div className="hub-provider-model-row__identity">
                        <span>
                          <KeyRound aria-hidden="true" size={16} />
                        </span>
                        <div>
                          <strong>{label}</strong>
                          <small>{model.id}</small>
                        </div>
                      </div>
                      <div className="hub-provider-model-row__controls">
                        <Badge tone={model.suggested_registry_id ? "accent" : "neutral"}>
                          {model.suggested_registry_id
                            ? t("provider.recognizedModel")
                            : t("provider.customModel")}
                        </Badge>
                        {model.compatible_engines.map((engine) => (
                          <Badge key={engine}>{engine}</Badge>
                        ))}
                        <Button
                          aria-label={t("provider.addDiscoveredAria", { model: label })}
                          onClick={() => addDiscovered([model])}
                          size="small"
                          variant="ghost"
                        >
                          {t("provider.addDiscovered")}
                        </Button>
                      </div>
                    </div>
                  );
                })}
              </div>
              {matchingDiscoveredModels.length > visibleDiscoveredModels.length ? (
                <small>
                  {t("provider.discoveryLimited", {
                    shown: DISCOVERY_PREVIEW_LIMIT,
                    count: matchingDiscoveredModels.length,
                  })}
                </small>
              ) : null}
            </Notice>
          ) : null}

          {/* Model Rows */}
          <div className="hub-provider-models-list">
            {visibleModels.length === 0 ? (
              <EmptyState
                title={t("provider.noModels")}
                description={t("provider.noModelsDescription")}
              />
            ) : (
              visibleModels.map((model) => (
                <div className="hub-provider-model-row" key={model.id}>
                  <div className="hub-provider-model-row__identity">
                    <span className="hub-provider-model-row__icon">
                      <Bot aria-hidden="true" size={16} />
                    </span>
                    {draft.managed ? (
                      <div className="hub-provider-model-row__titles">
                        <strong>{model.display_name}</strong>
                        <small>{model.id}</small>
                      </div>
                    ) : (
                      <div className="hub-provider-model-row__fields">
                        <TextField
                          aria-label={t("provider.modelDisplayName")}
                          value={model.display_name}
                          onChange={(e) =>
                            updateModel(model.id, { display_name: e.target.value })
                          }
                        />
                        <TextField
                          aria-label={t("provider.modelId")}
                          value={model.id}
                          onChange={(e) =>
                            updateModel(model.id, { id: e.target.value, registry_id: null })
                          }
                        />
                      </div>
                    )}
                  </div>
                  <div className="hub-provider-model-row__controls">
                    <Badge tone={model.registry_id ? "accent" : "neutral"}>
                      {model.registry_id
                        ? t("provider.recognizedModel")
                        : t("provider.customModel")}
                    </Badge>
                    {model.enabled ? <Check aria-hidden="true" size={15} /> : null}
                    {!draft.managed ? (
                      <Switch
                        checked={model.enabled}
                        label={`${model.display_name} · ${t("provider.active")}`}
                        onCheckedChange={(checked) =>
                          updateModel(model.id, { enabled: checked === true })
                        }
                      />
                    ) : (
                      <Badge tone="success">{t("provider.active")}</Badge>
                    )}
                    {routes
                      .filter(
                        (route) =>
                          route.target.provider_id === draft.id &&
                          route.target.model_id === model.id,
                      )
                      .map((route, routeIndex) => {
                        const identity = targetKey(route.target);
                        const isDefault =
                          defaultTarget !== null && identity === targetKey(defaultTarget);
                        return (
                          <div className="hub-provider-route-actions" key={identity}>
                            <Badge>{route.engine_label}</Badge>
                            {!draft.managed && routeIndex === 0 ? (
                              <Button
                                aria-label={`${t("provider.test")} ${model.display_name}`}
                                disabled={!route.available || testingRoute !== null}
                                leadingIcon={<FlaskConical size={14} />}
                                onClick={() => void testRoute(route)}
                                size="small"
                                title={
                                  route.available
                                    ? undefined
                                    : localize(route.unavailable_reason) ||
                                      t("provider.setupNeeded")
                                }
                                variant="ghost"
                              >
                                {testingRoute === identity
                                  ? t("provider.testing")
                                  : t("provider.test")}
                              </Button>
                            ) : null}
                            {isDefault ? (
                              <Badge tone="success">
                                {t("provider.default", { engine: route.engine_label })}
                              </Badge>
                            ) : (
                              <Button
                                aria-label={t("provider.setDefaultAria", {
                                  engine: route.engine_label,
                                  model: model.display_name,
                                })}
                                disabled={!route.available || settingDefault !== null}
                                onClick={() => void setDefaultRoute(route)}
                                size="small"
                                title={
                                  route.available
                                    ? undefined
                                    : localize(route.unavailable_reason) ||
                                      t("provider.setupNeeded")
                                }
                                variant="ghost"
                              >
                                {settingDefault === identity
                                  ? t("provider.settingDefault")
                                  : t("provider.setDefault")}
                              </Button>
                            )}
                          </div>
                        );
                      })}
                    {!draft.managed ? (
                      <Button
                        aria-label={t("provider.removeModel", { model: model.display_name })}
                        onClick={() =>
                          setDraft({
                            ...draft,
                            models: draft.models.filter((candidate) => candidate !== model),
                          })
                        }
                        size="small"
                        variant="ghost"
                      >
                        <Trash2 aria-hidden="true" size={14} />
                      </Button>
                    ) : null}
                  </div>
                </div>
              ))
            )}
          </div>
        </PanelSection>

        {/* Save Footer */}
        {!draft.managed ? (
          <footer className="hub-provider-footer">
            <Button
              disabled={saving}
              onClick={() => void save()}
              variant="primary"
            >
              {saving ? t("provider.saving") : t("provider.save")}
            </Button>
          </footer>
        ) : null}
      </div>
    </div>
  );
}

function discoveryCandidates(
  provider: ProviderProfile,
  discovered: DiscoveredProviderModel[],
) {
  const ids = new Set(provider.models.map((model) => model.id.trim()));
  const candidates: DiscoveredProviderModel[] = [];
  for (const remote of discovered) {
    const id = remote.id.trim();
    if (!id || ids.has(id)) continue;
    ids.add(id);
    candidates.push(remote);
  }
  return candidates;
}

function appendDiscoveredModels(
  provider: ProviderProfile,
  discovered: DiscoveredProviderModel[],
) {
  const ids = new Set(provider.models.map((model) => model.id.trim()));
  const additions: ProviderModel[] = [];
  for (const remote of discovered) {
    const id = remote.id.trim();
    if (!id || ids.has(id)) continue;
    ids.add(id);
    additions.push({
      id,
      display_name: remote.display_name?.trim() || id,
      enabled: true,
      registry_id: remote.suggested_registry_id,
      context_window: null,
      capabilities: [],
      supported_engines:
        remote.compatible_engines.length > 0
          ? [...remote.compatible_engines]
          : [...provider.supported_engines],
    });
  }
  return [...provider.models, ...additions];
}

function targetKey(
  target: Pick<EngineTarget, "engine_id" | "provider_id" | "model_id">,
) {
  return JSON.stringify([target.engine_id, target.provider_id, target.model_id]);
}
