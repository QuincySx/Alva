// INPUT:  timeline-model pure functions + synthetic durable/live events
// OUTPUT: H20 contract tests — identity, unified merge semantics, replay/live
//         convergence, session isolation
// POS:    Pins the ONE merge semantic both paths must share. These tests are
//         the acceptance evidence for "实时与重启回放得到同一列表".

import { describe, expect, it } from "vitest";
import {
  entriesForSession,
  mergeEntry,
  projectHistory,
  sortEntries,
  toEntry,
  type TimelineEntry,
} from "./timeline-model";
import type { HubSessionDetail, TimelineEvent } from "../bridge";
import type { HubTimelineEvent } from "../components/hub-timeline";

const SESSION = "ps-1";

it("retains tool input and path when the completion carries only output", () => {
  const started = mergeEntry([], liveEvent({ status: "activity_started", item_id: "call-1",
    activity: { tool_name: "read_file", succeeded: null, duration_ms: null, path: "src/main.rs", tool_input: "arguments" },
  }));
  const completed = mergeEntry(started, liveEvent({ status: "activity_completed", item_id: "call-1",
    activity: { tool_name: "read_file", succeeded: true, duration_ms: 12, path: null, tool_output: "result" },
  }));
  expect(completed).toHaveLength(1);
  expect(completed[0].activity).toMatchObject({ path: "src/main.rs", tool_input: "arguments", tool_output: "result" });
});

function liveEvent(overrides: Partial<HubTimelineEvent> & { status: TimelineEvent["status"] }): TimelineEvent {
  return {
    engine_id: "alva",
    product_session_id: SESSION,
    message: null,
    ...overrides,
  } as TimelineEvent;
}

function entryOf(overrides: Partial<HubTimelineEvent> & { status: HubTimelineEvent["status"] }): TimelineEntry {
  return toEntry(liveEvent(overrides) as HubTimelineEvent);
}

describe("toEntry identity", () => {
  it("parses turn-scoped message ids into message entries with turn attribution", () => {
    const entry = entryOf({ status: "user_message", role: "user", message: "hi", created_at: 10, item_id: "t1:user" });
    expect(entry.kind).toBe("message");
    expect(entry.turnId).toBe("t1");
    expect(entry.id).toBe(`${SESSION}::t1:user`);
  });

  it("classifies tool call ids as activities with tool attribution", () => {
    const entry = entryOf({
      status: "activity_started",
      message: null,
      created_at: 11,
      item_id: "call-9",
      activity: { tool_name: "bash", succeeded: null, duration_ms: null, path: null },
    });
    expect(entry.kind).toBe("activity");
    expect(entry.toolId).toBe("call-9");
    expect(entry.turnId).toBeNull();
  });

  it("classifies attempt-lifecycle ids as control entries", () => {
    const entry = entryOf({ status: "running", message: null, created_at: 12, item_id: "attempt-3" });
    expect(entry.kind).toBe("control");
    expect(entry.attemptId).toBe("attempt-3");
  });

  it("synthesizes a deterministic id for events without item ids", () => {
    const first = entryOf({ status: "running", message: null, created_at: 13 });
    const replay = entryOf({ status: "running", message: null, created_at: 13 });
    expect(first.itemId).toBeNull();
    expect(first.id).toBe(replay.id);
    expect(first.id).not.toContain("undefined");
  });
});

