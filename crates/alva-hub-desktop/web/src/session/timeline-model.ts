// INPUT:  Durable HubSessionDetail (store rows) + live TimelineEvent stream
// OUTPUT: TimelineEntry list — one stable identity + one merge semantic for
//         BOTH history replay and realtime events
// POS:    H20 data contract. Display components consume entries and never see
//         raw store rows, so a future fold/summary strategy stays a display
//         policy and cannot rewrite the authority record. Identity is
//         scoped by session and, for tools, attempt. Tool ordering uses the
//         durable first-observation clock/sequence, never completion or ID
//         spelling. Legacy journals retain their evidence array order.

import type { HubSessionDetail } from "../bridge";
import type { HubTimelineEvent } from "../components/hub-timeline";

export type EntryKind = "message" | "activity" | "control";

export interface TimelineEntry extends HubTimelineEvent {
  /** Canonical cross-restart identity: `${product_session_id}::${identity}`
   *  where identity is the engine item id, or a deterministic synthesis from
   *  the event facts when the engine sent none. Never an array index. */
  readonly id: string;
  /** Engine-provided item identity (turn-scoped message id, tool call id,
   *  attempt id, permission request id) — null when the event carried none. */
  readonly itemId: string | null;
  /** `${turn_id}:user` / `${turn_id}:assistant` item ids attribute the entry
   *  to a product turn; null for tool/attempt/permission items. */
  readonly turnId: string | null;
  /** Attempt lifecycle items (running/completed/failed/interrupted) carry the
   *  attempt id as their item id; null otherwise. */
  readonly attemptId: string | null;
  /** Tool call id for activity entries. */
  readonly toolId: string | null;
  readonly kind: EntryKind;
}

const TURN_ITEM = /^(.+):(user|assistant)$/;

