// INPUT:  Ordered Hub entries, module/Markdown/attachment renderers, bounded navigation and i18n
// OUTPUT: ChatTranscript — bounded messages/execution, safe Markdown/modules, search/copy and follow navigation
// POS:    R29/R30 chat-first reading flow with H21 bounded DOM and explicit search scope.
// Message bodies
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
import { ArrowDown, Bot, User } from "lucide-react";
import type { HubTimelineEvent } from "./hub-timeline";
import { BoundedList, useRowRef, type RowRegister } from "../session/bounded-list";
import { Button, TextField } from "../ui";
import { useI18n } from "../i18n";
import { renderMarkdown } from "./safe-markdown";
import { CopyTextButton } from "./copy-text-button";
import { MessageAttachments } from "./input-attachments";

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
// Markdown is delegated to safe-markdown; user text stays literal. Every
// message/control row shares the same measured scroll owner.
// ---------------------------------------------------------------------------


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
  const navigationRef = useRef<((key: string) => void) | null>(null);
  const [query, setQuery] = useState("");
  const [matchIndex, setMatchIndex] = useState(0);
  const matches = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return needle ? messages.flatMap((entry, index) =>
      [entry.message, ...(entry.attachments ?? []).map((file) => file.name), entry.activity?.tool_name, entry.activity?.tool_input, entry.activity?.tool_output,
        entry.permission_request?.description].filter(Boolean).join("\n").toLocaleLowerCase().includes(needle)
        ? [entryKey(entry, index)] : []) : [];
  }, [messages, query]);
  const copyConversation = useCallback(() => messages.map((entry) => [
    `[${messageRole(entry) ?? entry.status}] ${entry.engine_id}`,
    entry.message ?? "", entry.activity?.tool_name ?? "", entry.activity?.tool_input ?? "", entry.activity?.tool_output ?? "",
    ...(entry.attachments ?? []).map((file) => `[attachment] ${file.name} (${file.total_bytes} B)${file.path ? ` ${file.path}` : ""}`),
  ].filter(Boolean).join("\n")).join("\n\n"), [messages]);
  function navigateMatch(direction: number) {
    if (!matches.length) return;
    const next = (matchIndex % matches.length + direction + matches.length) % matches.length;
    setMatchIndex(next);
    navigationRef.current?.(matches[next]);
  }
  const lastAssistant = messages.reduce((last, entry, index) => messageRole(entry) === "assistant" ? index : last, -1);

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
      <div className="chat-transcript__actions" role="search" aria-label={t("chat.search")}>
        <TextField aria-label={t("chat.search")} value={query} onChange={(event) => { setQuery(event.target.value); setMatchIndex(0); }} />
        <Button size="small" variant="ghost" disabled={!matches.length} onClick={() => navigateMatch(-1)}>{t("chat.previousMatch")}</Button>
        <Button size="small" variant="ghost" disabled={!matches.length} onClick={() => navigateMatch(0)}>{t("chat.showMatch")}</Button>
        <Button size="small" variant="ghost" disabled={!matches.length} onClick={() => navigateMatch(1)}>{t("chat.nextMatch")}</Button>
        {query ? <small role="status">{t("chat.matchCount", { current: matches.length ? matchIndex % matches.length + 1 : 0, count: matches.length })}</small> : null}
        <CopyTextButton text={copyConversation} label={t("chat.copyConversation")} />
        <small>{t("chat.searchScope")}</small>
      </div>
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
            streaming={streaming && index === lastAssistant}
          />
        )}
        scrollerRef={scrollerRef}
        followRef={followRef}
        onFollowChange={setPinned}
        navigationRef={navigationRef}
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
      ? isUser ? <div className="chat-user-text">{entry.message}</div> : renderMarkdown(entry.message)
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
      {entry.message ? <CopyTextButton text={entry.message} label={t("chat.copyMessage")} /> : null}
      <div className="chat-message__body">
        {body}
        {entry.attachments?.length ? <MessageAttachments sessionId={entry.product_session_id} files={entry.attachments} /> : null}
        {!entry.message && entry.status === "running" ? (
          <p className="chat-paragraph">{t("timeline.executing")}</p>
        ) : null}
        {streaming && (entry.status === "running" || entry.status === "assistant_message") ? (
          <span aria-hidden="true" className="chat-cursor" />
        ) : null}
      </div>
      {!isUser && entry.status === "failed" ? <small className="chat-hint" role="status">{t("chat.failedHint")}</small> : null}
      {entry.status === "interrupted" ? (
        <small className="chat-hint">{t("timeline.interruptedHint")}</small>
      ) : null}
    </li>
  );
});