describe("mergeEntry — the one merge semantic", () => {
  it("appends streaming deltas to the same assistant item", () => {
    let entries = [entryOf({ status: "assistant_message", role: "assistant", message: "Hel", created_at: 10, item_id: "t1:assistant" })];
    entries = mergeEntry(entries, liveEvent({ status: "assistant_message", message: "lo w", created_at: 11, item_id: "t1:assistant" }) as HubTimelineEvent);
    expect(entries).toHaveLength(1);
    expect(entries[0].message).toBe("Hello w");
  });

  it("keeps the streamed body when a terminal status merges into the message item (R29)", () => {
    let entries = [entryOf({ status: "assistant_message", role: "assistant", message: "answer text", created_at: 10, item_id: "t1:assistant" })];
    entries = mergeEntry(entries, liveEvent({ status: "failed", message: "attempt failed", created_at: 12, item_id: "t1:assistant" }) as HubTimelineEvent);
    expect(entries).toHaveLength(1);
    expect(entries[0].status).toBe("failed");
    expect(entries[0].message).toBe("answer text");
  });

  it("lets the newest body win for control items (R40)", () => {
    let entries = [entryOf({ status: "running", message: "working", created_at: 10, item_id: "attempt-3" })];
    entries = mergeEntry(entries, liveEvent({ status: "failed", message: "crashed", created_at: 12, item_id: "attempt-3" }) as HubTimelineEvent);
    expect(entries).toHaveLength(1);
    expect(entries[0].message).toBe("crashed");
  });

  it("dedupes duplicate delivery by position, never by text", () => {
    const event = liveEvent({ status: "assistant_message", message: "done", created_at: 10, item_id: "native-id",
      message_position: { stream_id: "attempt-1", offset: 0 } });
    let entries = mergeEntry([], event);
    entries = mergeEntry(entries, { ...event });
    expect(entries).toHaveLength(1);
    expect(entries[0].message).toBe("done");
  });

  it("retains identical consecutive legacy deltas even in the same millisecond", () => {
    const event = liveEvent({ status: "assistant_message", message: "哈", created_at: 10, item_id: "native-id" });
    const entries = mergeEntry(mergeEntry([], event), { ...event });
    expect(entries[0].message).toBe("哈哈");
    expect(entries[0].kind).toBe("message");
    expect(entries[0].role).toBe("assistant");
  });

  it("fills reordered Unicode fragments and ignores earlier redelivery after later text", () => {
    const fragment = (offset: number, message: string) => liveEvent({
      status: "assistant_message", message, item_id: "native-id",
      message_position: { stream_id: "attempt-1", offset },
    });
    let entries = mergeEntry([], fragment(4, "好"));
    entries = mergeEntry(entries, fragment(2, "🙂"));
    entries = mergeEntry(entries, fragment(4, "好"));
    expect(entries[0].message).toBe("");
    entries = mergeEntry(entries, fragment(0, "🙂"));
    expect(entries[0].message).toBe("🙂🙂好");
    expect(entries[0].pendingText).toBeUndefined();
    entries = mergeEntry(entries, fragment(2, "🙂"));
    expect(entries[0].message).toBe("🙂🙂好");
    entries = mergeEntry(entries, { ...fragment(0, "new"), message_position: { stream_id: "attempt-2", offset: 0 } });
    expect(entries).toHaveLength(2);
  });

  it("does not resurrect a terminal message when its missing fragment arrives", () => {
    const event = liveEvent({status: "assistant_message", message: "a", item_id: "native-id",
      message_position: { stream_id: "attempt-1", offset: 0 }});
    let entries = mergeEntry([], event);
    entries = mergeEntry(entries, { ...event, message: "c", message_position: { stream_id: "attempt-1", offset: 2 } });
    entries = mergeEntry(entries, { ...event, status: "failed", message: "error" });
    entries = mergeEntry(entries, { ...event, message: "b", message_position: { stream_id: "attempt-1", offset: 1 } });
    expect(entries[0].message).toBe("abc");
    expect(entries[0].status).toBe("failed");
  });

  it("never merges parallel tools that share a tool name", () => {
    let entries = [entryOf({
      status: "activity_started", message: null, created_at: 10, item_id: "call-a",
      activity: { tool_name: "bash", succeeded: null, duration_ms: null, path: null },
    })];
    entries = mergeEntry(entries, liveEvent({
      status: "activity_started", message: null, created_at: 11, item_id: "call-b",
      activity: { tool_name: "bash", succeeded: null, duration_ms: null, path: null },
    }) as HubTimelineEvent);
    expect(entries).toHaveLength(2);
    expect(entries.map((entry) => entry.toolId).sort()).toEqual(["call-a", "call-b"]);
  });

  it("inserts a late event at its sorted position, not at the end", () => {
    let entries = [
      entryOf({ status: "user_message", role: "user", message: "first", created_at: 100, item_id: "t1:user" }),
      entryOf({ status: "assistant_message", role: "assistant", message: "reply", created_at: 300, item_id: "t2:assistant" }),
    ];
    entries = mergeEntry(entries, liveEvent({ status: "user_message", role: "user", message: "late", created_at: 200, item_id: "t1b:user" }) as HubTimelineEvent);
    expect(entries.map((entry) => entry.turnId)).toEqual(["t1", "t1b", "t2"]);
  });
});

