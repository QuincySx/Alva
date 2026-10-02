// INPUT: Empty-conversation callbacks
// OUTPUT: Non-executing prompt actions and model-configuration guidance
// POS: Home foundation behavior with no engine/provider calls.
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { UIProvider } from "../ui";
import { ConversationStart } from "./conversation-start";

it("fills a draft only and provides a real settings action when no model exists", () => {
  const prompt = vi.fn(); const settings = vi.fn();
  render(<UIProvider><ConversationStart modelAvailable={false} onPrompt={prompt} onSettings={settings} /></UIProvider>);
  fireEvent.click(screen.getByRole("button", { name: "Inspect this workspace and explain its structure." }));
  expect(prompt).toHaveBeenCalledWith("Inspect this workspace and explain its structure.");
  fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
  expect(settings).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/Nothing runs until you send/)).toBeTruthy();
});
