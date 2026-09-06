// INPUT:  App.tsx pure helpers (exported for testability)
// OUTPUT: deterministic assertions over claim parsing, timeline projection,
//         readiness reason copy and claim description copy
// POS:    Web-layer unit tests. The projection and copy logic is the contract
//         the timeline cards render; pinning it here keeps UI regressions
//         detectable without a webview.

import { describe, expect, it } from "vitest";
import {
  describeClaim,
  describeReadinessReason,
  formatRelativeTime,
  mergeLiveTimeline,
  parseClaims,
  projectTimeline,
  statusLabelFor,
} from "./App";
import type { HubSessionDetail } from "./bridge";
import type { HubTimelineEvent } from "./components/hub-timeline";

describe("parseClaims", () => {
  it("parses the documented shorthand forms", () => {
    expect(
      parseClaims("src/lib.rs, read:docs, facility:target-dir, tool:git"),
    ).toEqual([
      { kind: "path", path: "src/lib.rs", mode: "write" },
      { kind: "path", path: "docs", mode: "read" },
      { kind: "facility", name: "target-dir" },
      { kind: "exclusive_tool", tool: "git" },
    ]);
  });

  it("tolerates whitespace, empty entries and blank input", () => {
    expect(parseClaims("  a.rs , ,  b.rs  ")).toEqual([
      { kind: "path", path: "a.rs", mode: "write" },
      { kind: "path", path: "b.rs", mode: "write" },
    ]);
    expect(parseClaims("")).toEqual([]);
    expect(parseClaims("   ")).toEqual([]);
  });
});

describe("describeClaim", () => {
  it("renders each claim kind readably", () => {
    expect(describeClaim({ kind: "path", path: "src/lib.rs", mode: "write" }))
      .toBe("write src/lib.rs");
    expect(describeClaim({ kind: "facility", name: "target-dir" })).toBe(
      "facility target-dir",
    );
    expect(describeClaim({ kind: "exclusive_tool", tool: "git" })).toBe(
      "tool git",
    );
  });
});

describe("describeReadinessReason", () => {
  it("explains every failure code", () => {
    expect(
      describeReadinessReason({ code: "binary_not_found", searched: ["/opt"] }),
    ).toContain("Not installed");
    expect(
      describeReadinessReason({ code: "version_unreadable", detail: "no output" }),
    ).toContain("no output");
    expect(
      describeReadinessReason({
        code: "version_incompatible",
        found: "1.0",
        minimum: "2.0",
      }),
    ).toContain("1.0 is older than the required 2.0");
    expect(
      describeReadinessReason({ code: "unauthenticated", remedy: "Run login" }),
    ).toBe("Run login");
    expect(
      describeReadinessReason({
        code: "provider_key_missing",
        remedy: "Add a key",
      }),
    ).toBe("Add a key");
    expect(
      describeReadinessReason({ code: "stream_failing", last_failure: "EOF" }),
    ).toContain("EOF");
  });
});

function sampleDetail(): HubSessionDetail {
  return {
    session: {
      product_session_id: "ps-1",
      workspace: "/tmp/proj",
      workspace_kind: "project",
      active_branch_id: "b-claude",
      engine_id: "claude",
      model_id: "engine-default",
      provider_id: "claude-cli",
      engine_session_id: null,
      status: "active",
      updated_at: 115,
      last_prompt: "Fix the build",
      last_result: "Done",
      permission_profile: "workspace_write",
      include_full_transcript: false,
    },
    turns: [
      {
        turn_id: "turn-1",
        prompt: "Fix the build",
        result: "Done",
        error: null,
        status: "completed",
        created_at: 110,
        completed_at: 110,
        attempts: [
          {
            attempt_id: "a-1",
            turn_id: "turn-1",
            branch_id: "b-claude",
            engine_id: "claude",
            status: "completed",
            failure_reason: null,
            usage: null,
            side_effects: [],
            started_at: 105,
            completed_at: 110,
          },
        ],
      },
    ],
    pending_switch: null,
    fallback_policy: { targets: [], updated_at: 0 },
    timeline_events: [
      {
        event_id: "e-1",
        kind: "engine_fallback",
        source_branch_id: "b-alva",
        target_branch_id: "b-claude",
        source_engine_id: "alva",
        target_engine_id: "claude",
        reason: "rate limited",
        created_at: 108,
      },
      {
        event_id: "e-2",
        kind: "engine_switch_queued",
        source_branch_id: "b-claude",
        target_branch_id: "b-codex",
        source_engine_id: "claude",
        target_engine_id: "codex",
        reason: "user requested",
        created_at: 115,
      },
    ],
    handoffs: [
      {
        handoff_id: "h-1",
        product_session_id: "ps-1",
        source_branch_id: "b-alva",
        target_branch_id: "b-claude",
        source: {
          engine_id: "alva",
          model_id: "workspace-config",
          provider_id: "workspace-config",
          account_source: "Workspace provider",
          billing_route: "Workspace provider account",
          automatic_use_authorized: true,
          required_capabilities: [],
          reasoning_effort: "high",
          fast_mode: false,
        },
        target: {
          engine_id: "claude",
          model_id: "engine-default",
          provider_id: "claude-cli",
          account_source: "Claude Code CLI account",
          billing_route: "Anthropic account",
          automatic_use_authorized: true,
          required_capabilities: [],
          reasoning_effort: "high",
          fast_mode: false,
        },
        kind: "full",
        reason: "rate limited",
        user_goal: "Fix the build",
        latest_instruction: "Keep going",
        accepted_decisions: ["no force push"],
        completed_work: [
          { prompt: "Fix the build", result: "Done" },
        ],
        pending_work: [{ prompt: "Run tests", reason: "not yet" }],
        changed_files: ["src/main.rs"],
        recent_commands: ["cargo test"],
        verification_results: ["cargo test passes"],
        workspace_summary: "Rust workspace",
        team_snapshot: null,
        transcript: [{ prompt: "Fix the build", result: "Done" }],
        session_memory: [],
        available_skills: [],
    available_mcp: [],
        created_at: 108,
        used_at: null,
      },
    ],
    branches: [],
    memory: [],
  };
}

