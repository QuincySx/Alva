// INPUT:  Hub domain components rendered with deterministic provider and route data
// OUTPUT: Regression checks for composer, model-picker identity, and provider secret/default saves
// POS:    Behavior coverage above primitives and below full App integration

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { EngineTarget, ModelRoute, ProviderDiagnosticResult, ProviderProfile, TimelineEvent } from "../bridge";
import { I18nProvider } from "../i18n";
import { demoDetail, demoInspection } from "../demo-data";
import { UIProvider } from "../ui";
import { HubComposer } from "./hub-composer";
import { EngineInspectorPanel } from "./engine-inspector";
import { HubTimeline } from "./hub-timeline";
import { ModelPicker, type HubModelRoute } from "./model-picker";
import { ProviderSettings } from "./provider-settings";

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
    expect(screen.getByText(/native-a \/ demo-nat/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByText(/Child Agent demo-nat/i)).toBeInTheDocument();
  });

  it("renders a shared-clock execution overview above timed events", () => {
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

    expect(
      screen.getByRole("img", { name: /Agent execution timeline overview|Agent 执行时间线概览/ }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Tools|工具/)).toBeInTheDocument();
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
});
