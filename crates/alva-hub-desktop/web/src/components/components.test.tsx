// INPUT:  Hub domain components rendered with deterministic provider and route data
// OUTPUT: Regression checks for composer, model-picker identity, and provider secret/default saves
// POS:    Behavior coverage above primitives and below full App integration

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { EngineTarget, HubSessionSummary, ModelRoute, ProviderDiagnosticResult, ProviderProfile, TimelineEvent } from "../bridge";
import { I18nProvider } from "../i18n";
import { demoDetail, demoInspection } from "../demo-data";
import { UIProvider } from "../ui";
import { HubComposer } from "./hub-composer";
import { EngineInspectorPanel } from "./engine-inspector";
import { HubSidebar } from "./hub-sidebar";
import { HubTimeline } from "./hub-timeline";
import { ModelPicker, type HubModelRoute } from "./model-picker";
import { ProviderSettings } from "./provider-settings";
import { SettingsPage } from "./product-pages";
import { InspectorTool } from "./workbench-tools";

const routes: HubModelRoute[] = [
  {
    engineId: "alva",
    engineLabel: "Alva",
    modelId: "workspace-config",
    modelLabel: "Workspace model",
    providerId: "workspace-config",
    accountSource: "Workspace",
    billingRoute: "Workspace provider",
    reasoningEfforts: [],
    reasoningEffort: null,
    supportsFastMode: false,
    fastMode: false,
  },
  {
    engineId: "codex",
    engineLabel: "Codex",
    modelId: "gpt-5-codex",
    modelLabel: "GPT-5 Codex",
    providerId: "codex-cli",
    accountSource: "Subscription",
    billingRoute: "OpenAI account",
    capabilities: ["images", "reasoning"],
    reasoningEfforts: ["low", "medium", "high", "xhigh"],
    reasoningEffort: "high",
    supportsFastMode: true,
    fastMode: false,
  },
];

const provider: ProviderProfile = {
  id: "anthropic",
  display_name: "Anthropic",
  kind: "anthropic",
  base_url: "https://api.anthropic.com",
  enabled: true,
  api_key_configured: true,
  requires_api_key: true,
  endpoint_editable: false,
  managed: false,
  max_output_tokens: 8192,
  supported_engines: ["alva"],
  models: [{
    id: "claude-sonnet",
    display_name: "Claude Sonnet",
    enabled: true,
    context_window: 200_000,
    capabilities: ["tools"],
    supported_engines: ["alva"],
  }],
};

function target(providerId: string): EngineTarget {
  return {
    engine_id: "alva",
    model_id: "claude-sonnet",
    provider_id: providerId,
    account_source: `${providerId} account`,
    billing_route: `${providerId} API`,
    automatic_use_authorized: false,
    required_capabilities: [],
    reasoning_effort: "high",
    fast_mode: false,
  };
}

function providerRoute(providerId: string, available = true): ModelRoute {
  return {
    target: target(providerId),
    engine_label: "Alva",
    model_label: "Claude Sonnet",
    provider_label: providerId,
    description: `${providerId} route`,
    context_window: 200_000,
    capabilities: ["tools"],
    reasoning_efforts: ["low", "medium", "high"],
    default_reasoning_effort: "high",
    supports_fast_mode: false,
    available,
    unavailable_reason: available ? null : "Setup needed",
  };
}

