import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

interface ResizableSplitProps {
  /** Left-hand panel (e.g., the nav sidebar). */
  left: ReactNode;
  /** Right-hand content. */
  right: ReactNode;
  /** localStorage key used to persist the left width. */
  storageKey: string;
  /** Default width in px when no persisted value exists. */
  defaultWidth?: number;
  /** Hard minimum width in px. */
  minWidth?: number;
  /** Hard maximum width in px. */
  maxWidth?: number;
  /** When true, the left pane collapses to `collapsedWidth` with animation. */
  collapsed?: boolean;
  /** Width used when collapsed. Default 48 — enough for icon-only nav. */
  collapsedWidth?: number;
}

/**
 * Horizontal two-pane split with a draggable divider and optional collapse.
 * When `collapsed`, the left pane animates to width 0; on expand it animates
 * back to the last user-set width. The transition is disabled during active
 * drags so the cursor and panel stay locked together.
 */
export function ResizableSplit({
  left,
  right,
  storageKey,
  defaultWidth = 240,
  minWidth = 160,
  maxWidth = 480,
  collapsed = false,
  collapsedWidth = 48,
}: ResizableSplitProps) {
  const [width, setWidth] = useState<number>(() => {
    try {
      const raw = localStorage.getItem(storageKey);
      const n = raw ? parseInt(raw, 10) : NaN;
      if (Number.isFinite(n) && n >= minWidth && n <= maxWidth) return n;
    } catch {
      // ignore
    }
    return defaultWidth;
  });

  const [isDragging, setIsDragging] = useState(false);
  const startXRef = useRef(0);
  const startWidthRef = useRef(width);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      setIsDragging(true);
      startXRef.current = e.clientX;
      startWidthRef.current = width;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    },
    [width],
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!isDragging) return;
      const dx = e.clientX - startXRef.current;
      const next = Math.min(
        maxWidth,
        Math.max(minWidth, startWidthRef.current + dx),
      );
      setWidth(next);
    },
    [isDragging, minWidth, maxWidth],
  );

  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!isDragging) return;
      setIsDragging(false);
      (e.target as HTMLElement).releasePointerCapture(e.pointerId);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    },
    [isDragging],
  );

  useEffect(() => {
    try {
      localStorage.setItem(storageKey, String(width));
    } catch {
      // ignore
    }
  }, [storageKey, width]);

  const effectiveWidth = collapsed ? collapsedWidth : width;
  // Only animate on collapse toggle, not while the user is dragging the divider.
  const transitionClass = isDragging
    ? ""
    : "transition-[width] duration-200 ease-out";

  return (
    <div className="flex h-full w-full">
      <div
        style={{ width: effectiveWidth }}
        className={`shrink-0 overflow-hidden ${transitionClass}`}
      >
        {left}
      </div>
      {!collapsed && (
        <div
          role="separator"
          aria-orientation="vertical"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          className="w-[3px] shrink-0 cursor-col-resize bg-neutral-800 hover:bg-blue-600/50 active:bg-blue-600 transition-colors"
        />
      )}
      <div className="flex-1 min-w-0 overflow-hidden">{right}</div>
    </div>
  );
}
