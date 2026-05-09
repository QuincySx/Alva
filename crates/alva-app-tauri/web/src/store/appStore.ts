import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

import type { PendingApproval, RemoteModelInfo } from "../agent-bridge";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ProviderKind = "anthropic" | "openai" | "openai-responses" | "gemini";

export interface ProviderConfig {
  /** Stable local id (uuid-ish). */
  id: string;
  /** User-facing label shown in the picker. */
  name: string;
  provider: ProviderKind;
  model: string;
  api_key: string;
  /** Optional custom endpoint (e.g. proxy, self-host). */
  base_url?: string;
}

export interface AgentPreset {
  id: string;
  name: string;
  system_prompt: string;
  /** Preset skill names to enable for sessions using this agent. */
  preset_skills: string[];
  /** Preset tool names to enable for sessions using this agent. */
  preset_tools: string[];
  /** Free-form notes shown in the editor. */
  notes?: string;
}

/**
 * User-supplied capability overrides keyed by model id. Merged on top of
 * whatever the provider returned (usually nothing, except OpenRouter). A
 * field being `undefined` means "no override — inherit from API". Setting
 * a field explicitly overrides even a known-from-API value.
 */
export interface ModelOverride {
  supports_tools?: boolean;
  is_reasoning?: boolean;
  vision?: boolean;
  context_window?: number;
  /** User-set output token cap, overrides whatever the API reported.
   * `undefined` = no override. Frontend resolver order:
   * override → API caps → null (backend fallback applies). */
  max_output_tokens?: number;
  /** Image-output capability override. Mostly informational right
   * now — the chat path doesn't yet route image generation. */
  image_output?: boolean;
  /** Mark a model as embedding-only so it's filterable from the chat
   * picker. Default: not embedding (regular chat model). */
  embedding?: boolean;
  /** Free-form provider-specific options merged into the chat request
   * body. Stored as-is, sent verbatim. Use cases: Doubao
   * `{ "thinking": { "type": "disabled" } }` to turn off reasoning,
   * `{ "stream_options": { "include_usage": true } }`, etc.
   * `undefined` = no override; an empty object `{}` is treated the
   * same as undefined when merging.  */
  provider_options?: Record<string, unknown>;
}

/** Per-model capability resolution with opinionated defaults.
 *
 * tools + reasoning default to TRUE: as of 2026 the long tail of models
 * without these is small, and the user flips them off per-model via the
 * pencil icon in Settings.
 *
 * vision defaults to FALSE: vision is still the minority case, so the
 * user opts-in per-model.
 *
 * Resolution order: explicit override → API-reported cap → default. */
export interface ResolvedCapabilities {
  supports_tools: boolean;
  is_reasoning: boolean;
  vision: boolean;
  /** `null` means "let the backend pick its fallback" (currently 32 000).
   * Any positive number flows straight into ProviderConfig.max_tokens. */
  max_output_tokens: number | null;
  /** User-set vendor JSON, sent as-is into the LLM request body. `null`
   * = no overrides (request body unchanged). */
  provider_options: Record<string, unknown> | null;
}

export function resolveModelCapabilities(
  override: ModelOverride | undefined,
  apiCaps:
    | {
        supports_tools: boolean | null;
        is_reasoning: boolean | null;
        max_output_tokens?: number | null;
      }
    | null
    | undefined,
): ResolvedCapabilities {
  return {
    supports_tools:
      override?.supports_tools ?? apiCaps?.supports_tools ?? true,
    is_reasoning: override?.is_reasoning ?? apiCaps?.is_reasoning ?? true,
    vision: override?.vision ?? false,
    max_output_tokens:
      override?.max_output_tokens ?? apiCaps?.max_output_tokens ?? null,
    provider_options:
      override?.provider_options &&
      Object.keys(override.provider_options).length > 0
        ? override.provider_options
        : null,
  };
}

interface AppState {
  providerConfigs: ProviderConfig[];
  activeProviderConfigId: string | null;

  agents: AgentPreset[];

  /** Per-model capability overrides keyed by model id. */
  modelOverrides: Record<string, ModelOverride>;

  /** Last-known remote model list per provider config id. Written on a
   *  successful `listRemoteModels` fetch, overwritten on the next one.
   *  Persisted so Home can resolve the active model's API-reported
   *  capabilities (tools / reasoning / context window) without re-hitting
   *  the network every render. The user's explicit overrides live in
   *  `modelOverrides` and always win over this cache on merge. */
  remoteModelsCache: Record<string, RemoteModelInfo[]>;

  /** User-added models keyed by provider-config id. Identical shape to
   *  `remoteModelsCache` so the picker can merge both without caring
   *  about source. Used when the provider doesn't expose `/v1/models` or
   *  the user wants to track a private deployment. */
  manualModels: Record<string, RemoteModelInfo[]>;

  /** Sidebar collapsed — persisted so it sticks across reloads. */
  navCollapsed: boolean;

