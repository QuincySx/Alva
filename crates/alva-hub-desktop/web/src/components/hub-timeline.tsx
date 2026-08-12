// INPUT:  Projected Hub events, permission decisions, and fallback actions
// OUTPUT: Continuous conversation reading axis with expandable handoff envelopes
// POS:    Central business view between the topbar and composer

import { useState } from "react";
import type { HandoffPackage, TimelineEvent } from "../bridge";
import { Button } from "../ui";

export type PermissionAnswer = "allowed" | "denied";

export type HubTimelineEvent = TimelineEvent & {
  fallback?: {
    source_engine_id: string;
    handoff: HandoffPackage | null;
  };
};

export interface HubTimelineProps {
  activeSessionId: string | null;
  answeredPermissions: Record<string, PermissionAnswer>;
  attemptIsLive: boolean;
  engineId: string;
  events: HubTimelineEvent[];
  onAnswerPermission: (
    productSessionId: string,
    requestId: string,
    allow: boolean,
  ) => void;
  onStop: () => void;
  onSwitchBack: (engineId: string) => void;
}

export function HubTimeline({
  activeSessionId,
  answeredPermissions,
  attemptIsLive,
  engineId,
  events,
  onAnswerPermission,
  onStop,
  onSwitchBack,
}: HubTimelineProps) {
  const [openHandoffs, setOpenHandoffs] = useState<Record<number, boolean>>({});

  return (
    <section className="timeline" aria-live="polite">
      {events.length === 0 ? (
        <div className="empty-state">
          <span className="empty-icon">↗</span>
          <h2>Start one continuous project session</h2>
          <p>
            Hub keeps each engine branch in private local storage, so you can
            switch engines and return without losing its native session.
          </p>
        </div>
      ) : (
        events.map((entry, index) => (
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
            {entry.permission_request?.resource ? (
              <small className="timeline-hint">
                Resource: {entry.permission_request.resource}
              </small>
            ) : null}
            {entry.permission_request ? (
              <div className="permission-card">
                {entry.permission_request.description ? (
                  <small>{entry.permission_request.description}</small>
                ) : null}
                {answeredPermissions[entry.permission_request.request_id] ? (
                  <small className="permission-answered">
                    {answeredPermissions[entry.permission_request.request_id] ===
                    "allowed"
                      ? "You allowed this request."
                      : "You denied this request."}
                  </small>
                ) : (
                  <div className="timeline-controls">
                    <Button
                      onClick={() =>
                        onAnswerPermission(
                          entry.product_session_id,
                          entry.permission_request?.request_id ?? "",
                          true,
                        )
                      }
                      size="small"
                    >
                      Allow
                    </Button>
                    <Button
                      onClick={() =>
                        onAnswerPermission(
                          entry.product_session_id,
                          entry.permission_request?.request_id ?? "",
                          false,
                        )
                      }
                      size="small"
                      variant="ghost"
                    >
                      Deny
                    </Button>
                  </div>
                )}
              </div>
            ) : null}
            {entry.fallback ? (
              <div className="timeline-controls">
                <Button
                  disabled={!activeSessionId || !attemptIsLive}
                  onClick={onStop}
                  size="small"
                  variant="danger"
                >
                  Stop
                </Button>
                <Button
                  disabled={engineId === entry.fallback.source_engine_id}
                  onClick={() => onSwitchBack(entry.fallback?.source_engine_id ?? "")}
                  size="small"
                  variant="ghost"
                >
                  Switch back to {entry.fallback.source_engine_id}
                </Button>
                {!attemptIsLive ? (
                  <small className="timeline-hint">
                    This attempt has finished — Stop no longer applies.
                  </small>
                ) : null}
                <Button
                  disabled={!entry.fallback.handoff}
                  onClick={() =>
                    setOpenHandoffs((current) => ({
                      ...current,
                      [index]: !current[index],
                    }))
                  }
                  size="small"
                  variant="ghost"
                >
                  {openHandoffs[index] ? "Hide handoff" : "View handoff"}
                </Button>
              </div>
            ) : null}
            {entry.fallback?.handoff && openHandoffs[index] ? (
              <div className="handoff-view">
                <strong>
                  {entry.fallback.handoff.kind} handoff ·{" "}
                  {entry.fallback.handoff.source.engine_id} →{" "}
                  {entry.fallback.handoff.target.engine_id}
                  {entry.fallback.handoff.used_at ? " · delivered" : " · pending"}
                </strong>
                <small>Reason: {entry.fallback.handoff.reason}</small>
                {entry.fallback.handoff.user_goal ? (
                  <small>Goal: {entry.fallback.handoff.user_goal}</small>
                ) : null}
                {entry.fallback.handoff.latest_instruction ? (
                  <small>
                    Latest instruction: {entry.fallback.handoff.latest_instruction}
                  </small>
                ) : null}
                {entry.fallback.handoff.completed_work.map((work, workIndex) => (
                  <small key={`completed-${workIndex}`}>
                    ✓ {work.prompt} — {work.result}
                  </small>
                ))}
                {entry.fallback.handoff.pending_work.map((work, workIndex) => (
                  <small key={`pending-${workIndex}`}>
                    ⏳ {work.prompt} — {work.reason}
                  </small>
                ))}
                {entry.fallback.handoff.changed_files.length > 0 ? (
                  <small>
                    Files: {entry.fallback.handoff.changed_files.join(", ")}
                  </small>
                ) : null}
                {entry.fallback.handoff.workspace_summary ? (
                  <small>{entry.fallback.handoff.workspace_summary}</small>
                ) : null}
                {entry.fallback.handoff.available_skills.length > 0 ? (
                  <div className="handoff-skills">
                    <strong>Workspace skills</strong>
                    {entry.fallback.handoff.available_skills.map((skill) => (
                      <small key={skill.path}>
                        {skill.name} — {skill.description} <code>{skill.path}</code>
                      </small>
                    ))}
                  </div>
                ) : null}
                {entry.fallback.handoff.available_mcp.length > 0 ? (
                  <div className="handoff-skills">
                    <strong>MCP servers</strong>
                    {entry.fallback.handoff.available_mcp.map((server) => (
                      <small key={server.id}>
                        {server.display_name} <code>{server.transport}</code>
                      </small>
                    ))}
                  </div>
                ) : null}
                {entry.fallback.handoff.transcript.length > 0 ? (
                  <>
                    <strong>
                      Filtered transcript ({entry.fallback.handoff.transcript.length} turns)
                    </strong>
                    {entry.fallback.handoff.transcript.map((turn, turnIndex) => (
                      <small key={`transcript-${turnIndex}`}>
                        » {turn.prompt}{turn.result ? ` — ${turn.result}` : ""}
                      </small>
                    ))}
                  </>
                ) : (
                  <small>Transcript not shared (default).</small>
                )}
              </div>
            ) : null}
          </article>
        ))
      )}
    </section>
  );
}