describe("Hub domain components", () => {
  it("renders an expired approval card without re-offering Allow/Deny", () => {
    window.localStorage.setItem("alva-hub.language", "en-US");
    const event: TimelineEvent = {
      status: "permission_required",
      engine_id: "alva",
      product_session_id: "p-1",
      message: null,
      permission_request: {
        request_id: "req-expired",
        tool_name: "Bash",
        description: null,
        engine_session_id: "native-1",
      },
    };
    render(
      <I18nProvider>
        <UIProvider>
          <HubTimeline
            activeSessionId="p-1"
            answeredPermissions={{ "req-expired": "expired" }}
            attemptIsLive={false}
            events={[event]}
            onAnswerPermission={() => void Promise.resolve()}
            onStop={() => void Promise.resolve()}
            onSwitchBack={() => void Promise.resolve()}
          />
        </UIProvider>
      </I18nProvider>,
    );
    expect(screen.getByText("This approval has expired.")).toBeTruthy();
    // No Allow/Deny buttons on a stale card — the answer can never land.
    expect(screen.queryByText("Allow")).toBeNull();
    expect(screen.queryByText("Deny")).toBeNull();
  });

  it("drills into a child Agent and returns through the Inspector page stack", async () => {
    window.localStorage.setItem("alva-hub.language", "en-US");
    const user = userEvent.setup();
    const loadInspection = vi.fn(async (_productSessionId: string, nativeSessionId?: string | null) =>
      demoInspection(nativeSessionId ?? undefined));
    render(
      <I18nProvider>
        <UIProvider>
          <EngineInspectorPanel detail={demoDetail()} loadInspection={loadInspection} />
        </UIProvider>
      </I18nProvider>,
    );

    const childLabel = await screen.findByText(/Child Agent demo-nat/i);
    const childRow = childLabel.closest(".ui-list-row");
    expect(childRow).not.toBeNull();
    await user.click(within(childRow as HTMLElement).getByRole("button", { name: "Open" }));

    await waitFor(() => expect(loadInspection).toHaveBeenLastCalledWith("ps-demo-1", "demo-native-child"));
    // R33: breadcrumbs use friendly identity (engine + role); the short id is
    // a secondary hint instead of the primary label.
    expect(screen.getByText(/Root session \/ alva · Child session \(demo-nat/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByText(/Child Agent demo-nat/i)).toBeInTheDocument();
  });

  it("restores scroll position and focus through repeated Back steps (R33)", async () => {
    window.localStorage.setItem("alva-hub.language", "en-US");
    const user = userEvent.setup();
    const loadInspection = vi.fn(async (_productSessionId: string, nativeSessionId?: string | null) =>
      demoInspection(nativeSessionId ?? undefined));
    render(
      <I18nProvider>
        <UIProvider>
          <EngineInspectorPanel detail={demoDetail()} loadInspection={loadInspection} />
        </UIProvider>
      </I18nProvider>,
    );

    const body = () => document.querySelector(".hub-engine-inspector__body") as HTMLElement;
    const originButton = (key: string) =>
      document.querySelector(`[data-inspector-origin="${key}"]`) as HTMLButtonElement;

    await screen.findByText(/Child Agent demo-nat/i);
    body().scrollTop = 120;

    // Root → child: the fresh level starts at the top…
    await user.click(originButton("child:demo-native-child"));
    await waitFor(() => expect(loadInspection).toHaveBeenLastCalledWith("ps-demo-1", "demo-native-child"));
    expect(screen.getByText(/Child session/i)).toBeInTheDocument();

    // …and opening a detail from a scrolled level is visible immediately.
    body().scrollTop = 300;
    await user.click(originButton("llm:0"));
    expect(await screen.findByText("Messages sent")).toBeInTheDocument();
    expect(body().scrollTop).toBe(0);

    // Back #1: the child level's scroll and the LLM row control come back.
    await user.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(body().scrollTop).toBe(300));
    await waitFor(() => expect(document.activeElement).toBe(originButton("llm:0")));

    // Back #2: the root level's scroll and the child row control come back.
    await user.click(screen.getByRole("button", { name: "Back" }));
    await screen.findByText(/Child Agent demo-nat/i);
    await waitFor(() => expect(body().scrollTop).toBe(120));
    await waitFor(() => expect(document.activeElement).toBe(originButton("child:demo-native-child")));
  });

  it("product attempt selection drives the Inspector and unsupported engines say so (R33)", async () => {
    window.localStorage.setItem("alva-hub.language", "en-US");
    const user = userEvent.setup();
    const loadInspection = vi.fn(async (_productSessionId: string, nativeSessionId?: string | null) =>
      demoInspection(nativeSessionId ?? undefined));
    render(
      <I18nProvider>
        <UIProvider>
          <InspectorTool detail={demoDetail()} events={[]} loadInspection={loadInspection} />
        </UIProvider>
      </I18nProvider>,
    );

    await screen.findByText(/Child Agent demo-nat/i);

    // Drill deep first: the ALVA attempt must pull the Inspector back to its
    // native session from any depth.
    await user.click(document.querySelector(`[data-inspector-origin="child:demo-native-child"]`) as HTMLButtonElement);
    await waitFor(() => expect(loadInspection).toHaveBeenLastCalledWith("ps-demo-1", "demo-native-child"));

    await user.click(screen.getAllByText("Product attempts")[0]);
    await user.click(await screen.findByText("将脱敏测试迁移到新表结构"));
    const claudeAttempt = await screen.findByText(/claude · /i);
    const claudeRow = claudeAttempt.closest(".ui-disclosure");
    expect(claudeRow).not.toBeNull();
    await user.click(within(claudeRow as HTMLElement).getByRole("button", { name: "Locate in Inspector" }));
    // Explicit unavailable state; no bogus native lookup is attempted.
    expect(loadInspection).not.toHaveBeenCalledWith("ps-demo-1", "native-claude-1");
    expect(await screen.findByText(/claude attempts do not expose native Agent records/i)).toBeInTheDocument();

    // The attempt title ("alva · Failed") also matches the panel heading
    // ("ALVA · Native Agent Inspector") — scope to the disclosure row.
    const alvaAttempt = screen.getAllByText(/^alva · /i).find((el) => el.closest(".ui-disclosure"));
    expect(alvaAttempt).toBeTruthy();
    const alvaRow = (alvaAttempt as HTMLElement).closest(".ui-disclosure");
    await user.click(within(alvaRow as HTMLElement).getByRole("button", { name: "Locate in Inspector" }));
    // "Locate" truncates the stack to the focused session — the Inspector
    // lands on the root level, and the unavailable notice clears.
    expect(await screen.findByText(/^alva · Root session$/)).toBeInTheDocument();
    expect(screen.queryByText(/claude attempts do not expose native Agent records/i)).toBeNull();
  });

  it("renders failures, unknown usage and long hook chains with middleware collapsed (R33)", async () => {
    window.localStorage.setItem("alva-hub.language", "en-US");
    const user = userEvent.setup();
    const snapshot = demoInspection();
    snapshot.run.total_input_tokens = 0;
    snapshot.run.total_output_tokens = 0;
    snapshot.run.turns[0].llm_call.error_message = "provider 500";
    snapshot.run.turns[0].llm_call.middleware_hooks = Array.from({ length: 9 }, (_, index) => ({
      middleware_name: `mw-${index}`,
      hook: "wrap_llm_call",
      duration_ms: 10 + index,
      outcome: "success",
      error: null,
      tool_call_id: null,
    }));
    snapshot.run.middleware_hooks = snapshot.run.turns[0].llm_call.middleware_hooks;
    const loadInspection = vi.fn(async () => snapshot);
    render(
      <I18nProvider>
        <UIProvider>
          <EngineInspectorPanel detail={demoDetail()} loadInspection={loadInspection} />
        </UIProvider>
      </I18nProvider>,
    );

    await screen.findByText(/Child Agent demo-nat/i);
    // Zero/zero usage renders as unknown, never as a fake number.
    expect(screen.getByText("—")).toBeInTheDocument();
    // The failed LLM call keeps its danger badge on the overview.
    expect(screen.getAllByText("Failed").length).toBeGreaterThanOrEqual(1);
    // Middleware stays collapsed by default even with a long hook chain…
    expect(document.querySelector(`[data-inspector-origin="mw:0"]`)).toBeNull();
    await user.click(screen.getByText("Middleware calls"));
    // …and every hook of the long chain is reachable once expanded.
    expect(document.querySelector(`[data-inspector-origin="mw:8"]`)).not.toBeNull();

    // The child-agent path stays navigable from a sparse snapshot.
    await user.click(document.querySelector(`[data-inspector-origin="child:demo-native-child"]`) as HTMLButtonElement);
    expect(await screen.findByText(/Root session \/ alva · Child session/i)).toBeInTheDocument();
  });

  it("distinguishes same-name sessions and keeps search states truthful (R34)", () => {
    window.localStorage.setItem("alva-hub.language", "en-US");
    const summary = (overrides: Partial<HubSessionSummary>): HubSessionSummary => ({
      product_session_id: "ps-x",
      workspace: "/managed/aaaa-bbbb-cccc",
      workspace_kind: "session",
      active_branch_id: "b",
      engine_id: "alva",
      model_id: "workspace-config",
      provider_id: "workspace-config",
      engine_session_id: null,
      status: "completed",
      updated_at: 0,
      last_prompt: "Same prompt text",
      last_result: null,
      permission_profile: "workspace_write",
      include_full_transcript: false,
      ...overrides,
    });
    const sessions = [
      summary({ product_session_id: "ps-1", updated_at: Date.parse("2026-09-10T08:00:00Z") }),
      summary({
        product_session_id: "ps-2",
        updated_at: Date.parse("2026-09-11T09:30:00Z"),
        workspace_kind: "project",
        workspace: "/code/alva-agent",
      }),
    ];
    const sidebarProps = (overrides: Partial<Parameters<typeof HubSidebar>[0]> = {}) => ({
      activePage: "conversation" as const,
      activeSessionId: "ps-1",
      busy: false,
      formatUpdatedAt: (updatedAt: number) => new Date(updatedAt).toISOString().slice(0, 10),
      onNewSession: () => {},
      onOpenSession: () => {},
      onSessionQueryChange: () => {},
      sessionError: null,
      sessionQuery: "",
      sessions,
      totalSessionCount: 2,
      onNavigate: () => {},
      ...overrides,
    });
    const { rerender } = render(
      <I18nProvider>
        <HubSidebar {...sidebarProps()} />
      </I18nProvider>,
    );

    // Identical prompt labels stay distinguishable through real auxiliary
    // identity: project dir vs the independent-session label, and timestamps.
    expect(screen.getAllByText("Same prompt text")).toHaveLength(2);
    expect(screen.getByText("Independent session")).toBeInTheDocument();
    expect(screen.getByText("alva-agent")).toBeInTheDocument();
    expect(screen.getByText(/2026-09-10/)).toBeInTheDocument();
    expect(screen.getByText(/2026-09-11/)).toBeInTheDocument();

    // No-match search state (the sidebar renders the list App already filtered)…
    rerender(
      <I18nProvider>
        <HubSidebar {...sidebarProps({ sessionQuery: "zzz", sessions: [] })} />
      </I18nProvider>,
    );
    expect(screen.getByText("No sessions match that search.")).toBeInTheDocument();

    // …and the failure state names the real error instead of an empty list.
    rerender(
      <I18nProvider>
        <HubSidebar {...sidebarProps({ sessionError: "store unavailable" })} />
      </I18nProvider>,
    );
    expect(screen.getByText("store unavailable")).toBeInTheDocument();
  });

  it("renders execution events without chat messages or a three-lane overview (R30)", () => {
    render(
      <I18nProvider>
        <HubTimeline
          activeSessionId="ps-1"
          answeredPermissions={{}}
          attemptIsLive={false}
          events={[
            {
              status: "user_message",
              role: "user",
              engine_id: "alva",
              product_session_id: "ps-1",
              message: "Inspect the project",
              created_at: 1_000,
              item_id: "user-1",
            },
            {
              status: "activity_completed",
              engine_id: "alva",
              product_session_id: "ps-1",
              message: "read_file completed",
              created_at: 1_180,
              started_at: 1_050,
              item_id: "tool-1",
              activity: {
                tool_name: "read_file",
                succeeded: true,
                duration_ms: 130,
                path: "src/lib.rs",
              },
            },
          ]}
          onAnswerPermission={vi.fn()}
          onStop={vi.fn()}
          onSwitchBack={vi.fn()}
        />
      </I18nProvider>,
    );

    // The execution view keeps the tool row with its shared-clock duration…
    expect(screen.getAllByText(/read_file/).length).toBeGreaterThanOrEqual(1);
    // …but the main chat no longer owns a three-lane execution overview…
    expect(
      screen.queryByRole("img", { name: /Agent execution timeline overview|Agent 执行时间线概览/ }),
    ).not.toBeInTheDocument();
    // …and chat prose renders in ChatTranscript, not here.
    expect(screen.queryByText("Inspect the project")).not.toBeInTheDocument();
  });

  it("filters and selects a model route from the anchored picker", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={vi.fn()}
          onSelect={onSelect}
          routes={routes}
          selectedEngineId="alva"
          selectedModelId="workspace-config"
          selectedProviderId="workspace-config"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    await user.click(screen.getByRole("tab", { name: "Codex" }));
    await user.type(screen.getByLabelText("Search models"), "GPT-5");
    await user.click(screen.getByRole("button", { name: /GPT-5 Codex/ }));

    expect(onSelect).toHaveBeenCalledWith(routes[1]);
  });

  it("unified model search spans engine tabs and stays keyboard-selectable (R34)", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={vi.fn()}
          onSelect={onSelect}
          routes={routes}
          selectedEngineId="alva"
          selectedModelId="workspace-config"
          selectedProviderId="workspace-config"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    // Scoped to the active tab, the Codex model is not listed…
    expect(screen.queryByText(/codex-cli/)).toBeNull();
    // …but a query searches every runtime, and rows name their engine.
    await user.type(screen.getByLabelText("Search models"), "gpt-5");
    expect(screen.getByText("Searching every runtime.")).toBeInTheDocument();
    const codexRow = screen.getByRole("button", { name: /GPT-5 Codex/ });
    expect(within(codexRow).getByText(/Codex · codex-cli/)).toBeInTheDocument();

    // Keyboard-only: tabbing out of the search lands on the first result.
    await user.tab();
    expect(document.activeElement).toBe(codexRow);
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith(routes[1]);
  });

  it("distinguishes the active route when providers share a model id", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const duplicateModelRoutes: HubModelRoute[] = [
      {
        engineId: "alva",
        engineLabel: "Alva",
        modelId: "shared-model",
        modelLabel: "Shared model",
        providerId: "anthropic",
        accountSource: "Anthropic account",
        billingRoute: "Anthropic API",
        reasoningEfforts: ["low", "medium", "high"],
        reasoningEffort: "high",
        supportsFastMode: false,
        fastMode: false,
      },
      {
        engineId: "alva",
        engineLabel: "Alva",
        modelId: "shared-model",
        modelLabel: "Shared model",
        providerId: "openai",
        accountSource: "OpenAI account",
        billingRoute: "OpenAI API",
        reasoningEfforts: ["low", "medium", "high"],
        reasoningEffort: "high",
        supportsFastMode: false,
        fastMode: false,
      },
    ];
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={vi.fn()}
          onSelect={onSelect}
          routes={duplicateModelRoutes}
          selectedEngineId="alva"
          selectedModelId="shared-model"
          selectedProviderId="openai"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    const anthropicRoute = screen.getByRole("button", { name: /Shared model anthropic/i });
    const openAiRoute = screen.getByRole("button", { name: /Shared model openai/i });
    expect(anthropicRoute).not.toHaveClass("hub-model-picker__route--active");
    expect(openAiRoute).toHaveClass("hub-model-picker__route--active");

    await user.click(anthropicRoute);
    expect(onSelect).toHaveBeenCalledWith(duplicateModelRoutes[0]);
  });

  it("reveals model-specific options only from an available route", async () => {
    const user = userEvent.setup();
    const onOptionsChange = vi.fn();
    const groupedRoutes: HubModelRoute[] = [
      routes[1],
      {
        ...routes[1],
        modelId: "unavailable-codex",
        modelLabel: "Unavailable Codex",
        disabled: true,
        disabledReason: "Sign in first.",
      },
    ];
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={onOptionsChange}
          onSelect={vi.fn()}
          routes={groupedRoutes}
          selectedEngineId="codex"
          selectedModelId="gpt-5-codex"
          selectedProviderId="codex-cli"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();

    fireEvent.pointerMove(screen.getByRole("button", { name: /GPT-5 Codex/i }));
    expect(screen.getByRole("complementary", { name: "Options for GPT-5 Codex" })).toBeInTheDocument();

    fireEvent.pointerMove(screen.getByRole("button", { name: /Unavailable Codex/i }));
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();

    fireEvent.focus(screen.getByRole("button", { name: /GPT-5 Codex/i }));
    expect(screen.getByRole("complementary", { name: "Options for GPT-5 Codex" })).toBeInTheDocument();

    await user.click(screen.getByRole("option", { name: "Low" }));
    expect(onOptionsChange).toHaveBeenCalledWith(routes[1], {
      reasoningEffort: "low",
    });
    await user.click(screen.getByRole("switch", { name: "Fast mode" }));
    expect(onOptionsChange).toHaveBeenCalledWith(routes[1], {
      fastMode: true,
    });
  });

  it("does not invent Fast or reasoning controls for a model without metadata", async () => {
    const user = userEvent.setup();
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={vi.fn()}
          onSelect={vi.fn()}
          routes={[routes[0]]}
          selectedEngineId="alva"
          selectedModelId="workspace-config"
          selectedProviderId="workspace-config"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    fireEvent.pointerMove(screen.getByRole("button", { name: /Workspace model/i }));
    expect(screen.queryByRole("switch", { name: "Fast mode" })).not.toBeInTheDocument();
    expect(screen.queryByRole("listbox", { name: "Reasoning effort" })).not.toBeInTheDocument();
    expect(screen.getByText("This route does not expose an adjustable thinking level.")).toBeInTheDocument();
  });

  it("keeps Claude reasoning levels model-specific without showing Fast", async () => {
    const user = userEvent.setup();
    const claudeRoute: HubModelRoute = {
      ...routes[1],
      engineId: "claude",
      engineLabel: "Claude Code",
      modelId: "claude-opus-4-7",
      modelLabel: "Claude Opus 4.7",
      providerId: "claude-cli",
      reasoningEfforts: ["low", "medium", "high", "xhigh", "max"],
      supportsFastMode: false,
    };
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={vi.fn()}
          onSelect={vi.fn()}
          routes={[claudeRoute]}
          selectedEngineId="claude"
          selectedModelId="claude-opus-4-7"
          selectedProviderId="claude-cli"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    fireEvent.pointerMove(screen.getByRole("button", { name: /Claude Opus 4.7/i }));
    expect(screen.queryByRole("switch", { name: "Fast mode" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("option")).toHaveLength(5);
  });

  it("keeps unavailable models visible in a separate group at the bottom", async () => {
    const user = userEvent.setup();
    const groupedRoutes: HubModelRoute[] = [
      {
        ...routes[0],
        modelId: "missing-key",
        modelLabel: "Missing key model",
        disabled: true,
        disabledReason: "Add an API key in Settings.",
      },
      {
        ...routes[0],
        modelId: "ready-model",
        modelLabel: "Ready model",
      },
    ];
    render(
      <UIProvider>
        <ModelPicker
          onOptionsChange={vi.fn()}
          onSelect={vi.fn()}
          routes={groupedRoutes}
          selectedEngineId="alva"
          selectedModelId="ready-model"
          selectedProviderId="workspace-config"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    const group = screen.getByRole("region", { name: "Unavailable models" });
    const ready = screen.getByRole("button", { name: /Ready model/i });
    const unavailable = screen.getByRole("button", { name: /Missing key model/i });
    expect(group).toContainElement(unavailable);
    expect(ready.compareDocumentPosition(group) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(unavailable).toHaveAttribute("aria-disabled", "true");
  });

  it("saves replacement keys without echoing them and explicitly clears saved keys", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <UIProvider>
        <ProviderSettings
          defaultTarget={target("anthropic")}
          loading={false}
          onDiscover={vi.fn()}
          onSave={onSave}
          onSetDefault={vi.fn()}
          onTestRoute={vi.fn()}
          providers={[provider]}
          routes={[providerRoute("anthropic")]}
        />
      </UIProvider>,
    );

    const keyField = screen.getByLabelText("API key");
    expect(keyField).toHaveValue("");
    expect(keyField).toHaveAttribute("placeholder", "Saved securely");

    await user.type(keyField, "replacement-secret");
    await user.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenLastCalledWith(expect.objectContaining({
      api_key: "replacement-secret",
      clear_api_key: false,
    }));
    expect(screen.getByLabelText("API key")).toHaveValue("");
    expect(screen.queryByDisplayValue("replacement-secret")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Clear saved key" }));
    expect(screen.getByText("The saved key will be cleared when you save.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
    expect(onSave).toHaveBeenLastCalledWith(expect.objectContaining({
      api_key: null,
      clear_api_key: true,
    }));
    expect(screen.getByText("Setup needed")).toBeInTheDocument();
  });

  it("sets the default route by exact engine, provider, and model identity", async () => {
    const user = userEvent.setup();
    const onSetDefault = vi.fn().mockResolvedValue(undefined);
    const anthropicRoute = providerRoute("anthropic");
    const openAiDefault = target("openai");
    render(
      <UIProvider>
        <ProviderSettings
          defaultTarget={openAiDefault}
          loading={false}
          onDiscover={vi.fn()}
          onSave={vi.fn()}
          onSetDefault={onSetDefault}
          onTestRoute={vi.fn()}
          providers={[provider]}
          routes={[anthropicRoute, providerRoute("openai")]}
        />
      </UIProvider>,
    );

    expect(screen.queryByText(/Default · Alva/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Set Alva · Claude Sonnet as default" }));
    await waitFor(() => expect(onSetDefault).toHaveBeenCalledWith(anthropicRoute.target));
  });

  it("disables unavailable routes as global defaults", () => {
    render(
      <UIProvider>
        <ProviderSettings
          defaultTarget={null}
          loading={false}
          onDiscover={vi.fn()}
          onSave={vi.fn()}
          onSetDefault={vi.fn()}
          onTestRoute={vi.fn()}
          providers={[provider]}
          routes={[providerRoute("anthropic", false)]}
        />
      </UIProvider>,
    );

    expect(screen.getByRole("button", { name: "Set Alva · Claude Sonnet as default" })).toBeDisabled();
  });

  it("stages discovered models until the user confirms an explicit registry binding", async () => {
    const user = userEvent.setup();
    const onDiscover = vi.fn().mockResolvedValue({
      models: [
        { id: "claude-sonnet", display_name: "Duplicate", suggested_registry_id: null, compatible_engines: ["alva", "claude", "pi"] },
        { id: "claude-opus-4-7", display_name: "Claude Opus 4.7", suggested_registry_id: "claude-opus-4-7", compatible_engines: ["alva", "claude", "pi"] },
      ],
      ok: true,
      latency_ms: 12,
      message: "Discovered 2 model(s).",
    });
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <UIProvider>
        <ProviderSettings
          defaultTarget={null}
          loading={false}
          onDiscover={onDiscover}
          onSave={onSave}
          onSetDefault={vi.fn()}
          onTestRoute={vi.fn()}
          providers={[provider]}
          routes={[providerRoute("anthropic")]}
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Discover models" }));
    await screen.findByText("Claude Opus 4.7");
    expect(screen.queryByDisplayValue("Claude Opus 4.7")).not.toBeInTheDocument();
    expect(screen.getAllByDisplayValue("Claude Sonnet")).toHaveLength(1);
    expect(screen.getByText(/1 new model is ready for review/)).toBeInTheDocument();
    expect(screen.getByText("Recognized model")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add discovered model Claude Opus 4.7" }));
    await screen.findByDisplayValue("Claude Opus 4.7");
    await user.click(screen.getByRole("button", { name: "Save provider" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({
      models: expect.arrayContaining([expect.objectContaining({
        id: "claude-opus-4-7",
        registry_id: "claude-opus-4-7",
      })]),
    }));
  });

  it("merges a delayed discovery response into the latest same-provider edits", async () => {
    const user = userEvent.setup();
    let finishDiscovery: ((value: ProviderDiagnosticResult) => void) | undefined;
    const onDiscover = vi.fn(() => new Promise<ProviderDiagnosticResult>((resolve) => { finishDiscovery = resolve; }));
    render(
      <UIProvider>
        <ProviderSettings
          defaultTarget={null}
          loading={false}
          onDiscover={onDiscover}
          onSave={vi.fn()}
          onSetDefault={vi.fn()}
          onTestRoute={vi.fn()}
          providers={[provider]}
          routes={[providerRoute("anthropic")]}
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Discover models" }));
    const existingName = screen.getByDisplayValue("Claude Sonnet");
    await user.clear(existingName);
    await user.type(existingName, "My edited name");
    await act(async () => finishDiscovery?.({
      models: [{ id: "claude-opus", display_name: "Claude Opus", suggested_registry_id: null, compatible_engines: ["alva", "claude", "pi"] }],
      ok: true,
      latency_ms: 2,
      message: "Discovered 1 model(s).",
    }));

    expect(screen.getByDisplayValue("My edited name")).toBeInTheDocument();
    expect(screen.getByText("Claude Opus")).toBeInTheDocument();
    expect(screen.getAllByText("Custom model").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Add discovered model Claude Opus" }));
    expect(screen.getByDisplayValue("Claude Opus")).toBeInTheDocument();
  });

  it("tests the exact configured route and reports redacted diagnostics", async () => {
    const user = userEvent.setup();
    const route = providerRoute("anthropic");
    const onTestRoute = vi.fn().mockResolvedValue({
      models: [],
      ok: true,
      latency_ms: 34,
      message: "Inference succeeded for the selected model.",
    });
    render(
      <UIProvider>
        <ProviderSettings
          defaultTarget={null}
          loading={false}
          onDiscover={vi.fn()}
          onSave={vi.fn()}
          onSetDefault={vi.fn()}
          onTestRoute={onTestRoute}
          providers={[provider]}
          routes={[route]}
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Test Claude Sonnet" }));
    await waitFor(() => expect(onTestRoute).toHaveBeenCalledWith(route.target));
    expect(screen.getByText("Connection test passed")).toBeInTheDocument();
    expect(screen.getByText(/34 ms/)).toBeInTheDocument();
  });

  it("submits the composer on Enter but preserves Shift+Enter", () => {
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(
      <UIProvider>
        <HubComposer
          engineId="alva"
          onChange={() => {}}
          onSubmit={onSubmit}
          placeholder="Ask Alva"
          value="Inspect this project"
        />
      </UIProvider>,
    );

    const message = screen.getByLabelText("Message");
    fireEvent.keyDown(message, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.keyDown(message, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("routes the running action to stop instead of submitting again", () => {
    const onStop = vi.fn();
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(
      <UIProvider>
        <HubComposer
          busy
          canStop
          engineId="alva"
          onChange={() => {}}
          onStop={onStop}
          onSubmit={onSubmit}
          placeholder="Ask Alva"
          value=""
        />
      </UIProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Stop current run" }));
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  // ── R31: truthfulness of permission and execution-protection settings ────

  function renderSettings(sectionLabel: string) {
    render(
      <I18nProvider>
        <SettingsPage
          defaultTarget={null}
          onDiscoverProviderModels={vi.fn()}
          onSaveProvider={vi.fn()}
          onSetDefaultProvider={vi.fn()}
          onTestProviderRoute={vi.fn()}
          providers={[]}
          providersLoading={false}
          routes={[]}
        />
      </I18nProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: new RegExp(sectionLabel, "i") }));
  }

  it("permission mode control is disabled and never pretends to persist (R31)", () => {
    renderSettings("Permissions");
    // R37: the accessible name comes from the typed key, same as the heading.
    const modeSelect = screen.getByRole("combobox", { name: /Default Permission Mode/ });
    expect(modeSelect).toBeDisabled();
    // The row states the real source of truth instead of a success badge.
    expect(screen.getByText(/chosen per session in the composer/i)).toBeInTheDocument();
    // No "persistence in progress" badge that still renders an active select.
    expect(screen.queryByText(/Persistence in progress/i)).not.toBeInTheDocument();
  });

  it("sensitive-path protection reports as built-in, not as an enforced toggle (R31)", () => {
    renderSettings("Permissions");
    // R43: the badge scopes the capability to ALVA instead of claiming every
    // engine assembly ships the filter.
    expect(screen.getByText("ALVA only")).toBeInTheDocument();
    expect(screen.queryByText(/Built-in/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Protected/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/严格保护中/i)).not.toBeInTheDocument();
  });

  it("worker concurrency, resource tier and git snapshot are disabled (R31)", () => {
    renderSettings("General");
    // R37: accessible names follow the typed row headings.
    expect(screen.getByRole("combobox", { name: "Max Worker Concurrency" })).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Resource Mode" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Git Auto Checkpoint" })).toBeDisabled();
    expect(screen.getAllByText(/does not affect new sessions/i).length).toBeGreaterThanOrEqual(1);
  });

  // ── R32: production settings/extensions carry no fake state ──────────────

  it("appearance options that do nothing are disabled (R32)", () => {
    renderSettings("General");
    expect(screen.getByRole("button", { name: "Light" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "System" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Use compact density" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Reduce motion" })).toBeDisabled();
    expect(screen.getByText(/operating system/i)).toBeInTheDocument();
  });

  it("tools section disables fake toggles and shows MCP as not wired (R32)", () => {
    renderSettings("Tools");
    // R37: switch labels follow the typed row headings.
    expect(screen.getByRole("switch", { name: "File System" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Terminal / Shell" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Browser CDP" })).toBeDisabled();
    // No sample MCP rows, no add-server affordance.
    expect(screen.queryByText(/Chrome DevTools MCP/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Add MCP Server/i })).not.toBeInTheDocument();
    expect(screen.getAllByText(/not connected to a backend/i).length).toBeGreaterThanOrEqual(1);
  });

  it("remote SSH page shows an honest not-implemented state (R32)", () => {
    renderSettings("Remote");
    expect(screen.queryByText(/aliyun-prod/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Add SSH Host/i })).not.toBeInTheDocument();
    expect(screen.getAllByText(/not implemented in this build/i).length).toBeGreaterThanOrEqual(1);
  });

  it("usage page reports not-collected instead of invented numbers (R32)", () => {
    renderSettings("Usage");
    expect(screen.queryByText("1,420,500")).not.toBeInTheDocument();
    expect(screen.queryByText("$14.28")).not.toBeInTheDocument();
    expect(screen.queryByText(/GPT-5.6-Sol/i)).not.toBeInTheDocument();
    expect(screen.getAllByText(/not collected in this build/i).length).toBeGreaterThanOrEqual(1);
  });

  it("about page has no invented version or up-to-date claim (R32)", () => {
    renderSettings("About");
    expect(screen.queryByText(/v0\.1\.45/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Already up to date/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Check/i })).not.toBeInTheDocument();
  });
});
