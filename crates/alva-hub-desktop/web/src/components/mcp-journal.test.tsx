// INPUT: Controlled read-only journal loader and real shared UI/i18n
// OUTPUT: Metadata-only opening, bounded pages, search/retry/scope and inert text regressions
// POS: Account-free MCP journal UX evidence, not real provider or native acceptance.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { type McpJournalPage } from "../bridge";
import * as bridge from "../bridge";
import { demoDetail, demoInspection } from "../demo-data";
import { UIProvider } from "../ui";
import { EngineInspectorPanel } from "./engine-inspector";
import { McpJournal } from "./mcp-journal";
const index = (id: string, cursor = 1, more = false): McpJournalPage => ({ hits: [{ record_id: id, seq: cursor, timestamp: 1, server_id: id, direction: "incoming", source_bytes: 600000 }], next_cursor: cursor, through: 40, has_more: more });
const text = (id: string, offset = 0, content = "chunk", more = false): McpJournalPage => ({ record_id: id, text: content, offset, next_offset: offset + content.length, total_bytes: 600000, has_more: more });
describe("MCP journal inspection", () => {
  it("opens metadata only, replaces text pages and preserves snapshot cursors", async () => {
    const load = vi.fn().mockResolvedValueOnce(index("a", 20, true)).mockResolvedValueOnce(text("a", 0, "first", true)).mockResolvedValueOnce(text("a", 5, "<script>inert</script>last"));
    const { container } = render(<UIProvider><McpJournal productSessionId="p" nativeSessionId="n" load={load} /></UIProvider>);
    await screen.findByText("20 · a");
    expect(load).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenLastCalledWith("p", "n", expect.objectContaining({ action: "list" }));
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    await screen.findByText("first");
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await screen.findByText("<script>inert</script>last");
    expect(screen.queryByText("first")).toBeNull(); expect(container.querySelector("script")).toBeNull();
    expect(load).toHaveBeenLastCalledWith("p", "n", { action: "read", record_id: "a", offset: 5 });
    load.mockResolvedValueOnce(index("a", 20, true)).mockResolvedValueOnce(index("b", 40));
    fireEvent.click(screen.getByRole("button", { name: "Back to index" }));
    await screen.findByText("20 · a");
    expect(screen.getByRole("button", { name: "Open" })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await screen.findByText("40 · b");
    expect(screen.queryByText("20 · a")).toBeNull();
    expect(load).toHaveBeenLastCalledWith("p", "n", expect.objectContaining({ action: "list", after_seq: 20, through: 40 }));
  });
  it("searches literal keywords and does not mistake an empty scan page for completion", async () => {
    const load = vi.fn().mockResolvedValue({ hits: [], next_cursor: 128, through: 500, has_more: true });
    render(<McpJournal productSessionId="p" nativeSessionId="n" load={load} />);
    await screen.findByText(/More of this snapshot/);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "日志%_*🙂" } });
    fireEvent.click(screen.getByRole("button", { name: "Search journal" }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith("p", "n", expect.objectContaining({ action: "search_history", query: "日志%_*🙂", after_seq: 0 })));
    await waitFor(() => expect(screen.getByRole("button", { name: "Next page" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith("p", "n", expect.objectContaining({ action: "search_history", after_seq: 128, through: 500 })));
  });
  it("retries failures without backend details and rejects wrong record replies", async () => {
    const load = vi.fn().mockRejectedValueOnce(new Error("private-host-path")).mockResolvedValueOnce(index("a")).mockResolvedValueOnce(text("wrong-record"));
    render(<McpJournal productSessionId="p" nativeSessionId="n" load={load} />);
    await screen.findByRole("alert"); expect(screen.queryByText("private-host-path")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry journal read" })); await screen.findByRole("button", { name: "Open" });
    fireEvent.click(screen.getByRole("button", { name: "Open" })); await screen.findByRole("alert"); expect(screen.queryByText("chunk")).toBeNull();
  });
  it("discards old reads and resets native/product cursors on scope changes", async () => {
    let finish!: (page: McpJournalPage) => void;
    const load = vi.fn().mockResolvedValueOnce(index("a")).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; })).mockResolvedValueOnce(index("b"));
    const { rerender } = render(<McpJournal productSessionId="p" nativeSessionId="child" load={load} />);
    await screen.findByRole("button", { name: "Open" }); fireEvent.click(screen.getByRole("button", { name: "Open" }));
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    rerender(<McpJournal productSessionId="other" nativeSessionId="grandchild" load={load} />);
    await screen.findByText("1 · b");
    await act(async () => finish(text("a", 0, "old-child-data")));
    expect(screen.queryByText("old-child-data")).toBeNull();
    expect(load).toHaveBeenLastCalledWith("other", "grandchild", expect.objectContaining({ action: "list", after_seq: 0 }));
  });
  it("clears the prior body immediately on cursor navigation and rejects wrong offsets", async () => {
    let finish!: (page: McpJournalPage) => void;
    const load = vi.fn().mockResolvedValueOnce(index("a"))
      .mockResolvedValueOnce(text("a", 0, "first", true))
      .mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    render(<McpJournal productSessionId="p" nativeSessionId="n" load={load} />);
    await screen.findByRole("button", { name: "Open" });
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    await screen.findByText("first");
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    expect(screen.queryByText("first")).toBeNull();
    await waitFor(() => expect(load).toHaveBeenCalledTimes(3));
    await act(async () => finish(text("a", 0, "wrong-offset")));
    await screen.findByRole("alert");
    expect(screen.queryByText("wrong-offset")).toBeNull();
  });
  it("opens the production Inspector journal only on demand for the current child", async () => {
    const journal = vi.spyOn(bridge, "inspectHubMcpJournal").mockResolvedValue(index("child-index"));
    const load = vi.fn(async (_product: string, native?: string | null) => demoInspection(native ?? undefined));
    try {
      render(<UIProvider><EngineInspectorPanel detail={demoDetail()} loadInspection={load} /></UIProvider>);
      await screen.findByText(/Child Agent demo-nat/i);
      const origin = (key: string) => document.querySelector(`[data-inspector-origin="${key}"]`) as HTMLButtonElement;
      fireEvent.click(origin("child:demo-native-child"));
      await waitFor(() => expect(load).toHaveBeenLastCalledWith("ps-demo-1", "demo-native-child"));
      await screen.findByText(/Root session \/ alva · Child session/i);
      expect(journal).not.toHaveBeenCalled();
      fireEvent.click(origin("mcp"));
      await screen.findByText("1 · child-index");
      expect(journal).toHaveBeenLastCalledWith("ps-demo-1", "demo-native-child", expect.objectContaining({ action: "list" }));
      fireEvent.keyDown(document.querySelector(".hub-engine-inspector")!, { key: "Escape" });
      await waitFor(() => expect(origin("mcp")).toHaveFocus());
      expect(screen.queryByText("1 · child-index")).toBeNull();
    } finally {
      journal.mockRestore();
    }
  });
});
