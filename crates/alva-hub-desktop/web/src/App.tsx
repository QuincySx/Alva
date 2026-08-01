// INPUT:  React hooks, bridge workspace/run/timeline/session functions
// OUTPUT: persistent Hub sessions, interrupted recovery, composer, and timeline UI
// POS:    Primary Hub Desktop product surface for resumable Alva sessions.

import { FormEvent, useEffect, useMemo, useState } from "react";
import {
  chooseWorkspace,
  getHubSession,
  listHubSessions,
  listenTimeline,
  runAlvaPrompt,
  type HubSessionSummary,
  type TimelineEvent,
} from "./bridge";

export default function App() {
  const [workspace, setWorkspace] = useState("");
  const [prompt, setPrompt] = useState("");
  const [timeline, setTimeline] = useState<TimelineEvent[]>([]);
  const [sessions, setSessions] = useState<HubSessionSummary[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
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
        return "Alva is working";
      case "completed":
        return "Completed";
      case "failed":
        return "Failed";
      case "interrupted":
        return "Interrupted · ready to continue";
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
      const detail = await getHubSession(session.product_session_id);
      if (!detail) throw new Error("This Hub session no longer exists.");
      setActiveSessionId(detail.session.product_session_id);
      setWorkspace(detail.session.workspace);
      setTimeline(
        detail.turns.map((turn) => ({
          status:
            turn.status === "failed"
              ? "failed"
              : turn.status === "running"
                ? "running"
                : turn.status === "interrupted"
                  ? "interrupted"
                : "completed",
          engine_id: detail.session.engine_id,
          product_session_id: detail.session.product_session_id,
          message: [turn.prompt, turn.result ?? turn.error]
            .filter(Boolean)
            .join("\n\n"),
        })),
      );
    } catch (error) {
      appendLocalFailure(error);
    }
  }

  function appendLocalFailure(error: unknown) {
    setTimeline((current) => [
      ...current,
      {
        status: "failed",
        engine_id: "alva",
        product_session_id: "setup",
        message: error instanceof Error ? error.message : String(error),
      },
    ]);
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = prompt.trim();
    if (!workspace) {
      appendLocalFailure(new Error("Choose a workspace before running Alva."));
      return;
    }
    if (!text || busy) return;

    setBusy(true);
    setPrompt("");
    try {
      const outcome = await runAlvaPrompt({
        prompt: text,
        workspace,
        product_session_id: activeSessionId,
      });
      setActiveSessionId(outcome.product_session_id);
      await refreshSessions();
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
                  <small>{session.status}</small>
                </button>
              ))
            )}
          </div>
        </section>

        <div className="ticket-note">
          <span>Durable attempts</span>
          <small>Single Alva EngineTarget · crash recovery enabled</small>
        </div>
      </aside>

      <main className="main-column">
        <header className="topbar">
          <div>
            <span className="eyebrow">Engine</span>
            <select aria-label="Engine" defaultValue="alva">
              <option value="alva">Alva</option>
            </select>
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
                Hub keeps completed Alva turns in private local storage, so you
                can reopen a project session and continue after restarting.
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
            placeholder="Ask Alva to inspect, change, or explain this project…"
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
              {busy ? "Running…" : "Run with Alva"}
            </button>
          </div>
        </form>
      </main>
    </div>
  );
}
