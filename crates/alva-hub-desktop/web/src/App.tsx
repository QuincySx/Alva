// INPUT:  React hooks, bridge workspace/run/timeline/session functions
// OUTPUT: engine selection, durable branches, interrupted recovery, composer, timeline UI
// POS:    Primary Hub Desktop product surface for reversible Alva/Claude/Codex sessions.

import { FormEvent, useEffect, useMemo, useState } from "react";
import {
  chooseWorkspace,
  getHubSession,
  listHubSessions,
  listenTimeline,
  runHubPrompt,
  switchHubEngine,
  type HubSessionDetail,
  type HubSessionSummary,
  type TimelineEvent,
} from "./bridge";

function projectTimeline(detail: HubSessionDetail): TimelineEvent[] {
  const turns = detail.turns.map((turn) => ({
    status:
      turn.status === "failed"
        ? ("failed" as const)
        : turn.status === "running"
          ? ("running" as const)
          : turn.status === "interrupted"
            ? ("interrupted" as const)
            : ("completed" as const),
    engine_id: turn.attempts.at(-1)?.engine_id ?? detail.session.engine_id,
    product_session_id: detail.session.product_session_id,
    message: [turn.prompt, turn.result ?? turn.error].filter(Boolean).join("\n\n"),
    created_at: turn.created_at,
  }));
  const switches = detail.timeline_events.map((event) => ({
    status:
      event.kind === "engine_switch_queued"
        ? ("queued" as const)
        : event.kind === "engine_switch_cancelled"
          ? ("cancelled" as const)
          : ("switched" as const),
    engine_id: event.target_engine_id,
    product_session_id: detail.session.product_session_id,
    message: `${event.source_engine_id} → ${event.target_engine_id}\n\n${event.reason}`,
    created_at: event.created_at,
  }));
  return [...turns, ...switches].sort(
    (left, right) => (left.created_at ?? 0) - (right.created_at ?? 0),
  );
}

