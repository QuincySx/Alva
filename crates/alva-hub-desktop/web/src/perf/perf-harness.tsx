// INPUT:  URL params (n, seed, stream, streamMs), real Hub components, real CSS
// OUTPUT: Deterministic performance fixture page mounted at ?perf=1
// POS:    H19/H21 evidence harness. It mounts the SAME production components
//        (ChatTranscript / HubTimeline) inside the SAME layout primitives and
//        stylesheet as the real session view, feeds by the deterministic
//        dataset, and exposes a window.__perf probe for the measurement runner.
//        Not linked from any product surface; measurement only.

import { useCallback, useEffect, useMemo, useState } from "react";
import { mergeLiveTimeline } from "../App";
import type { HubTimelineEvent } from "../components/hub-timeline";
import { ChatTranscript } from "../components/chat-transcript";
import { HubTimeline } from "../components/hub-timeline";
import { buildPerfDataset } from "./fixture-data";
import { ContentFrame, ScrollRegion, UIProvider } from "../ui";

type SessionKey = "a" | "b";

function intParam(params: URLSearchParams, name: string, fallback: number): number {
  const raw = params.get(name);
  if (raw === null) return fallback;
  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

export default function PerfHarness() {
  const params = useMemo(() => new URLSearchParams(window.location.search), []);
  const entryCount = intParam(params, "n", 1000);
  const seed = intParam(params, "seed", 20260920);
  const streamEnabled = params.get("stream") === "1";
  const streamMs = intParam(params, "streamMs", 20000);

  const datasets = useMemo(
    () => ({
      a: buildPerfDataset({ entries: entryCount, seed, sessionId: "perf-session-a", baseTime: 1_700_000_000_000 }),
      b: buildPerfDataset({ entries: entryCount, seed: seed + 1, sessionId: "perf-session-b", baseTime: 1_700_100_000_000 }),
    }),
    [entryCount, seed],
  );

  const [active, setActive] = useState<SessionKey>("a");
  const [events, setEvents] = useState<HubTimelineEvent[]>(datasets.a);
  const [streaming, setStreaming] = useState(streamEnabled);

  const switchTo = useCallback((target: SessionKey) => {
    setActive(target);
    setEvents(datasets[target]);
  }, [datasets]);

  // Streaming: append deterministic deltas to the last assistant item through
  // the REAL merge path (mergeLiveTimeline), then settle with a terminal
  // status. The runner awaits [data-stream-state="done"].
  useEffect(() => {
    if (!streamEnabled) {
      document.documentElement.dataset.streamState = "off";
      return;
    }
    document.documentElement.dataset.streamState = "running";
    let appended = 0;
    const chunk = "The merge path appends deterministic streaming text so render work repeats under load. ";
    const timer = window.setInterval(() => {
      setEvents((current) => {
        const last = current[current.length - 1];
        const targetId = last?.role === "assistant"
          ? last.item_id ?? undefined
          : "perf-session-a-stream-tail";
        return mergeLiveTimeline(current, {
          status: "assistant_message",
          role: "assistant",
          engine_id: "alva",
          product_session_id: "perf-session-a",
          message: chunk,
          created_at: Date.now(),
          item_id: targetId ?? null,
        });
      });
      appended += 1;
      if (appended * 80 >= streamMs) {
        window.clearInterval(timer);
        setEvents((current) => {
          const last = current[current.length - 1];
          const targetId = last?.role === "assistant"
            ? last.item_id ?? undefined
            : "perf-session-a-stream-tail";
          return mergeLiveTimeline(current, {
            status: "assistant_message",
            role: "assistant",
            engine_id: "alva",
            product_session_id: "perf-session-a",
            message: "",
            created_at: Date.now(),
            item_id: targetId ?? null,
          });
        });
        setStreaming(false);
        document.documentElement.dataset.streamState = "done";
      }
    }, 80);
    return () => {
      window.clearInterval(timer);
      document.documentElement.dataset.streamState = "stopped";
    };
  }, [streamEnabled, streamMs]);

  // Measurement probe consumed by perf/run-perf.mjs.
  useEffect(() => {
    let armed = false;
    const probe = {
      domNodes: () => document.getElementsByTagName("*").length,
      scrollerStats: () => {
        const region = document.querySelector("[data-chat-scroll-region]");
        if (!(region instanceof HTMLElement)) return null;
        return {
          scrollTop: region.scrollTop,
          scrollHeight: region.scrollHeight,
          clientHeight: region.clientHeight,
          messageNodes: region.querySelectorAll(".chat-message").length,
          timelineNodes: region.querySelectorAll(".timeline-card").length,
        };
      },
      heap: () => {
        const memory = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory;
        return memory ? memory.usedJSHeapSize : null;
      },
      switchTo: (target: SessionKey) => new Promise<void>((resolve) => {
        switchTo(target);
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
      inputLatencies: [] as number[],
      // Arm before each dispatch; the next wheel/scroll event records
      // performance.timeOrigin + event.timeStamp (epoch ms).
      arm: () => {
        armed = true;
      },
    };
    (window as unknown as { __perf: typeof probe }).__perf = probe;
    // Input → event latency probe: records the epoch ms of the first wheel or
    // scroll event after each arm() call so the runner can diff against its
    // dispatch timestamp.
    const onInput = (event: Event) => {
      if (!armed) return;
      armed = false;
      probe.inputLatencies.push(performance.timeOrigin + event.timeStamp);
    };
    window.addEventListener("wheel", onInput, { passive: true, capture: true });
    window.addEventListener("scroll", onInput, { passive: true, capture: true });
    return () => {
      window.removeEventListener("wheel", onInput, { capture: true });
      window.removeEventListener("scroll", onInput, { capture: true });
    };
  }, [switchTo]);

  return (
    <UIProvider>
      <div
        className="ui-app-frame"
        style={{ ["--workbench-current-width" as string]: "440px", minHeight: "100vh" }}
      >
        <nav
          aria-label="Perf fixture sessions"
          style={{
            width: "292px",
            borderRight: "1px solid var(--border-subtle, #333)",
            padding: "16px",
            display: "grid",
            gap: "8px",
            alignContent: "start",
          }}
        >
          <button data-testid="switch-session-a" onClick={() => switchTo("a")}>Session A (fixture)</button>
          <button data-testid="switch-session-b" onClick={() => switchTo("b")}>Session B (fixture)</button>
        </nav>
        <main className="ui-main-pane">
          <ContentFrame className="hub-session-frame">
            <ScrollRegion className="hub-session-scroll" data-chat-scroll-region="">
              <ChatTranscript key={active} events={events} streaming={streaming}
                renderExecution={(entry) => <HubTimeline
                activeSessionId={active}
                answeredPermissions={{}}
                attemptIsLive={streaming}
                events={[entry]}
                onAnswerPermission={() => undefined}
                onStop={() => undefined}
                onSwitchBack={() => undefined}
              />} />
            </ScrollRegion>
          </ContentFrame>
        </main>
      </div>
    </UIProvider>
  );
}
