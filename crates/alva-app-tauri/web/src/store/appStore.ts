import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ProviderKind = "anthropic" | "openai" | "openai-responses";

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
  context_window?: number;
}

interface AppState {
  providerConfigs: ProviderConfig[];
  activeProviderConfigId: string | null;

  agents: AgentPreset[];

  /** Per-model capability overrides keyed by model id. */
  modelOverrides: Record<string, ModelOverride>;

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

  openSettings: () => void;
  closeSettings: () => void;

  toggleNavCollapsed: () => void;
  setNavCollapsed: (v: boolean) => void;

  setActiveSessionId: (id: string | null) => void;
  bumpSessionList: () => void;

  setToolsAutoMode: (v: boolean) => void;
  setSelectedTools: (tools: string[]) => void;
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
    if (provider !== "anthropic" && provider !== "openai" && provider !== "openai-responses") {
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
      navCollapsed: false,
      activeSessionId: null,
      sessionListNonce: 0,
      toolsAutoMode: true,
      selectedTools: [],
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
          return { providerConfigs: next, activeProviderConfigId: activeId };
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