  /** Active session id. Not persisted — sessions live in memory on the
   *  Rust side and don't survive a restart. */
  activeSessionId: string | null;
  /** Monotonic counter; consumers (NavSidebar, Home) increment deps on
   *  it to re-fetch the session list. */
  sessionListNonce: number;

  /** Tool selection mode. When `true`, the agent sees every tool (current
   *  default behaviour); when `false`, only `selectedTools` are advertised
   *  to the LLM on the next turn. */
  toolsAutoMode: boolean;
  /** Explicitly allow-listed tool names (manual mode only). */
  selectedTools: string[];

  /** Pending approval requests pushed from the backend. Not persisted —
   *  if the app restarts, in-flight tool calls are gone too. Render
   *  inline in the chat as cards with Allow / Reject buttons. */
  pendingApprovals: PendingApproval[];

  // Ephemeral UI state — intentionally NOT persisted (see `partialize`).
  settingsOpen: boolean;

  addProviderConfig: (config: Omit<ProviderConfig, "id">) => ProviderConfig;
  updateProviderConfig: (id: string, updates: Partial<Omit<ProviderConfig, "id">>) => void;
  deleteProviderConfig: (id: string) => void;
  setActiveProviderConfig: (id: string | null) => void;

  addAgent: (agent: Omit<AgentPreset, "id">) => AgentPreset;
  updateAgent: (id: string, updates: Partial<Omit<AgentPreset, "id">>) => void;
  deleteAgent: (id: string) => void;

  setModelOverride: (modelId: string, override: ModelOverride | null) => void;

  setRemoteModels: (configId: string, models: RemoteModelInfo[]) => void;

  addManualModel: (configId: string, model: RemoteModelInfo) => void;
  removeManualModel: (configId: string, modelId: string) => void;

  openSettings: () => void;
  closeSettings: () => void;

  toggleNavCollapsed: () => void;
  setNavCollapsed: (v: boolean) => void;

  setActiveSessionId: (id: string | null) => void;
  bumpSessionList: () => void;

  setToolsAutoMode: (v: boolean) => void;
  setSelectedTools: (tools: string[]) => void;

