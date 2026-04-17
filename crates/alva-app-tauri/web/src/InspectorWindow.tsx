import { useEffect, useState } from "react";
import { Inspector } from "./components/Inspector";
import {
  subscribeAgentEvents,
  type AgentEventEnvelope,
} from "./agent-bridge";

const HANDOFF_KEY = "alva.inspector.session_id";

/**
 * Standalone Inspector window. Opened by the main window via the Rust
 * `open_inspector_window` command. The main window stashes the session id
 * in `localStorage[HANDOFF_KEY]` just before opening — Tauri's default
 * WebContext shares storage across windows, so we read the same value
 * here on mount. The window then subscribes to `agent_event` directly and
 * bumps its own refresh nonce on `AgentEnd`, giving live updates as the
 * main window runs the agent.
 */
export default function InspectorWindow() {
  const [sessionId, setSessionId] = useState<string | null>(() => {
    try {
      return localStorage.getItem(HANDOFF_KEY);
    } catch {
      return null;
    }
  });
  const [nonce, setNonce] = useState(0);

  // React to main window bumping the handoff key while we're still open.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === HANDOFF_KEY && e.newValue) {
        setSessionId(e.newValue);
        setNonce((n) => n + 1);
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  // Subscribe to the Rust `agent_event` stream. Each window gets its own
  // copy — Tauri broadcasts to all webviews by default — so we can refresh
  // the projection whenever the targeted session finishes a turn.
  useEffect(() => {
    const p = subscribeAgentEvents((envelope: AgentEventEnvelope) => {
      if (!sessionId || envelope.session_id !== sessionId) return;
      if (
        envelope.event.type === "AgentEnd" ||
        envelope.event.type === "RunChannelClosed"
      ) {
        setNonce((n) => n + 1);
      }
    });
    return () => {
      p.then((fn) => fn());
    };
  }, [sessionId]);

  return (
    <div className="h-screen w-screen bg-neutral-950 text-neutral-100 flex flex-col">
      <header className="shrink-0 flex items-center gap-3 px-4 py-2 border-b border-neutral-800 text-xs">
        <span className="font-semibold">Alva Inspector</span>
        {sessionId ? (
          <span className="text-neutral-500 font-mono truncate">
            session: {sessionId}
          </span>
        ) : (
          <span className="text-neutral-500">没有会话</span>
        )}
      </header>
      <div className="flex-1 min-h-0">
        <Inspector sessionId={sessionId} refreshNonce={nonce} />
      </div>
    </div>
  );
}
