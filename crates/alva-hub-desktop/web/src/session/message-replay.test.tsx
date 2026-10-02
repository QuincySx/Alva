// INPUT: Journal pages, live fragments and controlled replay bridge
// OUTPUT: Gap fill, overlap, restart settlement and stale-session regression tests
// POS: Message recovery acceptance independent of transport.

import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { getHubMessagePage, type MessageJournalPage, type TimelineEvent } from "../bridge";
import { mergeEntry, type TimelineEntry } from "./timeline-model";
import { mergeMessagePage, useMessageReplay } from "./message-replay";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), getHubMessagePage: vi.fn() }));
afterEach(() => { cleanup(); vi.mocked(getHubMessagePage).mockReset(); });
const event = (offset: number, message: string | null, status = "assistant_message"): TimelineEvent => ({
  status: status as TimelineEvent["status"], product_session_id: "a", engine_id: "alva", created_at: 1,
  message, item_id: "native", message_position: { stream_id: "attempt", offset },
});
const page = (events: TimelineEvent[], state = "completed"): MessageJournalPage => ({ records: events.map((event, index) => ({ sequence: index + 1, turn_id: "turn", attempt_status: state, event })), next_cursor: events.length, through: events.length, has_more: false });

it("fills a lost initial fragment and deduplicates overlapping replay", () => {
  let entries = mergeEntry([], event(2, "🙂"));
  const journal = page([event(0, "🙂"), event(2, "🙂"), event(4, null, "completed")]);
  entries = mergeMessagePage(entries, journal);
  entries = mergeMessagePage(entries, journal);
  expect(entries).toHaveLength(1);
  expect(entries[0].message).toBe("🙂🙂");
  expect(entries[0].status).toBe("completed");
});

it("does not substitute terminal summary for missing deltas, and marks crash repair", () => {
  const terminalFirst = mergeEntry([], event(6, "different summary", "completed"));
  const restored = mergeMessagePage(terminalFirst, page([event(0, "abcdef")], "interrupted"));
  expect(restored[0].message).toBe("abcdef");
  expect(restored[0].status).toBe("interrupted");
});

it("replaces only the journal-covered legacy result, preserving users and old turns", () => {
  let entries = mergeEntry([], { ...event(0, "summary"), message_position: undefined, item_id: "turn:assistant", role: "assistant" });
  entries = mergeEntry(entries, { ...event(0, "old result"), message_position: undefined, item_id: "old:assistant", role: "assistant" });
  const restored = mergeMessagePage(entries, page([event(0, "full body")]));
  expect(restored.map((entry) => entry.message)).toEqual(expect.arrayContaining(["full body", "old result"]));
  expect(restored.some((entry) => entry.message === "summary")).toBe(false);
});

function Probe({ id }: { id: string }) {
  const [entries, setEntries] = useState<TimelineEntry[]>([]);
  useMessageReplay(id, 0, setEntries);
  return <div>{entries.map((entry) => entry.message).join("")}</div>;
}

it("does not apply late pages from the previous session", async () => {
  let finish!: (value: MessageJournalPage) => void;
  vi.mocked(getHubMessagePage).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  vi.mocked(getHubMessagePage).mockResolvedValueOnce(page([{ ...event(0, "new body"), product_session_id: "b" }]));
  const rendered = render(<Probe key="a" id="a" />);
  rendered.rerender(<Probe key="b" id="b" />);
  await screen.findByText("new body");
  await act(async () => finish(page([event(0, "old body")])));
  expect(screen.queryByText("old body")).toBeNull();
});

it("follows page cursors through a bounded high-water snapshot", async () => {
  vi.mocked(getHubMessagePage).mockResolvedValueOnce({ ...page([event(0, "a")]), through: 2, has_more: true });
  vi.mocked(getHubMessagePage).mockResolvedValueOnce({ ...page([event(1, "b")]), next_cursor: 2, through: 2 });
  render(<Probe id="a" />);
  await waitFor(() => expect(screen.getByText("ab")).toBeVisible());
  expect(getHubMessagePage).toHaveBeenNthCalledWith(2, "a", 1, 2);
});