  upsertPendingApproval: (req: PendingApproval) => void;
  removePendingApproval: (requestId: string) => void;
  setPendingApprovals: (list: PendingApproval[]) => void;
  clearPendingApprovals: () => void;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function newId(): string {
  // Good enough for local identifiers — not a security token.
  return `cfg_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * One-shot migration from the old flat keys set by previous versions
 * (`alva.provider` / `alva.model` / `alva.apiKey` / `alva.baseUrl`) into a
 * single ProviderConfig. Runs only when the store is empty at init.
 */
function migrateLegacyLocalStorage(): ProviderConfig | null {
  try {
    const provider = localStorage.getItem("alva.provider");
    const model = localStorage.getItem("alva.model");
    const apiKey = localStorage.getItem("alva.apiKey") ?? "";
    const baseUrl = localStorage.getItem("alva.baseUrl") ?? "";
    if (!provider || !model) return null;
    if (
      provider !== "anthropic" &&
      provider !== "openai" &&
      provider !== "openai-responses" &&
      provider !== "gemini"
    ) {
      return null;
    }
    return {
      id: newId(),
      name: `${provider} · ${model}`,
      provider,
      model,
      api_key: apiKey,
      base_url: baseUrl || undefined,
    };
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const useAppStore = create<AppState>()(
  persist(
    (set, _get) => ({
      providerConfigs: [],
      activeProviderConfigId: null,
      agents: [],
      modelOverrides: {},
      remoteModelsCache: {},
      manualModels: {},
      navCollapsed: false,
      activeSessionId: null,
      sessionListNonce: 0,
      toolsAutoMode: true,
      selectedTools: [],
      pendingApprovals: [],
      settingsOpen: false,

      openSettings: () => set({ settingsOpen: true }),
      closeSettings: () => set({ settingsOpen: false }),

      toggleNavCollapsed: () =>
        set((s) => ({ navCollapsed: !s.navCollapsed })),
      setNavCollapsed: (v) => set({ navCollapsed: v }),

      setActiveSessionId: (id) => set({ activeSessionId: id }),
      bumpSessionList: () =>
        set((s) => ({ sessionListNonce: s.sessionListNonce + 1 })),

      setToolsAutoMode: (v) => set({ toolsAutoMode: v }),
      setSelectedTools: (tools) => set({ selectedTools: tools }),

      upsertPendingApproval: (req) =>
        set((s) => {
          // De-dupe on request_id (multiple tabs / re-emit safety).
          const idx = s.pendingApprovals.findIndex(
            (p) => p.request_id === req.request_id,
          );
          if (idx === -1) {
            return { pendingApprovals: [...s.pendingApprovals, req] };
          }
          const next = s.pendingApprovals.slice();
          next[idx] = req;
          return { pendingApprovals: next };
        }),
      removePendingApproval: (requestId) =>
        set((s) => ({
          pendingApprovals: s.pendingApprovals.filter(
            (p) => p.request_id !== requestId,
          ),
        })),
      setPendingApprovals: (list) => set({ pendingApprovals: list }),
      clearPendingApprovals: () => set({ pendingApprovals: [] }),

      addAgent: (agent) => {
        const entry: AgentPreset = { id: newId(), ...agent };
        set((s) => ({ agents: [...s.agents, entry] }));
        return entry;
      },
      updateAgent: (id, updates) => {
        set((s) => ({
          agents: s.agents.map((a) => (a.id === id ? { ...a, ...updates } : a)),
        }));
      },
      deleteAgent: (id) => {
        set((s) => ({ agents: s.agents.filter((a) => a.id !== id) }));
      },

      setModelOverride: (modelId, override) => {
        set((s) => {
          const next = { ...s.modelOverrides };
          if (override == null) {
            delete next[modelId];
          } else {
            next[modelId] = override;
          }
          return { modelOverrides: next };
        });
      },

      setRemoteModels: (configId, models) => {
        set((s) => ({
          remoteModelsCache: { ...s.remoteModelsCache, [configId]: models },
        }));
      },

      addManualModel: (configId, model) => {
        set((s) => {
          const current = s.manualModels[configId] ?? [];
          // De-dup on id — re-adding overwrites display_name / capabilities
          // (lets the user "fix" a typo by re-saving).
          const next = current.filter((m) => m.id !== model.id);
          next.unshift(model);
          return {
            manualModels: { ...s.manualModels, [configId]: next },
          };
        });
      },
      removeManualModel: (configId, modelId) => {
        set((s) => {
          const current = s.manualModels[configId] ?? [];
          const next = current.filter((m) => m.id !== modelId);
          return {
            manualModels: { ...s.manualModels, [configId]: next },
          };
        });
      },

      addProviderConfig: (config) => {
        const entry: ProviderConfig = { id: newId(), ...config };
        set((s) => ({
          providerConfigs: [...s.providerConfigs, entry],
          activeProviderConfigId: s.activeProviderConfigId ?? entry.id,
        }));
        return entry;
      },
      updateProviderConfig: (id, updates) => {
        set((s) => ({
          providerConfigs: s.providerConfigs.map((c) =>
            c.id === id ? { ...c, ...updates } : c,
          ),
        }));
      },
      deleteProviderConfig: (id) => {
        set((s) => {
          const next = s.providerConfigs.filter((c) => c.id !== id);
          const activeId =
            s.activeProviderConfigId === id
              ? (next[0]?.id ?? null)
              : s.activeProviderConfigId;
          const { [id]: _drop, ...remoteModelsCache } = s.remoteModelsCache;
          const { [id]: _drop2, ...manualModels } = s.manualModels;
          return {
            providerConfigs: next,
            activeProviderConfigId: activeId,
            remoteModelsCache,
            manualModels,
          };
        });
      },
      setActiveProviderConfig: (id) => {
        set({ activeProviderConfigId: id });
      },
    }),
    {
      name: "alva.appStore",
      storage: createJSONStorage(() => localStorage),
      // Don't persist UI-ephemeral fields.
      partialize: (state) => ({
        providerConfigs: state.providerConfigs,
        activeProviderConfigId: state.activeProviderConfigId,
        agents: state.agents,
        modelOverrides: state.modelOverrides,
        remoteModelsCache: state.remoteModelsCache,
        manualModels: state.manualModels,
        navCollapsed: state.navCollapsed,
        toolsAutoMode: state.toolsAutoMode,
        selectedTools: state.selectedTools,
      }),
      // Run legacy migration once on init, only if the store is empty.
      onRehydrateStorage: () => (state) => {
        if (!state) return;
        if (state.providerConfigs.length === 0) {
          const legacy = migrateLegacyLocalStorage();
          if (legacy) {
            state.providerConfigs = [legacy];
            state.activeProviderConfigId = legacy.id;
          }
        }
      },
    },
  ),
);

/** Convenience: pull the currently-active provider config, or null. */
export function useActiveProviderConfig(): ProviderConfig | null {
  return useAppStore((s) => {
    const id = s.activeProviderConfigId;
    if (!id) return null;
    return s.providerConfigs.find((c) => c.id === id) ?? null;
  });
}

/** Resolve capability flags for a model. Consults:
 *  1. The user's explicit override in `modelOverrides`.
 *  2. The cached API capabilities from the last `listRemoteModels` fetch
 *     against the owning provider config (pass `providerConfigId`).
 *  3. The opinionated defaults in `resolveModelCapabilities`.
 *
 *  Pass both ids where possible so Home gets real context-window + API
 *  caps the same way the Settings screen does. */
export function useResolvedModelCapabilities(
  modelId: string | null | undefined,
  providerConfigId?: string | null,
): ResolvedCapabilities {
  return useAppStore((s) => {
    const override = modelId ? s.modelOverrides[modelId] : undefined;
    const list =
      providerConfigId ? s.remoteModelsCache[providerConfigId] : undefined;
    const apiCaps =
      list && modelId
        ? (list.find((m) => m.id === modelId)?.capabilities ?? null)
        : null;
    return resolveModelCapabilities(override, apiCaps);
  });
}
