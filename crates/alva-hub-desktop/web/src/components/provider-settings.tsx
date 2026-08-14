// INPUT:  provider profiles, backend model routes/default target, save callbacks, shared UI controls
// OUTPUT: Cindy-style provider navigation, redacted connection editing, model/default-route controls
// POS:    Settings domain surface for staged model discovery, explicit registry binding, and Hub-owned routes

import { useEffect, useMemo, useRef, useState } from "react";
import { Check, FlaskConical, KeyRound, Plus, RefreshCw, Search, ServerCog, Trash2 } from "lucide-react";
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
  EmptyState,
  FieldFrame,
  NavItem,
  Notice,
  PanelSection,
  Switch,
  TextField,
} from "../ui";

const DISCOVERY_PREVIEW_LIMIT = 100;

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
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<ProviderProfile | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [clearApiKey, setClearApiKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [settingDefault, setSettingDefault] = useState<string | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [discoveredModels, setDiscoveredModels] = useState<DiscoveredProviderModel[]>([]);
  const [testingRoute, setTestingRoute] = useState<string | null>(null);
  const [diagnostic, setDiagnostic] = useState<{ ok: boolean; heading: string; message: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const discoveryToken = useRef(0);
  const draftRef = useRef<ProviderProfile | null>(null);
  draftRef.current = draft;
  const selected = providers.find((provider) => provider.id === selectedId) ?? providers[0];

  useEffect(() => {
    if (selected && selected.id !== draft?.id) {
      setSelectedId(selected.id);
      setDraft(structuredClone(selected));
      setApiKey("");
      setClearApiKey(false);
      setDiscoveredModels([]);
      setDiagnostic(null);
    }
  }, [selected, draft?.id]);

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
    setDraft((current) => current ? {
      ...current,
      models: current.models.map((model) => model.id === id ? { ...model, ...update } : model),
    } : current);
  }

  function addModel() {
    if (!draft) return;
    const id = `model-${draft.models.length + 1}`;
    setDraft({
      ...draft,
      models: [...draft.models, {
        id,
        display_name: t("provider.newModel"),
        enabled: true,
        registry_id: null,
        context_window: null,
        capabilities: [],
        supported_engines: [...draft.supported_engines],
      }],
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
        setDiagnostic({ ok: false, heading: t("provider.discoveryFailed"), message: localizeDiagnostic(result.message) });
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
          candidates.length === 1 ? "provider.discoverySummaryOne" : "provider.discoverySummaryMany",
          { found: result.models.length, added: candidates.length },
        ),
      });
    } catch (cause) {
      if (token === discoveryToken.current && draftRef.current?.id === providerId) {
        setDiagnostic({ ok: false, heading: t("provider.discoveryFailed"), message: cause instanceof Error ? cause.message : String(cause) });
      }
    } finally {
      if (token === discoveryToken.current) setDiscovering(false);
    }
  }

  function addDiscovered(candidates: DiscoveredProviderModel[]) {
    if (candidates.length === 0) return;
    setDiagnostic(null);
    setDraft((current) => current ? {
      ...current,
      models: appendDiscoveredModels(current, candidates),
    } : current);
    const addedIds = new Set(candidates.map((candidate) => candidate.id.trim()));
    setDiscoveredModels((current) => current.filter((candidate) => !addedIds.has(candidate.id.trim())));
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
        message: result.ok ? t("provider.testSummary", { latency: result.latency_ms }) : localizeDiagnostic(result.message),
      });
    } catch (cause) {
      setDiagnostic({ ok: false, heading: t("provider.testFailed"), message: cause instanceof Error ? cause.message : String(cause) });
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
      setDraft((current) => current?.id === draft.id ? {
        ...current,
        api_key_configured: nextKeyConfigured,
      } : current);
      setApiKey("");
      setClearApiKey(false);
      setDiscoveredModels([]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  }

  if (loading && providers.length === 0) {
    return <EmptyState title={t("provider.loading")} description={t("provider.loadingDescription")} />;
  }
  if (!draft) {
    return <EmptyState title={t("provider.none")} description={t("provider.noneDescription")} />;
  }

  const ready = routes.some(
    (route) => route.target.provider_id === draft.id && route.available,
  ) && (!draft.requires_api_key || draft.api_key_configured);
  return (
    <div className="hub-provider-settings">
      <aside className="hub-provider-settings__nav">
        <div className="hub-provider-settings__nav-heading">
          <strong>{t("provider.heading")}</strong>
          <small>{t("provider.subheading")}</small>
        </div>
        {providers.map((provider) => (
          <NavItem
            active={provider.id === draft.id}
            description={t("provider.modelCount", { count: provider.models.length })}
            key={provider.id}
            label={provider.display_name}
            icon={<span className={routes.some((route) => route.target.provider_id === provider.id && route.available) && (!provider.requires_api_key || provider.api_key_configured) ? "hub-provider-dot hub-provider-dot--ready" : "hub-provider-dot"} />}
            onClick={() => { discoveryToken.current += 1; setDiscovering(false); setDiscoveredModels([]); setSelectedId(provider.id); setDraft(structuredClone(provider)); setApiKey(""); setClearApiKey(false); setDiagnostic(null); }}
          />
        ))}
      </aside>
      <section className="hub-provider-settings__content">
        {error ? <Notice heading={t("provider.settingsNotSaved")} description={localizeDiagnostic(error)} tone="danger" /> : null}
        {diagnostic ? <Notice heading={diagnostic.heading} description={diagnostic.ok ? diagnostic.message : localizeDiagnostic(diagnostic.message)} tone={diagnostic.ok ? "info" : "danger"} /> : null}
        <header className="hub-provider-settings__header">
          <div>
            <span className="hub-provider-settings__icon"><ServerCog aria-hidden="true" size={20} /></span>
            <div><h2>{draft.display_name}</h2><p>{t("provider.compatible", { engines: draft.supported_engines.join(" · ") })}</p></div>
          </div>
          <div className="hub-provider-settings__actions">
            <Badge tone={ready ? "success" : "warning"}>{ready ? t("provider.available") : t("provider.setupNeeded")}</Badge>
            {!draft.managed ? <Switch checked={draft.enabled} label={t("provider.enabled")} onCheckedChange={(checked) => setDraft({ ...draft, enabled: checked === true })} /> : null}
          </div>
        </header>

        {draft.managed ? (
          <Notice heading={t("provider.managed")} description={t("provider.managedDescription")} tone="info" />
        ) : (
          <PanelSection heading={t("provider.connection")} description={t("provider.connectionDescription")}>
            <div className="hub-provider-form">
              {draft.endpoint_editable ? <FieldFrame label={t("provider.endpoint")}><TextField value={draft.base_url} onChange={(event) => setDraft({ ...draft, base_url: event.target.value })} /></FieldFrame> : null}
              {draft.requires_api_key ? (
                <>
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
                    <TextField
                      aria-label={t("provider.apiKey")}
                      autoComplete="off"
                      placeholder={clearApiKey ? t("provider.noKeySaved") : draft.api_key_configured ? t("provider.savedSecurely") : t("provider.pasteKey")}
                      type="password"
                      value={apiKey}
                      onChange={(event) => {
                        setApiKey(event.target.value);
                        if (event.target.value.length > 0) setClearApiKey(false);
                      }}
                    />
                  </FieldFrame>
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
                </>
              ) : <Notice heading={t("provider.localSubscription")} description={t("provider.localSubscriptionDescription")} tone="neutral" />}
            </div>
          </PanelSection>
        )}

        <PanelSection
          actions={!draft.managed ? <div className="hub-provider-settings__actions"><Button disabled={discovering || saving || !draft.enabled || (draft.requires_api_key && (!draft.api_key_configured || clearApiKey))} leadingIcon={<RefreshCw size={14} />} onClick={() => void discoverModels()} size="small" title={!draft.api_key_configured && draft.requires_api_key ? t("provider.saveCredentialsFirst") : undefined} variant="ghost">{discovering ? t("provider.discovering") : t("provider.discover")}</Button><Button leadingIcon={<Plus size={14} />} onClick={addModel} size="small" variant="ghost">{t("provider.addModel")}</Button></div> : undefined}
          description={t("provider.modelsDescription")}
          heading={t("provider.models")}
          meta={t("provider.enabledCount", { count: draft.models.filter((model) => model.enabled).length })}
        >
          <label className="hub-provider-search"><Search aria-hidden="true" size={16} /><TextField aria-label={t("provider.searchModels")} placeholder={t("modelPicker.searchPlaceholder")} value={query} onChange={(event) => setQuery(event.target.value)} /></label>
          {discoveredModels.length > 0 ? (
            <Notice
              actions={<Button onClick={() => addDiscovered(discoveredModels)} size="small" variant="secondary">{t("provider.addAllDiscovered")}</Button>}
              description={t("provider.discoveryReviewDescription", { count: discoveredModels.length })}
              heading={t("provider.discoveryReview")}
              tone="info"
            >
              <div className="hub-provider-models">
                {visibleDiscoveredModels.map((model) => {
                  const label = model.display_name?.trim() || model.id;
                  return (
                    <div className="hub-provider-model" key={model.id}>
                      <div className="hub-provider-model__identity">
                        <span><KeyRound aria-hidden="true" size={16} /></span>
                        <div><strong>{label}</strong><small>{model.id}</small></div>
                      </div>
                      <div className="hub-provider-model__controls">
                        <Badge tone={model.suggested_registry_id ? "accent" : "neutral"}>{model.suggested_registry_id ? t("provider.recognizedModel") : t("provider.customModel")}</Badge>
                        {model.compatible_engines.map((engine) => <Badge key={engine}>{engine}</Badge>)}
                        <Button aria-label={t("provider.addDiscoveredAria", { model: label })} onClick={() => addDiscovered([model])} size="small" variant="ghost">{t("provider.addDiscovered")}</Button>
                      </div>
                    </div>
                  );
                })}
              </div>
              {matchingDiscoveredModels.length > visibleDiscoveredModels.length ? <small>{t("provider.discoveryLimited", { shown: DISCOVERY_PREVIEW_LIMIT, count: matchingDiscoveredModels.length })}</small> : null}
            </Notice>
          ) : null}
          <div className="hub-provider-models">
            {visibleModels.length === 0 ? <EmptyState title={t("provider.noModels")} description={t("provider.noModelsDescription")} /> : visibleModels.map((model) => (
              <div className="hub-provider-model" key={model.id}>
                <div className="hub-provider-model__identity">
                  <span><KeyRound aria-hidden="true" size={16} /></span>
                  {draft.managed ? <div><strong>{model.display_name}</strong><small>{model.id}</small></div> : <div className="hub-provider-model__fields"><TextField aria-label={t("provider.modelDisplayName")} value={model.display_name} onChange={(event) => updateModel(model.id, { display_name: event.target.value })} /><TextField aria-label={t("provider.modelId")} value={model.id} onChange={(event) => updateModel(model.id, { id: event.target.value, registry_id: null })} /></div>}
                </div>
                <div className="hub-provider-model__controls">
                  <Badge tone={model.registry_id ? "accent" : "neutral"}>{model.registry_id ? t("provider.recognizedModel") : t("provider.customModel")}</Badge>
                  {model.enabled ? <Check aria-hidden="true" size={15} /> : null}
                  {!draft.managed ? <Switch checked={model.enabled} label={`${model.display_name} · ${t("provider.active")}`} onCheckedChange={(checked) => updateModel(model.id, { enabled: checked === true })} /> : <Badge tone="success">{t("provider.active")}</Badge>}
                  {routes
                    .filter((route) => route.target.provider_id === draft.id && route.target.model_id === model.id)
                    .map((route, routeIndex) => {
                      const identity = targetKey(route.target);
                      const isDefault = defaultTarget !== null && identity === targetKey(defaultTarget);
                      return (
                        <div className="hub-provider-settings__actions" key={identity}>
                          <Badge>{route.engine_label}</Badge>
                          {!draft.managed && routeIndex === 0 ? <Button aria-label={`${t("provider.test")} ${model.display_name}`} disabled={!route.available || testingRoute !== null} leadingIcon={<FlaskConical size={14} />} onClick={() => void testRoute(route)} size="small" title={route.available ? undefined : localize(route.unavailable_reason) || t("provider.setupNeeded")} variant="ghost">{testingRoute === identity ? t("provider.testing") : t("provider.test")}</Button> : null}
                          {isDefault ? <Badge tone="success">{t("provider.default", { engine: route.engine_label })}</Badge> : <Button
                            aria-label={t("provider.setDefaultAria", { engine: route.engine_label, model: model.display_name })}
                            disabled={!route.available || settingDefault !== null}
                            onClick={() => void setDefaultRoute(route)}
                            size="small"
                            title={route.available ? undefined : localize(route.unavailable_reason) || t("provider.setupNeeded")}
                            variant="ghost"
                          >
                            {settingDefault === identity ? t("provider.settingDefault") : t("provider.setDefault")}
                          </Button>}
                        </div>
                      );
                    })}
                  {!draft.managed ? <Button aria-label={t("provider.removeModel", { model: model.display_name })} onClick={() => setDraft({ ...draft, models: draft.models.filter((candidate) => candidate !== model) })} size="small" variant="ghost"><Trash2 aria-hidden="true" size={14} /></Button> : null}
                </div>
              </div>
            ))}
          </div>
        </PanelSection>
        {!draft.managed ? <footer className="hub-provider-settings__footer"><Button disabled={saving} onClick={() => void save()} variant="primary">{saving ? t("provider.saving") : t("provider.save")}</Button></footer> : null}
      </section>
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
      supported_engines: remote.compatible_engines.length > 0
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