export default function App() {
  const [workspace, setWorkspace] = useState("");
  const [prompt, setPrompt] = useState("");
  const [timeline, setTimeline] = useState<TimelineEvent[]>([]);
  const [sessions, setSessions] = useState<HubSessionSummary[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [engine, setEngine] = useState("alva");
  const [pendingEngine, setPendingEngine] = useState<string | null>(null);
  const [sessionError, setSessionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void listenTimeline((event) => {
      if (active) {
        setTimeline((current) => [...current, event]);
      }
    }).then((stop) => {
      if (active) {
        unsubscribe = stop;
      } else {
        stop();
      }
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    void refreshSessions();
  }, []);

  const latest = timeline.at(-1);
  const statusLabel = useMemo(() => {
    switch (latest?.status) {
      case "running":
        return `${latest.engine_id} is working`;
      case "completed":
        return "Completed";
      case "failed":
        return "Failed";
      case "interrupted":
        return "Interrupted · ready to continue";
      case "queued":
        return "Engine switch queued";
      case "switched":
        return "Engine switched";
      case "cancelled":
        return "Engine switch cancelled";
      default:
        return "Ready";
    }
  }, [latest]);

  async function selectWorkspace() {
    try {
      const selected = await chooseWorkspace();
      if (selected) {
        setWorkspace(selected);
        setActiveSessionId(null);
        setEngine("alva");
        setPendingEngine(null);
        setTimeline([]);
      }
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function refreshSessions() {
    try {
      setSessions(await listHubSessions());
      setSessionError(null);
    } catch (error) {
      setSessionError(error instanceof Error ? error.message : String(error));
    }
  }

  async function openSession(session: HubSessionSummary) {
    if (busy) return;
    try {
      await loadSession(session.product_session_id);
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  async function loadSession(productSessionId: string) {
    const detail = await getHubSession(productSessionId);
    if (!detail) throw new Error("This Hub session no longer exists.");
    setActiveSessionId(detail.session.product_session_id);
    setWorkspace(detail.session.workspace);
    setEngine(detail.session.engine_id);
    setPendingEngine(detail.pending_switch?.target.engine_id ?? null);
    setTimeline(projectTimeline(detail));
  }

  async function selectEngine(engineId: string) {
    if (!activeSessionId) {
      setEngine(engineId);
      setPendingEngine(null);
      return;
    }
    try {
      const outcome = await switchHubEngine(
        activeSessionId,
        engineId,
        `User explicitly selected ${engineId}`,
      );
      setEngine(outcome.active_engine_id);
      setPendingEngine(outcome.disposition === "queued" ? engineId : null);
      await loadSession(activeSessionId);
      await refreshSessions();
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  function appendLocalFailure(error: unknown) {
    setTimeline((current) => [
      ...current,
      {
        status: "failed",
        engine_id: engine,
        product_session_id: "setup",
        message: error instanceof Error ? error.message : String(error),
      },
    ]);
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = prompt.trim();
    if (!workspace) {
      appendLocalFailure(new Error("Choose a workspace before running an engine."));
      return;
    }
    if (!text || busy) return;

    setBusy(true);
    setPrompt("");
    try {
      const outcome = await runHubPrompt({
        prompt: text,
        workspace,
        engine_id: engine,
        product_session_id: activeSessionId,
      });
      setActiveSessionId(outcome.product_session_id);
      await refreshSessions();
      await loadSession(outcome.product_session_id);
    } catch (error) {
      appendLocalFailure(error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div>
          <div className="brand-mark">A</div>
          <h1>ALVA Hub</h1>
          <p>One project. Every engine.</p>
        </div>

        <section className="workspace-panel">
          <span className="eyebrow">Workspace</span>
          <strong>{workspace ? workspace.split("/").at(-1) : "Not selected"}</strong>
          <small>{workspace || "Choose a local project to begin."}</small>
          <button className="secondary-button" type="button" onClick={selectWorkspace}>
            Choose workspace
          </button>
        </section>

        <section className="session-panel">
          <div className="session-heading">
            <span className="eyebrow">Sessions</span>
            <small>{sessions.length}</small>
          </div>
          <div className="session-list">
            {sessionError ? (
              <small className="session-error">{sessionError}</small>
            ) : sessions.length === 0 ? (
              <small className="session-empty">Hub runs will appear here.</small>
            ) : (
              sessions.map((session) => (
                <button
                  className={`session-item ${
                    activeSessionId === session.product_session_id ? "active" : ""
                  }`}
                  key={session.product_session_id}
                  type="button"
                  onClick={() => void openSession(session)}
                >
                  <strong>{session.workspace.split("/").at(-1)}</strong>
                  <span>{session.last_prompt ?? "Untitled session"}</span>
                  <small>{session.engine_id} · {session.status}</small>
                </button>
              ))
            )}
          </div>
        </section>

        <div className="ticket-note">
          <span>Durable engine branches</span>
          <small>Alva · Claude · Codex · reversible handoff</small>
        </div>
      </aside>

      <main className="main-column">
        <header className="topbar">
          <div>
            <span className="eyebrow">Engine</span>
            <select
              aria-label="Engine"
              value={pendingEngine ?? engine}
              onChange={(event) => void selectEngine(event.target.value)}
            >
              <option value="alva">Alva</option>
              <option value="claude">Claude</option>
              <option value="codex">Codex</option>
            </select>
            {pendingEngine ? <small>Queued after the current run</small> : null}
          </div>
          <div className={`status-pill status-${latest?.status ?? "ready"}`}>
            <span />
            {statusLabel}
          </div>
        </header>

        <section className="timeline" aria-live="polite">
          {timeline.length === 0 ? (
            <div className="empty-state">
              <span className="empty-icon">↗</span>
              <h2>Start one continuous project session</h2>
              <p>
                Hub keeps each engine branch in private local storage, so you can
                switch engines and return without losing its native session.
              </p>
            </div>
          ) : (
            timeline.map((entry, index) => (
              <article
                className={`timeline-card timeline-${entry.status}`}
                key={`${entry.product_session_id}-${index}`}
              >
                <div className="timeline-meta">
                  <span>{entry.engine_id}</span>
                  <span>{entry.status}</span>
                </div>
                <p>
                  {entry.message ??
                    (entry.status === "running"
                      ? "Executing in the selected workspace…"
                      : "Run finished.")}
                </p>
                {entry.status === "interrupted" ? (
                  <small className="timeline-hint">
                    The previous runtime is gone. Continue safely from the composer below.
                  </small>
                ) : null}
              </article>
            ))
          )}
        </section>

        <form className="composer" onSubmit={submit}>
          <textarea
            aria-label="Message"
            placeholder={`Ask ${pendingEngine ?? engine} to inspect, change, or explain this project…`}
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <div className="composer-footer">
            <span>{workspace ? "Workspace ready" : "Select a workspace first"}</span>
            <button
              className="primary-button"
              type="submit"
              disabled={busy || !prompt.trim() || !workspace}
            >
              {busy ? "Running…" : `Run with ${engine}`}
            </button>
          </div>
        </form>
      </main>
    </div>
  );
}
