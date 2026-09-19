// INPUT:  entry count + seed options
// OUTPUT: deterministic mixed Hub timeline datasets (messages, markdown, code,
//         tool activities, failure/interrupt/permission states, one ~1MB item)
// POS:    H19 performance baseline. The SAME seed + entry count must produce a
//         byte-identical dataset for every future re-run, so baseline and
//         post-optimization numbers stay comparable. Never use Math.random here.

import type { HubTimelineEvent } from "../components/hub-timeline";

/** Deterministic PRNG (mulberry32). Fixed seed => reproducible datasets. */
function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const CODE_SNIPPETS: string[][] = [
  [
    "async fn execute(&self, call: ToolCall) -> Result<ToolOutput> {",
    "    let guard = self.locks.claim(&call.resource()).await?;",
    "    let outcome = match call.kind() {",
    "        Kind::Read => self.read(call.path()).await?,",
    "        Kind::Write => self.write(call.path(), call.body()).await?,",
    "        Kind::Shell => self.shell(call.argv()).await?,",
    "    };",
    "    guard.release();",
    "    Ok(outcome)",
    "}",
  ],
  [
    "export function mergeLiveTimeline(current, event) {",
    "  const index = current.findIndex((c) =>",
    "    c.product_session_id === event.product_session_id &&",
    "    c.item_id === event.item_id,",
    "  );",
    "  if (index < 0) return [...current, event];",
    "  const next = [...current];",
    "  next[index] = { ...next[index], ...event };",
    "  return next;",
    "}",
  ],
  [
    "def resolve_route(target: EngineTarget) -> Route:",
    "    routes = catalog.routes_for(target.model_id)",
    "    eligible = [r for r in routes if r.authorized and r.healthy]",
    "    if not eligible:",
    "        raise RouteUnavailable(target)",
    "    return min(eligible, key=lambda r: r.priority)",
  ],
  [
    "fn frame_interval_percentile(frames: &[u64], p: f64) -> u64 {",
    "    let mut sorted = frames.to_vec();",
    "    sorted.sort_unstable();",
    "    let rank = ((sorted.len() as f64 - 1.0) * p).round() as usize;",
    "    sorted[rank]",
    "}",
  ],
];

/** One deterministic pseudo-code block of `lines` lines. */
function codeBlock(rand: () => number, lines: number): string {
  const base = CODE_SNIPPETS[Math.floor(rand() * CODE_SNIPPETS.length)];
  const out: string[] = [];
  for (let i = 0; i < lines; i += 1) {
    out.push(base[i % base.length].replace(/\b(\w+)\b/, (name) => `${name}_${i}`));
  }
  return out.join("\n");
}

function prose(rand: () => number, paragraphs: number): string {
  const sentences = [
    "The projection layer keeps a stable identity for every entry so late events merge instead of duplicating.",
    "Streaming deltas append to the same item until the terminal state arrives.",
    "Tool output stays on the reading axis; the workbench is an auxiliary操作面.",
    "A bounded DOM keeps long sessions responsive without hiding any authority record.",
    "Scroll position is owned by one component and preserved across re-renders.",
    "Permissions surface where the work happens so approval never blocks the reading flow.",
  ];
  const out: string[] = [];
  for (let p = 0; p < paragraphs; p += 1) {
    const count = 2 + Math.floor(rand() * 3);
    const parts: string[] = [];
    for (let s = 0; s < count; s += 1) {
      parts.push(sentences[Math.floor(rand() * sentences.length)]);
    }
    out.push(parts.join(" "));
  }
  return out.join("\n\n");
}

const TOOL_CYCLE = [
  { tool_name: "read_file", path: "crates/alva-hub-desktop/src/run.rs" },
  { tool_name: "grep", path: "crates/alva-hub-desktop/web/src/App.tsx" },
  { tool_name: "bash", path: null },
  { tool_name: "write_file", path: "crates/alva-hub-desktop/web/src/perf/fixture-data.ts" },
];

/** A single message body of at least `bytes` characters inside one fenced block. */
function hugeCodeBlock(bytes: number): string {
  const lines: string[] = [];
  let total = 0;
  for (let i = 0; total < bytes; i += 1) {
    const line = `const fixture_value_${i} = 0x${(i * 2654435761 >>> 0).toString(16).padStart(8, "0")}; // deterministic perf fixture line ${i}`;
    lines.push(line);
    total += line.length + 1;
  }
  return lines.join("\n");
}

export interface PerfDatasetOptions {
  /** Approximate number of timeline entries (messages + activities). */
  entries: number;
  seed: number;
  sessionId: string;
  /** Epoch ms of the first entry. */
  baseTime: number;
}

/**
 * Build one deterministic mixed dataset. Composition per 6-entry exchange
 * cycle: short answer / list+inline-code / code block / long prose /
 * tool-heavy exchange / large code block — plus one ~1MB single output, one
 * failed turn, one interrupted turn and one permission card per dataset.
 */
