// INPUT:  Ordered Hub entries, optional execution renderer, i18n, clipboard
// OUTPUT: ChatTranscript — the always-visible message reading flow with safe
//         Markdown rendering, per-codeblock copy, follow/jump-to-bottom, and
//         a bounded mounted window (H21)
// POS:    R29/R30 chat-first transcript; H21 bounds the DOM. Message bodies
//         are memoized per entry so a streaming delta re-renders only the
//         tail item; history outside the window unmounts as a DOM policy —
//         the full list stays in the data array and is fully reachable by
//         scrolling. With an execution renderer every event shares one window.

import {
  memo,
  useCallback,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ArrowDown, Bot, Check, Copy, User } from "lucide-react";
import type { HubTimelineEvent } from "./hub-timeline";
import { BoundedList, useRowRef, type RowRegister } from "../session/bounded-list";
import { Button } from "../ui";
import { useI18n } from "../i18n";

/** Only the message events the transcript renders. Live events may arrive
 *  without an explicit role — user_message/assistant_message statuses carry
 *  the same meaning. */
function messageRole(entry: HubTimelineEvent): "user" | "assistant" | null {
  if (entry.role === "user" || entry.role === "assistant") return entry.role;
  if (entry.status === "user_message") return "user";
  if (entry.status === "assistant_message") return "assistant";
  return null;
}

function isMessageEntry(entry: HubTimelineEvent): boolean {
  return messageRole(entry) !== null;
}

function entryKey(entry: HubTimelineEvent, index: number): string {
  // H20 entries carry a canonical id; plain events fall back to session+item.
  if ("id" in entry && typeof entry.id === "string") return entry.id;
  return `${entry.product_session_id}-${entry.item_id ?? index}`;
}

// ---------------------------------------------------------------------------
// Safe Markdown — deliberately no third-party renderer. The assistant text is
// untrusted model output; we render a restricted block/inline subset where
// every piece of source text becomes a React text child. React escapes text
// children by construction, so injected markup can never reach the DOM —
// without double-escaping (R41): pre-escaping with escapeHtml used to surface
// literal "&lt;"/"&amp;" entities for prose like `a < b && c > d`.
// ---------------------------------------------------------------------------

/** Render fenced-code + heading + list + paragraph structure to nodes. */
function renderMarkdown(text: string): ReactNode[] {
  const lines = text.split("\n");
  const blocks: ReactNode[] = [];
  let index = 0;
  let key = 0;

  const inline = (raw: string): ReactNode => {
    // Inline code, then bold — matched on the RAW source; emitted strings are
    // React text children and stay inert (R41).
    const parts: ReactNode[] = [];
    let rest = raw;
    let inlineKey = 0;
    const pattern = /(`[^`]+`)|(\*\*[^*]+\*\*)/g;
    let cursor = 0;
    for (const match of rest.matchAll(pattern)) {
      const start = match.index ?? 0;
      if (start > cursor) parts.push(rest.slice(cursor, start));
      if (match[1]) {
        parts.push(<code className="chat-inline-code" key={`i-${inlineKey++}`}>{match[1].slice(1, -1)}</code>);
      } else if (match[2]) {
        parts.push(<strong key={`i-${inlineKey++}`}>{match[2].slice(2, -2)}</strong>);
      }
      cursor = start + match[0].length;
    }
    if (cursor < rest.length) parts.push(rest.slice(cursor));
    return <span>{parts}</span>;
  };

  while (index < lines.length) {
    const line = lines[index];
    if (line.startsWith("```")) {
      const language = line.slice(3).trim();
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].startsWith("```")) {
        body.push(lines[index]);
        index += 1;
      }
      // Consume the closing fence (or run to the end while streaming).
      if (index < lines.length) index += 1;
      blocks.push(
        <CodeBlock code={body.join("\n")} key={`code-${key++}`} language={language || null} />,
      );
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      const level = Math.min(heading[1].length + 2, 6);
      const content = inline(heading[2]);
      blocks.push(
        level <= 3
          ? <h3 className="chat-heading" key={`h-${key++}`}>{content}</h3>
          : <h4 className="chat-heading" key={`h-${key++}`}>{content}</h4>,
      );
      index += 1;
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\s*[-*]\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^\s*[-*]\s+/, ""));
        index += 1;
      }
      blocks.push(
        <ul className="chat-list" key={`ul-${key++}`}>
          {items.map((item, itemIndex) => <li key={itemIndex}>{inline(item)}</li>)}
        </ul>,
      );
      continue;
    }
    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\s*\d+\.\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^\s*\d+\.\s+/, ""));
        index += 1;
      }
      blocks.push(
        <ol className="chat-list chat-list--ordered" key={`ol-${key++}`}>
          {items.map((item, itemIndex) => <li key={itemIndex}>{inline(item)}</li>)}
        </ol>,
      );
      continue;
    }
    if (line.trim() === "") {
      index += 1;
      continue;
    }
    const paragraph: string[] = [];
    while (
      index < lines.length &&
      lines[index].trim() !== "" &&
      !lines[index].startsWith("```") &&
      !/^(#{1,4})\s+/.test(lines[index]) &&
      !/^\s*([-*]|\d+\.)\s+/.test(lines[index])
    ) {
      paragraph.push(lines[index]);
      index += 1;
    }
    blocks.push(<p className="chat-paragraph" key={`p-${key++}`}>{inline(paragraph.join(" "))}</p>);
  }
  return blocks;
}