describe("replay/live convergence", () => {
  const detail: HubSessionDetail = {
    session: {
      product_session_id: SESSION, workspace: "/tmp/w", workspace_kind: "project",
      active_branch_id: "b1", engine_id: "alva", model_id: "m", provider_id: "p",
      engine_session_id: null, status: "completed", updated_at: 999,
      last_prompt: null, last_result: null, permission_profile: "workspace_write",
      include_full_transcript: false,
    },
    turns: [{
      turn_id: "t1",
      prompt: "do it",
      result: "did",
      error: null,
      status: "completed",
      created_at: 100,
      completed_at: 200,
      attempts: [{
        attempt_id: "a1", turn_id: "t1", branch_id: "b1", engine_id: "alva",
        status: "completed", failure_reason: null, usage: null,
        side_effects: [{
          tool_call_id: "call-1", tool_name: "read_file", succeeded: true,
          duration_ms: 50, path: "src/lib.rs",
        }],
        started_at: 110, completed_at: 200,
      }],
    }],
    branches: [], handoffs: [], timeline_events: [], pending_switch: null,
    fallback_policy: { targets: [], updated_at: 0 },
    memory: [],
  };

  const liveFacts: HubTimelineEvent[] = [
    liveEvent({ status: "user_message", role: "user", message: "do it", created_at: 100, item_id: "t1:user" }) as HubTimelineEvent,
    liveEvent({
      status: "activity_completed", message: "read_file completed", created_at: 150, item_id: "call-1",
      activity: { tool_name: "read_file", succeeded: true, duration_ms: 50, path: "src/lib.rs" },
    }) as HubTimelineEvent,
    liveEvent({ status: "assistant_message", role: "assistant", message: "did", created_at: 200, item_id: "t1:assistant" }) as HubTimelineEvent,
  ];

  it("replaying history and feeding the same facts live converge to one list", () => {
    const replayed = projectHistory(detail, (d) => {
      // The injected projector stands in for App's projectTimeline; it must
      // produce the same facts the live stream carried.
      return liveFacts.map((fact) => ({ ...fact, product_session_id: d.session.product_session_id }));
    });
    let live: TimelineEntry[] = [];
    for (const fact of liveFacts) live = mergeEntry(live, fact);
    expect(replayed.map((entry) => entry.id)).toEqual(live.map((entry) => entry.id));
    expect(replayed.map((entry) => [entry.kind, entry.message])).toEqual(
      live.map((entry) => [entry.kind, entry.message]),
    );
  });
});

describe("session isolation", () => {
  it("keeps entries of different sessions apart", () => {
    const a = toEntry({ ...liveEvent({ status: "user_message", role: "user", message: "a", created_at: 1, item_id: "t1:user" }), product_session_id: "ps-a" } as HubTimelineEvent);
    const b = toEntry({ ...liveEvent({ status: "user_message", role: "user", message: "b", created_at: 2, item_id: "t1:user" }), product_session_id: "ps-b" } as HubTimelineEvent);
    expect(entriesForSession([a, b], "ps-a")).toHaveLength(1);
    // Same item id in another session is a DIFFERENT entry (never merged).
    expect(a.id).not.toBe(b.id);
  });
});

describe("sortEntries", () => {
  it("is stable and never uses array index as identity", () => {
    const entries = [
      entryOf({ status: "assistant_message", role: "assistant", message: null, created_at: 20, item_id: "t2:assistant" }),
      entryOf({ status: "user_message", role: "user", message: null, created_at: 10, item_id: "t2:user" }),
      entryOf({ status: "user_message", role: "user", message: null, created_at: 10, item_id: "t1:user" }),
    ];
    const sorted = sortEntries(entries);
    expect(sorted.map((entry) => entry.itemId)).toEqual(["t1:user", "t2:user", "t2:assistant"]);
    expect(sortEntries([...sorted]).map((entry) => entry.id)).toEqual(sorted.map((entry) => entry.id));
  });
});
