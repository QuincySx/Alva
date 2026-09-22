// INPUT:  A list of entries, a scroll-owner element, per-item measured heights
// OUTPUT: BoundedList — windowed rendering with dynamic-height support and
//         read-anchor preservation
// POS:    H21. Only a viewport-sized slice of items is mounted (pad spacers
//         keep total scroll height exact against measured/estimated heights);
//         unmounted history is DOM reuse policy, NOT hidden content — the
//         full list stays in the data array and scrolling mounts any part of
//         it. The single scroll owner stays the shared [data-chat-scroll-region]
//         element; this component owns measurement corrections and follow
//         intent, distinguishing layout clamps from external scroll movement.
//
//         Row-level memoization belongs to the ROW component (pass a
//         React.memo'd renderer); BoundedList only bounds how many rows exist.

import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

export interface WindowRange {
  start: number;
  end: number;
  padTop: number;
  padBottom: number;
}

/** Pure window math — exported for unit tests (jsdom has no layout). */
export function computeWindow(
  prefix: Float64Array,
  scrollTop: number,
  clientHeight: number,
  overscan: number,
): { start: number; end: number; padTop: number; padBottom: number } {
  const n = prefix.length - 1;
  // First index whose bottom edge is below the viewport top.
  let lo = 0;
  let hi = n;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (prefix[mid + 1] > scrollTop) hi = mid;
    else lo = mid + 1;
  }
  const first = lo;
  let end = first;
  const viewportBottom = scrollTop + clientHeight;
  while (end < n && prefix[end] < viewportBottom) end += 1;
  const start = Math.max(0, first - overscan);
  const endPad = Math.min(n, end + overscan);
  return {
    start,
    end: endPad,
    padTop: prefix[start],
    padBottom: prefix[n] - prefix[endPad],
  };
}

/** Register a row node for measurement and anchor tracking. Rows call this
 *  once in a `useCallback`-stable ref with their key. */
export type RowRegister = (key: string, node: HTMLElement | null) => void;

/** Hook helper for rows: a stable ref that registers with BoundedList. */
export function useRowRef(register: RowRegister, key: string) {
  return useCallback(
    (node: HTMLElement | null) => register(key, node),
    [register, key],
  );
}

export interface BoundedListProps<T> {
  items: T[];
  keyOf: (item: T, index: number) => string;
  /** Renders one row; the row MUST call useRowRef(register, keyOf(item)) and
   *  attach the ref to its outer element. */
  renderItem: (item: T, index: number, register: RowRegister) => ReactNode;
  /** Height estimate for not-yet-measured items (px). */
  estimate?: number;
  overscan?: number;
  /** Shared scroll element; corrections and bottom following are owned here. */
  scrollerRef: { current: HTMLElement | null };
  followRef?: { current: boolean };
  onFollowChange?: (following: boolean) => void;
  className?: string;
  ariaLabel?: string;
  testId?: string;
}