function CodeBlock({ code, language }: { code: string; language: string | null }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copy = useCallback(() => {
    void navigator.clipboard
      .writeText(code)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
      })
      .catch(() => {
        // Clipboard permission denied — the button simply reports nothing.
      });
  }, [code]);

  return (
    <div className="chat-code-block">
      <div className="chat-code-block__header">
        <span className="chat-code-block__language">{language ?? "text"}</span>
        <Button
          aria-label={t("chat.copyCode")}
          onClick={copy}
          size="small"
          variant="ghost"
        >
          {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
          {copied ? t("chat.copied") : t("chat.copy")}
        </Button>
      </div>
      <pre className="chat-code-block__body" tabIndex={0}>
        <code>{code}</code>
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Follow / jump-to-bottom
// ---------------------------------------------------------------------------

/** Was the scroll pinned near the bottom the last time the user scrolled? */
export interface ChatTranscriptProps {
  events: HubTimelineEvent[];
  /** The live stream may still be appending deltas to the last message. */
  streaming: boolean;
  /** Product-owned actions stay outside the list/Markdown implementation. */
  renderExecution?: (entry: HubTimelineEvent) => ReactNode;
}

export function ChatTranscript({ events, streaming, renderExecution }: ChatTranscriptProps) {
  const { t } = useI18n();
  const messages = useMemo(() => renderExecution ? events : events.filter(isMessageEntry), [events, renderExecution]);
  const scrollerRef = useRef<HTMLElement | null>(null);
  const [pinned, setPinned] = useState(true);
  const followRef = useRef(true);

  const jumpToBottom = useCallback(() => {
    const scroller = scrollerRef.current;
    if (scroller) {
      followRef.current = true;
      scroller.scrollTop = scroller.scrollHeight;
      setPinned(true);
    }
  }, []);

  if (messages.length === 0) return null;

  return (
    <div className="chat-transcript" data-testid="chat-transcript">
      <BoundedList
        ariaLabel={t("chat.transcriptLabel")}
        className="chat-transcript__list"
        items={messages}
        keyOf={entryKey}
        renderItem={(entry, index, rowRegister) => (
          !isMessageEntry(entry) && renderExecution ? <ExecutionRow
            entry={entry}
            index={index}
            register={rowRegister}
            renderExecution={renderExecution}
          /> : <ChatMessageRow
            entry={entry}
            index={index}
            register={rowRegister}
            streaming={streaming && index === messages.length - 1}
          />
        )}
        scrollerRef={scrollerRef}
        followRef={followRef}
        onFollowChange={setPinned}
        testId="chat-transcript-list"
      />
      {!pinned ? (
        <Button
          aria-label={t("chat.jumpToLatest")}
          className="chat-jump"
          onClick={jumpToBottom}
          size="small"
          variant="ghost"
        >
          <ArrowDown size={14} aria-hidden="true" />
          {t("chat.jumpToLatest")}
        </Button>
      ) : null}
      {/* Register the scroll owner this transcript controls (see wire-up in
          App: the same element is the single scroll owner of the region). */}
      <span aria-hidden="true" data-chat-scroller-ref="" ref={(node) => {
        if (node) {
          // Walk to the closest scroll region ancestor and bind listeners.
          const region = node.closest("[data-chat-scroll-region]");
          if (region) {
            const element = region as HTMLElement;
            scrollerRef.current = element;
          }
        }
      }} />
    </div>
  );
}

const ExecutionRow = memo(function ExecutionRow({
  entry, index, register, renderExecution,
}: {
  entry: HubTimelineEvent;
  index: number;
  register: RowRegister;
  renderExecution: (entry: HubTimelineEvent) => ReactNode;
}) {
  const rowRef = useRowRef(register, entryKey(entry, index));
  return <li ref={rowRef} data-entry-key={entryKey(entry, index)} data-status={entry.status}>{renderExecution(entry)}</li>;
});

// ---------------------------------------------------------------------------
// Memoized message row (H21): a streaming delta re-renders ONLY this item
// when its body changed; unchanged items keep reference equality so their
// Markdown parse result is reused.
// ---------------------------------------------------------------------------

interface ChatMessageRowProps {
  entry: HubTimelineEvent;
  index: number;
  register: RowRegister;
  /** True only for the tail item while the stream is appending. */
  streaming: boolean;
}

const ChatMessageRow = memo(function ChatMessageRow({
  entry,
  index,
  register,
  streaming,
}: ChatMessageRowProps) {
  const { localize, t } = useI18n();
  const isUser = messageRole(entry) === "user";
  const key = entryKey(entry, index);
  const rowRef = useRowRef(register, key);
  const body = useMemo(
    () => (entry.message
      ? renderMarkdown(isUser ? entry.message : localizeSystemFallback(entry.message, localize))
      : null),
    [entry.message, isUser, localize],
  );

  return (
    <li
      className={`chat-message chat-message--${isUser ? "user" : "assistant"}`}
      data-status={entry.status}
      data-entry-key={key}
      ref={rowRef}
    >
      <div className="chat-message__meta">
        {isUser ? (
          <User size={14} aria-hidden="true" />
        ) : (
          <Bot size={14} aria-hidden="true" />
        )}
        <span>{isUser ? t("timeline.you") : entry.engine_id || "ALVA Agent"}</span>
        {!isUser && entry.status !== "assistant_message" ? (
          <small>{localize(entry.status)}</small>
        ) : null}
      </div>
      <div className="chat-message__body">
        {body}
        {!entry.message && entry.status === "running" ? (
          <p className="chat-paragraph">{t("timeline.executing")}</p>
        ) : null}
        {streaming && entry.status === "running" ? (
          <span aria-hidden="true" className="chat-cursor" />
        ) : null}
      </div>
      {entry.status === "interrupted" ? (
        <small className="chat-hint">{t("timeline.interruptedHint")}</small>
      ) : null}
    </li>
  );
});

/** Assistant text stays as-is (it is model prose, not localized system copy);
 *  this hook exists so future system-authored messages localize correctly. */
function localizeSystemFallback(text: string, localize: (value: string | null | undefined) => string): string {
  return localize(text) || text;
}
