// INPUT:  ChatEntry list, StreamEvent-shaped delta JSON from AgentEvent.MessageUpdate
// OUTPUT: pure reducers that accumulate streaming assistant/thinking text
// POS:    Shared streaming logic for Home (and tests) — no React, no Tauri.

import type { ChatEntry } from "./agent-bridge";

/** Flags tracking whether the current LLM turn has an open bubble. */
export interface StreamFlags {
  assistantOpen: boolean;
  thinkingOpen: boolean;
}

export const INITIAL_STREAM_FLAGS: StreamFlags = {
  assistantOpen: false,
  thinkingOpen: false,
};

export function closeStreams(_flags?: StreamFlags): StreamFlags {
  return { assistantOpen: false, thinkingOpen: false };
}

/**
 * Append a text delta to the live assistant bubble.
 *
 * Correctness rule: all decisions use the previous `messages` snapshot so
 * back-to-back deltas (many MessageUpdate events before React re-renders)
 * compose. The open flag alone is not enough — if it says open but the last
 * assistant bubble is missing (or not last), we still recover by creating a
 * new bubble rather than silently dropping text.
 */
export function appendAssistantDelta(
  messages: ChatEntry[],
  flags: StreamFlags,
  text: string,
): { messages: ChatEntry[]; flags: StreamFlags } {
  if (!text) {
    return { messages, flags };
  }

  if (flags.assistantOpen) {
    for (let i = messages.length - 1; i >= 0; i--) {
      const entry = messages[i];
      if (entry.type === "assistant") {
        const next = messages.slice();
        next[i] = { type: "assistant", text: entry.text + text };
        return {
          messages: next,
          // Thinking is no longer the live stream once text starts.
          flags: { assistantOpen: true, thinkingOpen: false },
        };
      }
    }
  }

  return {
    messages: [...messages, { type: "assistant", text }],
    flags: { assistantOpen: true, thinkingOpen: false },
  };
}

/**
 * Append a reasoning/thinking delta. Same recovery rules as assistant text.
 */
export function appendThinkingDelta(
  messages: ChatEntry[],
  flags: StreamFlags,
  text: string,
): { messages: ChatEntry[]; flags: StreamFlags } {
  if (!text) {
    return { messages, flags };
  }

  if (flags.thinkingOpen) {
    for (let i = messages.length - 1; i >= 0; i--) {
      const entry = messages[i];
      if (entry.type === "thinking") {
        const next = messages.slice();
        next[i] = { type: "thinking", text: entry.text + text };
        return {
          messages: next,
          flags: { assistantOpen: false, thinkingOpen: true },
        };
      }
    }
  }

  return {
    messages: [...messages, { type: "thinking", text }],
    flags: { assistantOpen: false, thinkingOpen: true },
  };
}

/**
 * Parse the nested StreamEvent JSON that AgentEvent.MessageUpdate carries.
 *
 * Wire shape (externally tagged, no rename_all on StreamEvent):
 *   { "TextDelta": { "text": "..." } }
 *   { "ReasoningDelta": { "text": "..." } }
 *   { "ReasoningBlock": { "text": "...", "signature": null } }
 */
export type ParsedDelta =
  | { kind: "text"; text: string }
  | { kind: "reasoning"; text: string }
  | { kind: "reasoning_block"; text: string }
  | { kind: "other" };

export function parseStreamDelta(delta: unknown): ParsedDelta {
  if (!delta || typeof delta !== "object") {
    return { kind: "other" };
  }
  const d = delta as Record<string, unknown>;

  // Primary wire shape (externally tagged Rust enum).
  const textDelta = d.TextDelta as { text?: string } | undefined;
  if (textDelta && typeof textDelta.text === "string" && textDelta.text.length > 0) {
    return { kind: "text", text: textDelta.text };
  }

  const reasoningDelta = d.ReasoningDelta as { text?: string } | undefined;
  if (
    reasoningDelta &&
    typeof reasoningDelta.text === "string" &&
    reasoningDelta.text.length > 0
  ) {
    return { kind: "reasoning", text: reasoningDelta.text };
  }

  const reasoningBlock = d.ReasoningBlock as { text?: string } | undefined;
  if (
    reasoningBlock &&
    typeof reasoningBlock.text === "string" &&
    reasoningBlock.text.length > 0
  ) {
    return { kind: "reasoning_block", text: reasoningBlock.text };
  }

  // Defensive: accept snake_case if a future rename_all lands.
  const snakeText = d.text_delta as { text?: string } | undefined;
  if (snakeText && typeof snakeText.text === "string" && snakeText.text.length > 0) {
    return { kind: "text", text: snakeText.text };
  }
  const snakeReasoning = d.reasoning_delta as { text?: string } | undefined;
  if (
    snakeReasoning &&
    typeof snakeReasoning.text === "string" &&
    snakeReasoning.text.length > 0
  ) {
    return { kind: "reasoning", text: snakeReasoning.text };
  }

  return { kind: "other" };
}

/**
 * Apply a MessageUpdate delta to the chat list. Returns null if the delta
 * is not a text/reasoning variant we render.
 */
export function applyMessageUpdateDelta(
  messages: ChatEntry[],
  flags: StreamFlags,
  delta: unknown,
): { messages: ChatEntry[]; flags: StreamFlags } | null {
  const parsed = parseStreamDelta(delta);
  switch (parsed.kind) {
    case "text":
      return appendAssistantDelta(messages, flags, parsed.text);
    case "reasoning":
      return appendThinkingDelta(messages, flags, parsed.text);
    case "reasoning_block":
      // Seal thinking with the authoritative block text when provided.
      return sealThinking(messages, flags, parsed.text);
    case "other":
      return null;
  }
}

/** Close the thinking stream; optionally replace the bubble with sealed text. */
export function sealThinking(
  messages: ChatEntry[],
  flags: StreamFlags,
  sealedText: string | null,
): { messages: ChatEntry[]; flags: StreamFlags } {
  const nextFlags = { ...flags, thinkingOpen: false };
  if (sealedText === null) {
    return { messages, flags: nextFlags };
  }
  for (let i = messages.length - 1; i >= 0; i--) {
    const entry = messages[i];
    if (entry.type === "thinking") {
      const next = messages.slice();
      next[i] = { type: "thinking", text: sealedText };
      return { messages: next, flags: nextFlags };
    }
  }
  // No open thinking bubble — surface the sealed block as a new one.
  return {
    messages: [...messages, { type: "thinking", text: sealedText }],
    flags: nextFlags,
  };
}