export function BoundedList<T>({
  items, keyOf, renderItem, estimate = 140, overscan = 6,
  scrollerRef, followRef, onFollowChange, className, ariaLabel, testId,
}: BoundedListProps<T>) {
  const listRef = useRef<HTMLOListElement>(null);
  const heights = useRef(new Map<string, number>());
  const nodes = useRef(new Map<string, HTMLElement>());
  const observer = useRef<ResizeObserver | null>(null);
  const frame = useRef<number | null>(null);
  const correctedTop = useRef<number | null>(null);
  const correctedSize = useRef({ height: 0, total: 0 });
  const [revision, setRevision] = useState(0);
  const [viewport, setViewport] = useState({ top: 0, height: 0 });
  const keys = useMemo(() => items.map(keyOf), [items, keyOf]);
  const prefix = useMemo(() => {
    const sums = new Float64Array(keys.length + 1);
    keys.forEach((key, i) => { sums[i + 1] = sums[i] + (heights.current.get(key) ?? estimate); });
    return sums;
  }, [keys, estimate, revision]);
  const geometry = useRef({ keys, prefix });
  const anchor = useRef<{ key: string; offset: number; bottom: boolean } | null>(null);

  const listTop = useCallback(() => {
    const scroller = scrollerRef.current;
    const list = listRef.current;
    if (!scroller || !list) return 0;
    return list.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
  }, [scrollerRef]);

  const readViewport = useCallback(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const top = Math.max(0, scroller.scrollTop - listTop());
    setViewport((old) => Math.abs(old.top - top) < 0.5 && old.height === scroller.clientHeight
      ? old : { top, height: scroller.clientHeight });
    const { keys: currentKeys, prefix: sums } = geometry.current;
    const visible = computeWindow(sums, top, scroller.clientHeight, 0).start;
    if (currentKeys[visible]) anchor.current = {
      key: currentKeys[visible],
      offset: top - sums[visible],
      bottom: followRef?.current ?? scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48,
    };
  }, [scrollerRef, listTop, followRef]);

  const measure = useCallback(() => {
    let changed = false;
    nodes.current.forEach((node, key) => {
      const height = node.getBoundingClientRect().height;
      if (height > 0 && Math.abs(height - (heights.current.get(key) ?? 0)) > 0.5) {
        heights.current.set(key, height);
        changed = true;
      }
    });
    if (changed) setRevision((value) => value + 1);
  }, []);

  const register = useCallback<RowRegister>((key, node) => {
    const previous = nodes.current.get(key);
    if (previous) observer.current?.unobserve(previous);
    if (node) {
      nodes.current.set(key, node);
      observer.current?.observe(node);
    } else nodes.current.delete(key);
  }, []);

  useLayoutEffect(() => {
    // Resolve before the parent's trailing ref/effects: the first measured
    // window must already know which element owns scrolling.
    scrollerRef.current ??= listRef.current?.closest<HTMLElement>("[data-chat-scroll-region]") ?? null;
    const scroller = scrollerRef.current;
    const saved = anchor.current;
    geometry.current = { keys, prefix };
    if (scroller) {
      if (followRef?.current ?? saved?.bottom) scroller.scrollTop = scroller.scrollHeight;
      else if (saved) {
        const index = keys.indexOf(saved.key);
        if (index >= 0) scroller.scrollTop = listTop() + prefix[index] + saved.offset;
      }
      correctedTop.current = scroller.scrollTop;
      correctedSize.current = { height: scroller.clientHeight, total: scroller.scrollHeight };
    }
    readViewport();
    measure();
  }, [keys, prefix, viewport.height, listTop, readViewport, measure, scrollerRef, followRef]);

  useEffect(() => {
    // Cached heights are session data, not an unbounded global history.
    const live = new Set(keys);
    for (const key of heights.current.keys()) if (!live.has(key)) heights.current.delete(key);
  }, [keys]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const schedule = () => {
      if (frame.current !== null) return;
      frame.current = requestAnimationFrame(() => {
        frame.current = null;
        readViewport();
      });
    };
    const onScroll = () => {
      // Measurement corrections generate scroll events too. Only a movement
      // away from our last applied position changes the user's follow intent.
      const layoutChanged = correctedSize.current.height !== scroller.clientHeight
        || correctedSize.current.total !== scroller.scrollHeight;
      if (followRef && !layoutChanged && scroller.scrollTop !== correctedTop.current) {
        followRef.current = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48;
        onFollowChange?.(followRef.current);
      }
      readViewport();
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    // Dynamic row heights (streaming, wrapping, font changes) and viewport resize.
    if (typeof ResizeObserver !== "undefined") {
      observer.current = new ResizeObserver(() => {
        measure();
        if (followRef?.current) {
          scroller.scrollTop = scroller.scrollHeight;
          correctedTop.current = scroller.scrollTop;
          correctedSize.current = { height: scroller.clientHeight, total: scroller.scrollHeight };
        }
        schedule();
      });
      nodes.current.forEach((node) => observer.current?.observe(node));
      observer.current.observe(scroller);
    }
    schedule();
    return () => {
      scroller.removeEventListener("scroll", onScroll);
      observer.current?.disconnect();
      observer.current = null;
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      frame.current = null;
    };
  }, [measure, readViewport, scrollerRef, followRef, onFollowChange]);

  const range = computeWindow(prefix, viewport.top, viewport.height || 800, overscan);
  return (
    <ol ref={listRef} aria-label={ariaLabel} className={className}
      data-bounded-list="" data-testid={testId}
      style={{ paddingTop: range.padTop, paddingBottom: range.padBottom, overflowAnchor: "none" }}>
      {items.slice(range.start, range.end).map((item, index) => {
        const absolute = range.start + index;
        return <Fragment key={keys[absolute]}>{renderItem(item, absolute, register)}</Fragment>;
      })}
    </ol>
  );
}
