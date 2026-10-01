// INPUT: ToolContent component and controlled paged bridge responses
// OUTPUT: Full-text paging, stale-response, legacy and error regressions
// POS: UI-level contract for bounded reachable tool bodies.

import { cleanup, fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getHubToolContent, type ToolContentChunk, type ToolContentRef } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { ToolContent } from "./tool-content";

vi.mock("../bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../bridge")>(), getHubToolContent: vi.fn(),
}));
const getContent = vi.mocked(getHubToolContent);
const reference: ToolContentRef = { content_id: "content-1", chunk_count: 2, total_bytes: 30_000 };
const chunk = (index: number, text: string): ToolContentChunk => ({ ...reference, chunk_index: index, text });
const view = (sessionId = "session-1", content: ToolContentRef | null = reference) =>
  <I18nProvider><UIProvider>
    <ToolContent key={content?.content_id ?? "legacy"} sessionId={sessionId} content={content} preview="preview only" label="Tool output" />
  </UIProvider></I18nProvider>;

beforeEach(() => { getContent.mockReset(); localStorage.setItem("alva-hub.language", "en-US"); });
afterEach(cleanup);

it("automatically loads first page and replaces rather than accumulates pages", async () => {
  getContent.mockImplementation(async (_, __, index) => chunk(index, index === 0 ? "first full page" : "🙂last page<script>inert</script>"));
  const { container } = render(view());
  await screen.findByText("first full page");
  expect(getContent).toHaveBeenCalledWith("session-1", "content-1", 0);
  fireEvent.click(screen.getByRole("button", { name: "Next text page" }));
  await screen.findByText("🙂last page<script>inert</script>");
  expect(screen.queryByText("first full page")).toBeNull();
  expect(container.querySelector("script")).toBeNull();
  expect(screen.getByRole("button", { name: "Next text page" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Previous text page" }));
  await screen.findByText("first full page");
  expect(container.querySelectorAll("pre")).toHaveLength(1);
});

it("does not display an old session response after switching", async () => {
  let resolveOld!: (value: ToolContentChunk) => void;
  getContent.mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }));
  getContent.mockResolvedValueOnce(chunk(0, "new session body"));
  const rendered = render(view());
  rendered.rerender(view("session-2"));
  await screen.findByText("new session body");
  await act(async () => resolveOld(chunk(0, "old session body")));
  expect(screen.queryByText("old session body")).toBeNull();
});

it("reports full-text failure and retries without claiming the preview is complete", async () => {
  getContent.mockRejectedValueOnce(new Error("private backend details"));
  getContent.mockResolvedValueOnce(chunk(0, "recovered body"));
  render(view());
  await screen.findByText(/Full text could not be loaded/);
  expect(screen.queryByText("private backend details")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Retry text loading" }));
  await screen.findByText("recovered body");
});

it("labels legacy previews and does not fetch nonexistent full text", async () => {
  render(view("session-1", null));
  expect(screen.getByText(/Preview only: no full-text record/)).toBeVisible();
  await waitFor(() => expect(getContent).not.toHaveBeenCalled());
});
