// INPUT: Cursor-paged message journal and current live timeline entries
// OUTPUT: mergeMessagePage and useMessageReplay recovery polling
// POS: Gap recovery shared by initial history, live overlap, and reconnect.

import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { getHubMessagePage, type MessageJournalPage } from "../bridge";
import { mergeEntry, type TimelineEntry } from "./timeline-model";

export function mergeMessagePage(current: TimelineEntry[], page: MessageJournalPage): TimelineEntry[] {
  const covered = new Set(page.records.map((record) => `${record.turn_id}:assistant`));
  let next = current.filter((entry) => !entry.itemId || !covered.has(entry.itemId) || !!entry.message_position);
  const settled = new Map<string, "completed" | "failed" | "interrupted">();
  for (const record of page.records) {
    next = mergeEntry(next, record.event);
    // Crash-repaired attempts have no final live event. The durable attempt
    // state settles every message without inventing a successful completion.
    if (["completed", "failed", "interrupted"].includes(record.attempt_status)) {
      if (record.event.message_position) settled.set(record.event.message_position.stream_id, record.attempt_status as "completed" | "failed" | "interrupted");
      next = mergeEntry(next, { ...record.event, message: null,
        status: record.attempt_status as "completed" | "failed" | "interrupted" });
    }
  }
  return next.map((entry) => {
    const status = entry.message_position ? settled.get(entry.message_position.stream_id) : undefined;
    return status ? { ...entry, status } : entry;
  });
}

export function useMessageReplay(sessionId: string | null, revision: number, setTimeline: Dispatch<SetStateAction<TimelineEntry[]>>) {
  const [errorSession, setErrorSession] = useState<string | null>(null);
  useEffect(() => {
    if (!sessionId) return;
    let active = true;
    let cursor = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function recover() {
      try {
        let through: number | undefined;
        do {
          const page = await getHubMessagePage(sessionId!, cursor, through);
          if (!active) return;
          if (page.records.some((record) => record.event.product_session_id !== sessionId)
            || page.next_cursor < cursor || (page.has_more && page.next_cursor === cursor)) throw new Error("invalid message page");
          if (page.records.length) setTimeline((current) => active ? mergeMessagePage(current, page) : current);
          cursor = page.next_cursor;
          through = page.through;
          if (!page.has_more) break;
        } while (active);
        if (active) setErrorSession(null);
      } catch {
        if (active) setErrorSession(sessionId);
      } finally {
        // Polling also covers a dropped Tauri event or an SSE disconnect. A
        // fixed high-water cursor bounds each catch-up pass during streaming.
        if (active) timer = setTimeout(() => void recover(), 2000);
      }
    }
    void recover();
    return () => { active = false; if (timer) clearTimeout(timer); };
  }, [sessionId, revision, setTimeline]);
  return errorSession === sessionId && sessionId !== null;
}
