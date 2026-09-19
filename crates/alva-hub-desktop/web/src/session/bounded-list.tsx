// INPUT:  A list of entries, a scroll-owner element, per-item measured heights
// OUTPUT: BoundedList — windowed rendering with dynamic-height support and
//         read-anchor preservation
// POS:    H21. Only a viewport-sized slice of items is mounted (pad spacers
//         keep total scroll height exact against measured/estimated heights);
//         unmounted history is DOM reuse policy, NOT hidden content — the
//         full list stays in the data array and scrolling mounts any part of
//         it. The single scroll owner stays the shared [data-chat-scroll-region]
//         element; this component only READS it (except anchor corrections,
//         which compensate its own mounting effects).
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
  /** The shared scroll owner (read-only). */
  scrollerRef: { current: HTMLElement | null };
  className?: string;
  ariaLabel?: string;
  testId?: string;
}

export function BoundedList<T>({
  items,
  keyOf,
  renderItem,
  estimate = 140,
  overscan = 6,
  scrollerRef,
  className,
  ariaLabel,
  testId,
}: BoundedListProps<T>) {
  const [range, setRange] = useState<WindowRange>(() => ({
    start: 0,
    end: Math.min(items.length, overscan * 2),
    padTop: 0,
    padBottom: 0,
  }));
  const heightsRef = useRef(new Map<string, number>());
  const rafRef = useRef<number | null>(null);
  // Visual anchor: (key, content-space top) of the first visible item —
  // refreshed after every correction so DOM changes (window shifts, height
  // measures, prepends) can be compensated synchronously before paint.
  const anchorRef = useRef<{ key: string; top: number } | null>(null);
  const nodesRef = useRef(new Map<string, HTMLElement>());

  const prefix = useMemo(() => {
    const heights = heightsRef.current;
    const result = new Float64Array(items.length + 1);
    for (let i = 0; i < items.length; i += 1) {
      const measured = heights.get(keyOf(items[i], i));
      result[i + 1] = result[i] + (measured ?? estimate);
    }
    return result;
    // heightsRef is intentionally not a dependency: measures arrive via
    // register and are folded in by the post-commit recompute below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, estimate]);

  const recompute = useCallback(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const next = computeWindow(prefix, scroller.scrollTop, scroller.clientHeight, overscan);
    setRange((current) =>
      current.start === next.start && current.end === next.end &&
      current.padTop === next.padTop && current.padBottom === next.padBottom
        ? current
        : next,
    );
  }, [prefix, overscan, scrollerRef]);

  const scheduleRecompute = useCallback(() => {
    if (rafRef.current !== null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      recompute();
    });
  }, [recompute]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const onScroll = () => scheduleRecompute();
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, [scheduleRecompute, scrollerRef]);

  // Recompute after every commit (mount, data change, measure) — cheap when
  // nothing moved because setRange bails on identical windows.
  useEffect(() => {
    scheduleRecompute();
  });

  // Synchronous anchor correction before paint.
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let best: { key: string; top: number } | null = null;
    for (const [key, node] of nodesRef.current) {
      const top = node.offsetTop;
      if (top + node.offsetHeight > scroller.scrollTop && (best === null || top < best.top)) {
        best = { key, top };
      }
    }
    const anchor = anchorRef.current ?? best;
    if (anchor) {
      const node = nodesRef.current.get(anchor.key);
      if (node) {
        const delta = node.offsetTop - anchor.top;
        if (Math.abs(delta) > 0.5) {
          scroller.scrollTop += delta;
        }
        anchorRef.current = { key: anchor.key, top: node.offsetTop };
      }
    } else if (best) {
      anchorRef.current = best;
    }
  });

  const register = useCallback((key: string, node: HTMLElement | null) => {
    if (node) {
      nodesRef.current.set(key, node);
      const measured = node.offsetHeight;
      if (measured > 0) heightsRef.current.set(key, measured);
    } else {
      nodesRef.current.delete(key);
    }
  }, []);

  const slice = items.slice(range.start, range.end);

  return (
    <ol
      aria-label={ariaLabel}
      className={className}
      data-bounded-list=""
      data-testid={testId}
      style={{ paddingTop: range.padTop, paddingBottom: range.padBottom }}
    >
      {slice.map((item, index) => {
        const absolute = range.start + index;
        return (
          <Fragment key={keyOf(item, absolute)}>
            {renderItem(item, absolute, register)}
          </Fragment>
        );
      })}
    </ol>
  );
}
