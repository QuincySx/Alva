// INPUT:  streamReducer pure functions
// OUTPUT: process-exit non-zero on assertion failure
// POS:    Zero-dep smoke tests for streaming accumulation (run via tsx).

import assert from "node:assert/strict";
import type { ChatEntry } from "./agent-bridge";
import {
  INITIAL_STREAM_FLAGS,
  appendAssistantDelta,
  appendThinkingDelta,
  applyMessageUpdateDelta,
  closeStreams,
  parseStreamDelta,
  sealThinking,
  type StreamFlags,
} from "./streamReducer";

function texts(messages: ChatEntry[]): string[] {
  return messages.map((m) => {
    if ("text" in m) return `${m.type}:${m.text}`;
    return m.type;
  });
}

// -- parseStreamDelta -------------------------------------------------------

assert.deepEqual(parseStreamDelta({ TextDelta: { text: "hi" } }), {
  kind: "text",
  text: "hi",
});
assert.deepEqual(parseStreamDelta({ ReasoningDelta: { text: "think" } }), {
  kind: "reasoning",
  text: "think",
});
assert.deepEqual(
  parseStreamDelta({ ReasoningBlock: { text: "full", signature: null } }),
  { kind: "reasoning_block", text: "full" },
);
assert.equal(parseStreamDelta({ Start: null }).kind, "other");
assert.equal(parseStreamDelta(null).kind, "other");

// -- text deltas accumulate into one assistant bubble ----------------------

{
  let messages: ChatEntry[] = [{ type: "user", text: "hi" }];
  let flags: StreamFlags = { ...INITIAL_STREAM_FLAGS };
  for (const chunk of ["Hel", "lo ", "world"]) {
    const out = appendAssistantDelta(messages, flags, chunk);
    messages = out.messages;
    flags = out.flags;
  }
  assert.equal(flags.assistantOpen, true);
  assert.deepEqual(texts(messages), ["user:hi", "assistant:Hello world"]);
}

// -- back-to-back deltas still compose when open flag races ahead ----------
// Simulates the old bug: flag already true but messages snapshot has not
// yet received the first assistant bubble (would drop chunks).

{
  let messages: ChatEntry[] = [{ type: "user", text: "q" }];
  // Flag pretends the stream is open even though no assistant bubble exists.
  let flags: StreamFlags = { assistantOpen: true, thinkingOpen: false };
  const out1 = appendAssistantDelta(messages, flags, "A");
  messages = out1.messages;
  flags = out1.flags;
  const out2 = appendAssistantDelta(messages, flags, "B");
  messages = out2.messages;
  flags = out2.flags;
  assert.deepEqual(texts(messages), ["user:q", "assistant:AB"]);
}

// -- thinking then text produces two bubbles --------------------------------

{
  let messages: ChatEntry[] = [];
  let flags = { ...INITIAL_STREAM_FLAGS };
  ({ messages, flags } = appendThinkingDelta(messages, flags, "step1 "));
  ({ messages, flags } = appendThinkingDelta(messages, flags, "step2"));
  ({ messages, flags } = appendAssistantDelta(messages, flags, "answer"));
  assert.deepEqual(texts(messages), [
    "thinking:step1 step2",
    "assistant:answer",
  ]);
  assert.equal(flags.assistantOpen, true);
  assert.equal(flags.thinkingOpen, false);
}

// -- tool boundary (closeStreams) starts a fresh assistant bubble ----------

{
  let messages: ChatEntry[] = [];
  let flags = { ...INITIAL_STREAM_FLAGS };
  ({ messages, flags } = appendAssistantDelta(messages, flags, "before tool"));
  flags = closeStreams(flags);
  messages = [
    ...messages,
    {
      type: "tool_call",
      id: "t1",
      name: "bash",
      arguments: {},
      result: "ok",
      is_error: false,
    },
  ];
  ({ messages, flags } = appendAssistantDelta(messages, flags, "after tool"));
  assert.deepEqual(texts(messages), [
    "assistant:before tool",
    "tool_call",
    "assistant:after tool",
  ]);
}

// -- applyMessageUpdateDelta end-to-end -------------------------------------

{
  let messages: ChatEntry[] = [];
  let flags = { ...INITIAL_STREAM_FLAGS };
  const r1 = applyMessageUpdateDelta(messages, flags, {
    TextDelta: { text: "x" },
  });
  assert.notEqual(r1, null);
  messages = r1!.messages;
  flags = r1!.flags;
  const r2 = applyMessageUpdateDelta(messages, flags, {
    TextDelta: { text: "y" },
  });
  assert.notEqual(r2, null);
  assert.deepEqual(texts(r2!.messages), ["assistant:xy"]);
}

// -- sealThinking replaces streamed thinking text --------------------------

{
  let messages: ChatEntry[] = [];
  let flags = { ...INITIAL_STREAM_FLAGS };
  ({ messages, flags } = appendThinkingDelta(messages, flags, "partial"));
  ({ messages, flags } = sealThinking(messages, flags, "authoritative full"));
  assert.deepEqual(texts(messages), ["thinking:authoritative full"]);
  assert.equal(flags.thinkingOpen, false);
}

console.log("streamReducer tests: ok");