function stableHash(text: string): string {
  let hash = 2166136261;
  for (let i = 0; i < text.length; i += 1) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

/** Deterministic identity for events the engine sent without an item id:
 *  the same fact replayed later synthesizes the same id and dedupes. */
function synthesizedIdentity(event: HubTimelineEvent): string {
  const facts = [
    event.status,
    String(event.created_at ?? 0),
    event.message ?? "",
    event.activity?.tool_name ?? "",
    event.activity?.path ?? "",
    event.permission_request?.request_id ?? "",
  ].join("\u001f");
  return `anon-${stableHash(facts)}`;
}

/** Classify one event and resolve its canonical identity.
 *
 * Role decides message-ness FIRST (R29: the item's role, not the incoming
 * status — a streamed assistant item stays a message even when the terminal
 * merge flips its status); item-id shape is the fallback signal for live
 * events that arrive without a role. */
export function toEntry(event: HubTimelineEvent): TimelineEntry {
  const itemId = event.item_id ?? null;
  const turnMatch = itemId ? TURN_ITEM.exec(itemId) : null;
  let kind: EntryKind;
  let turnId: string | null = null;
  let attemptId: string | null = null;
  let toolId: string | null = null;
  if (event.role === "user" || event.role === "assistant") {
    kind = "message";
    turnId = turnMatch ? turnMatch[1] : null;
  } else if (turnMatch) {
    kind = "message";
    turnId = turnMatch[1];
  } else if (event.activity) {
    kind = "activity";
    toolId = itemId;
    attemptId = event.activity.attempt_id ?? null;
  } else if (event.status === "permission_required") {
    kind = "control";
  } else if (itemId) {
    kind = "control";
    attemptId = itemId;
  } else {
    kind = "control";
  }
  return {
    ...event,
    role: event.role,
    id: event.activity?.attempt_id
      ? JSON.stringify([event.product_session_id, event.activity.attempt_id, itemId ?? synthesizedIdentity(event)])
      : `${event.product_session_id}::${itemId ?? synthesizedIdentity(event)}`,
    itemId,
    turnId,
    attemptId,
    toolId,
    kind,
  };
}

/** Deterministic display order. created_at first (product clock), then kind
 *  weight so a turn's message sorts after its own tool activity emitted at
 *  the same tick, then canonical id as the final stable tiebreak. */
export function entrySortKey(entry: TimelineEntry): [number, number, string] {
  const kindWeight = entry.kind === "activity" ? 0 : entry.kind === "control" ? 1 : 2;
  return [entry.activity?.first_seen_at ?? entry.started_at ?? entry.created_at ?? 0, kindWeight, entry.id];
}

export function sortEntries(entries: TimelineEntry[]): TimelineEntry[] {
  return [...entries].sort((left, right) => {
    const a = entrySortKey(left);
    const b = entrySortKey(right);
    const toolOrder = left.kind === "activity" && right.kind === "activity"
      && left.attemptId === right.attemptId
      ? (left.activity?.sequence ?? 0) - (right.activity?.sequence ?? 0) : 0;
    return a[0] - b[0] || a[1] - b[1] || toolOrder || (a[2] < b[2] ? -1 : a[2] > b[2] ? 1 : 0);
  });
}

/**
 * THE merge semantic — used identically by history replay and live appends.
 *
 * Message items: an `assistant_message` merge APPENDS text (streaming delta);
 * any other status merge keeps the body the user was already reading and
 * updates status only (R29). Activity/control items: the newest body wins
 * wholesale so tool cards and failure cards never show stale captions (R40).
 *
 * Duplicate delivery of the same fact (late event, reconnect backfill) merges
 * into the same entry instead of duplicating it.
 */
export function mergeEntry(current: TimelineEntry[], incoming: HubTimelineEvent): TimelineEntry[] {
  const observed = toEntry(incoming);
  const index = current.findIndex((candidate) => candidate.id === observed.id);
  if (index < 0) {
    return sortEntries([...current, observed]);
  }
  const merged = mergeEntryAncestry(current[index], observed);
  const next = [...current];
  next[index] = merged;
  return next;
}

function mergeEntryAncestry(previous: TimelineEntry, incoming: TimelineEntry): TimelineEntry {
  const derivedRole = previous.role ?? incoming.role;
  const base: TimelineEntry = {
    ...previous,
    ...incoming,
    // Attribution/promotion: keep the richer identity fields already known.
    id: previous.itemId ? previous.id : incoming.id,
    itemId: previous.itemId ?? incoming.itemId,
    turnId: previous.turnId ?? incoming.turnId,
    attemptId: previous.attemptId ?? incoming.attemptId,
    toolId: previous.toolId ?? incoming.toolId,
    kind: previous.kind,
    role: derivedRole,
    started_at: previous.started_at ?? previous.created_at ?? incoming.created_at,
    activity: incoming.activity ? {
      ...incoming.activity,
      first_seen_at: previous.activity?.first_seen_at ?? incoming.activity.first_seen_at,
      sequence: previous.activity?.sequence ?? incoming.activity.sequence,
      path: incoming.activity.path ?? previous.activity?.path ?? null,
      tool_input: incoming.activity.tool_input ?? previous.activity?.tool_input,
      tool_output: incoming.activity.tool_output ?? previous.activity?.tool_output,
    } : previous.activity,
  };
  const messageItem = base.kind === "message";
  if (!messageItem) {
    return { ...base, message: incoming.message ?? null };
  }
  if (incoming.status === "assistant_message") {
    // Append streaming deltas — but a redelivery whose text equals the whole
    // current body is a duplicate of the same fact, not a delta (H20: late /
    // duplicate events must not corrupt the body).
    const delta = incoming.message ?? "";
    const duplicate = delta === "" || delta === (previous.message ?? "");
    return { ...base, message: duplicate ? previous.message : `${previous.message ?? ""}${delta}` };
  }
  return { ...base, message: previous.message || incoming.message || null };
}

/** Project the durable record into display entries with the same identity and
 *  order semantics as the live path. The projector is injected (App's
 *  projectTimeline) so this module owns identity/merge only, not row mapping. */
export function projectHistory(
  detail: HubSessionDetail,
  project: (detail: HubSessionDetail) => HubTimelineEvent[],
): TimelineEntry[] {
  return sortEntries(project(detail).map(toEntry));
}

/** Session-scoped bucket: entries of different sessions never merge, and a
 *  stale flush for session A can never land in session B's bucket. */
export function entriesForSession(entries: TimelineEntry[], sessionId: string): TimelineEntry[] {
  return entries.filter((entry) => entry.product_session_id === sessionId);
}