describe("projectTimeline", () => {
  it("projects turns and switches into one time-ordered surface", () => {
    const projected = projectTimeline(sampleDetail());
    expect(projected.map((event) => event.status)).toEqual([
      "fallback",
      "user_message",
      "assistant_message",
      "queued",
    ]);
    // A turn without its own engine attribution reports the last attempt's.
    expect(projected[2].engine_id).toBe("claude");
  });

  it("attaches the matching handoff to a fallback event for the controls", () => {
    const [fallback] = projectTimeline(sampleDetail());
    expect(fallback.fallback?.source_engine_id).toBe("alva");
    expect(fallback.fallback?.handoff?.kind).toBe("full");
    // The handoff must match by branch AND timestamp, or the card loses it.
    expect(fallback.fallback?.handoff?.handoff_id).toBe("h-1");
  });

  it("classifies the remaining event kinds onto stable statuses", () => {
    const detail = sampleDetail();
    detail.timeline_events = [
      { ...detail.timeline_events[0], kind: "fallback_exhausted" },
      { ...detail.timeline_events[0], kind: "permission_decision" },
      { ...detail.timeline_events[0], kind: "worker_outcome" },
      { ...detail.timeline_events[0], kind: "engine_switch_cancelled" },
    ];
    const projected = projectTimeline(detail);
    // The turn from the fixture projects too — assert the classified kinds
    // first, then the full order.
    expect(projected.map((event) => event.status)).toEqual([
      "exhausted",
      "activity_completed",
      "worker",
      "cancelled",
      "user_message",
      "assistant_message",
    ]);
  });
});

describe("mergeLiveTimeline", () => {
  it("finishes a streamed assistant item without duplicating its answer", () => {
    const streamed: HubTimelineEvent = {
      status: "assistant_message",
      role: "assistant",
      engine_id: "alva",
      product_session_id: "ps-1",
      message: "Hello world",
      created_at: 100,
      item_id: "assistant-1",
    };
    const completed: HubTimelineEvent = {
      status: "completed",
      engine_id: "alva",
      product_session_id: "ps-1",
      message: null,
      created_at: 101,
      item_id: "assistant-1",
    };

    expect(mergeLiveTimeline([streamed], completed)).toEqual([
      {
        ...completed,
        role: "assistant",
        started_at: 100,
        message: "Hello world",
      },
    ]);
  });
});

describe("formatRelativeTime", () => {
  const NOW = 1_000_000_000_000;

  it("renders recency labels", () => {
    expect(formatRelativeTime(NOW - 30_000, NOW)).toBe("just now");
    expect(formatRelativeTime(NOW - 2 * 60_000, NOW)).toBe("2m ago");
    expect(formatRelativeTime(NOW - 3 * 3600_000, NOW)).toBe("3h ago");
    expect(formatRelativeTime(NOW - 2 * 86400_000, NOW)).toBe("2d ago");
    expect(formatRelativeTime(NOW - 30 * 86400_000, NOW)).toBe(
      new Date(NOW - 30 * 86400_000).toISOString().slice(0, 10),
    );
  });

  it("never goes negative for future timestamps", () => {
    expect(formatRelativeTime(NOW + 60_000, NOW)).toBe("just now");
  });
});

describe("statusLabelFor", () => {
  // Assert the message key, not the copy: this pins behaviour without going
  // red every time the wording is edited.
  const key = ((k: string) => k) as never;

  const event = (status: string): HubTimelineEvent =>
    ({
      status,
      role: "assistant",
      engine_id: "alva",
      product_session_id: "session-1",
      message: "hi",
      created_at: 1,
      item_id: "assistant-1",
    }) as HubTimelineEvent;

  it("does not claim the engine is replying once the attempt is over", () => {
    // A turn replayed from the store ends on `assistant_message` —
    // `projectTimeline` only special-cases failed/running/interrupted — so the
    // header used to sit on "…is replying" forever after any reload.
    expect(statusLabelFor(event("assistant_message"), false, key)).toBe(
      "status.completed",
    );
  });

  it("still reports live progress while the attempt is running", () => {
    expect(statusLabelFor(event("assistant_message"), true, key)).toBe(
      "status.responding",
    );
    expect(statusLabelFor(event("activity_started"), true, key)).toBe(
      "status.usingTool",
    );
  });

  it("keeps terminal outcomes distinguishable when the attempt is over", () => {
    expect(statusLabelFor(event("failed"), false, key)).toBe("status.failed");
    expect(statusLabelFor(event("interrupted"), false, key)).toBe(
      "status.interrupted",
    );
    expect(statusLabelFor(event("exhausted"), false, key)).toBe(
      "status.fallbackExhausted",
    );
  });

  it("reports ready for a session that has not run yet", () => {
    expect(statusLabelFor(undefined, false, key)).toBe("status.ready");
  });
});
