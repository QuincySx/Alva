// INPUT:  React hooks, bridge workspace/run/timeline functions
// OUTPUT: Hub Desktop ticket-01 workspace, composer, and timeline UI
// POS:    Primary walking-skeleton product surface for a single Alva turn.

import { FormEvent, useEffect, useMemo, useState } from "react";
import {
  chooseWorkspace,
  listenTimeline,
  runAlvaPrompt,
  type TimelineEvent,
} from "./bridge";

export default function App() {
  const [workspace, setWorkspace] = useState("");
  const [prompt, setPrompt] = useState("");
  const [timeline, setTimeline] = useState<TimelineEvent[]>([]);
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

  const latest = timeline.at(-1);
  const statusLabel = useMemo(() => {
    switch (latest?.status) {
      case "running":
        return "Alva is working";
      case "completed":
        return "Completed";
      case "failed":
        return "Failed";
      default:
        return "Ready";
    }
  }, [latest]);

  async function selectWorkspace() {
    try {
      const selected = await chooseWorkspace();
      if (selected) setWorkspace(selected);
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
      await runAlvaPrompt({ prompt: text, workspace });
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

        <div className="ticket-note">
          <span>Walking skeleton</span>
          <small>Single Alva EngineTarget · no persistence yet</small>
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
                Ticket 01 connects the real Hub runtime to a focused Desktop
                timeline. Persistence and multi-engine branches arrive next.
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