export function buildPerfDataset(options: PerfDatasetOptions): HubTimelineEvent[] {
  const { entries, seed, sessionId, baseTime } = options;
  const rand = mulberry32(seed);
  const events: HubTimelineEvent[] = [];
  const engine = "alva";
  let exchange = 0;

  const push = (event: HubTimelineEvent) => {
    events.push(event);
  };

  while (events.length < entries) {
    const at = baseTime + exchange * 1500;
    push({
      status: "user_message",
      role: "user",
      engine_id: engine,
      product_session_id: sessionId,
      message: `Fixture request ${exchange}: summarize the streaming merge behavior and list what changes for tool output.`,
      created_at: at,
      item_id: `${sessionId}-gen-${exchange}:user`,
    });

    const variant = exchange % 6;
    let body: string;
    if (variant === 0) {
      body = prose(rand, 1);
    } else if (variant === 1) {
      body = [
        `### Summary ${exchange}`,
        "",
        `- merged by \`product_session_id + item_id\``,
        `- terminal events keep the streamed body — **not** replace it`,
        "",
        "Deltas append until the attempt settles.",
      ].join("\n");
    } else if (variant === 2) {
      body = `Assistant reply with an implementation sketch:\n\n\`\`\`rust\n${codeBlock(rand, 12 + Math.floor(rand() * 18))}\n\`\`\``;
    } else if (variant === 3) {
      body = `## Long answer ${exchange}\n\n${prose(rand, 5)}`;
    } else if (variant === 4) {
      body = prose(rand, 1);
    } else {
      body = `Reference implementation (long block):\n\n\`\`\`typescript\n${codeBlock(rand, 60 + Math.floor(rand() * 60))}\n\`\`\``;
    }

    // Tool-heavy exchanges carry two activity events before the answer.
    if (variant === 4) {
      for (let k = 0; k < 2; k += 1) {
        const tool = TOOL_CYCLE[(exchange + k) % TOOL_CYCLE.length];
        const started = Math.floor(rand() * 2) === 0;
        push({
          status: started ? "activity_started" : "activity_completed",
          engine_id: engine,
          product_session_id: sessionId,
          message: started
            ? `${tool.tool_name} running`
            : `${tool.tool_name} completed`,
          created_at: at + 200 + k * 150,
          item_id: `${sessionId}-gen-${exchange}-tool-${k}`,
          activity: {
            tool_name: tool.tool_name,
            succeeded: started ? null : true,
            duration_ms: started ? null : 40 + Math.floor(rand() * 900),
            path: tool.path,
          },
        });
      }
    }

    // Insert the special states at deterministic positions.
    const isFailedTurn = exchange === Math.floor(entries / 5);
    const isInterruptedTurn = exchange === Math.floor(entries / 2.5);
    const isHugeTurn = exchange === Math.max(2, Math.floor(entries * 0.8));

    if (isHugeTurn) {
      push({
        status: "assistant_message",
        role: "assistant",
        engine_id: engine,
        product_session_id: sessionId,
        message: `Full dump (single output, ~1MB):\n\n\`\`\`\n${hugeCodeBlock(1024 * 1024)}\n\`\`\``,
        created_at: at + 400,
        item_id: `${sessionId}-gen-${exchange}:assistant`,
      });
    } else if (isFailedTurn) {
      push({
        status: "failed",
        role: "assistant",
        engine_id: engine,
        product_session_id: sessionId,
        message: "Upstream connection reset before completion (fixture failure state).",
        created_at: at + 400,
        item_id: `${sessionId}-gen-${exchange}:assistant`,
      });
    } else if (isInterruptedTurn) {
      push({
        status: "interrupted",
        role: "assistant",
        engine_id: engine,
        product_session_id: sessionId,
        message: null,
        created_at: at + 400,
        item_id: `${sessionId}-gen-${exchange}:assistant`,
      });
    } else {
      push({
        status: "assistant_message",
        role: "assistant",
        engine_id: engine,
        product_session_id: sessionId,
        message: body,
        created_at: at + 400,
        item_id: `${sessionId}-gen-${exchange}:assistant`,
      });
    }

    if (exchange === Math.floor(entries / 3.5)) {
      push({
        status: "permission_required",
        engine_id: engine,
        product_session_id: sessionId,
        message: null,
        created_at: at + 600,
        item_id: `${sessionId}-gen-${exchange}-permission`,
        permission_request: {
          request_id: `${sessionId}-perm-${exchange}`,
          tool_name: "bash",
          description: "Run the deterministic fixture command: cargo test -p alva-hub-desktop",
          engine_session_id: `${sessionId}-engine`,
          resource: "~/Projects/fixture",
        },
      });
    }

    exchange += 1;
  }

  return events;
}
