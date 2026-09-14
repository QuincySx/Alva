// INPUT:  Projected Hub events (user/assistant messages), i18n, clipboard
// OUTPUT: ChatTranscript — the always-visible message reading flow with safe
//         Markdown rendering, per-codeblock copy, and follow/jump-to-bottom
// POS:    R29/R30 chat-first transcript. Execution diagnostics and the tool
//         timeline live in the Inspector workbench, NOT here; this component
//         renders messages only and never unmounts them on terminal events.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ArrowDown, Bot, Check, Copy, User } from "lucide-react";
import type { HubTimelineEvent } from "./hub-timeline";
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
function isPinnedToBottom(element: HTMLElement): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight < 48;
}

export interface ChatTranscriptProps {
  /** aria-controls id of the scroll container this transcript renders into. */
  events: HubTimelineEvent[];
  /** The live stream may still be appending deltas to the last message. */
  streaming: boolean;
}

export function ChatTranscript({ events, streaming }: ChatTranscriptProps) {
  const { localize, t } = useI18n();
  const messages = useMemo(() => events.filter(isMessageEntry), [events]);
  const scrollerRef = useRef<HTMLElement | null>(null);
  const [pinned, setPinned] = useState(true);

  // Follow the stream while pinned; preserve the reading position otherwise.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (scroller && pinned) {
      scroller.scrollTop = scroller.scrollHeight;
    }
  }, [messages, pinned]);

  const onScroll = useCallback(() => {
    const scroller = scrollerRef.current;
    if (scroller) setPinned(isPinnedToBottom(scroller));
  }, []);

  const jumpToBottom = useCallback(() => {
    const scroller = scrollerRef.current;
    if (scroller) {
      scroller.scrollTop = scroller.scrollHeight;
      setPinned(true);
    }
  }, []);

  if (messages.length === 0) return null;

  return (
    <div className="chat-transcript" data-testid="chat-transcript">
      <ol
        aria-label={t("chat.transcriptLabel")}
        className="chat-transcript__list"
      >
        {messages.map((entry, index) => {
          const isUser = messageRole(entry) === "user";
          return (
            <li
              className={`chat-message chat-message--${isUser ? "user" : "assistant"}`}
              data-status={entry.status}
              key={`${entry.product_session_id}-${entry.item_id ?? index}`}
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
                {entry.message
                  ? renderMarkdown(isUser ? entry.message : localizeSystemFallback(entry.message, localize))
                  : entry.status === "running"
                    ? <p className="chat-paragraph">{t("timeline.executing")}</p>
                    : null}
                {streaming && !isUser && index === messages.length - 1 && entry.status === "running" ? (
                  <span aria-hidden="true" className="chat-cursor" />
                ) : null}
              </div>
              {entry.status === "interrupted" ? (
                <small className="chat-hint">{t("timeline.interruptedHint")}</small>
              ) : null}
            </li>
          );
        })}
      </ol>
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
            element.onscroll = onScroll;
            scrollerRef.current = element;
          }
        }
      }} />
    </div>
  );
}

/** Assistant text stays as-is (it is model prose, not localized system copy);
 *  this hook exists so future system-authored messages localize correctly. */
function localizeSystemFallback(text: string, localize: (value: string | null | undefined) => string): string {
  return localize(text) || text;
}
