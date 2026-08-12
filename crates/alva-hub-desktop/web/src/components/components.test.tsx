// INPUT:  Hub domain components rendered with deterministic route data
// OUTPUT: Regression checks for composer and model-picker interaction
// POS:    Behavior coverage above primitives and below full App integration

import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UIProvider } from "../ui";
import { HubComposer } from "./hub-composer";
import { ModelPicker, type HubModelRoute } from "./model-picker";

const routes: HubModelRoute[] = [
  {
    engineId: "alva",
    engineLabel: "Alva",
    modelId: "workspace-config",
    modelLabel: "Workspace model",
    providerId: "workspace-config",
    accountSource: "Workspace",
    billingRoute: "Workspace provider",
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
  },
];

describe("Hub domain components", () => {
  it("filters and selects a model route from the anchored picker", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <UIProvider>
        <ModelPicker
          onSelect={onSelect}
          routes={routes}
          selectedEngineId="alva"
          selectedModelId="workspace-config"
        />
      </UIProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Model and engine" }));
    await user.click(screen.getByRole("tab", { name: "Codex" }));
    await user.type(screen.getByLabelText("Search models"), "GPT-5");
    await user.click(screen.getByRole("button", { name: /GPT-5 Codex/ }));

    expect(onSelect).toHaveBeenCalledWith(routes[1]);
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
});
