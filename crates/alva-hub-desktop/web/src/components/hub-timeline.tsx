// INPUT:  Projected Hub events, permission decisions, and fallback actions
// OUTPUT: Continuous conversation reading axis with expandable handoff envelopes
// POS:    Central business view between the topbar and composer

import { useState } from "react";
import type { EngineTarget, HandoffPackage, TimelineEvent } from "../bridge";
import { Button } from "../ui";
import { localizeSystemText, useI18n } from "../i18n";

export type PermissionAnswer = "allowed" | "denied";

export type HubTimelineEvent = TimelineEvent & {
  role?: "user" | "assistant";
  fallback?: {
    source_engine_id: string;
    handoff: HandoffPackage | null;
  };
};

export interface HubTimelineProps {
  activeSessionId: string | null;
  answeredPermissions: Record<string, PermissionAnswer>;
  attemptIsLive: boolean;
  events: HubTimelineEvent[];
  onAnswerPermission: (
    productSessionId: string,
    requestId: string,
    allow: boolean,
  ) => void;
  onStop: () => void;
  onSwitchBack: (target: EngineTarget) => void;
}

export function HubTimeline({
  activeSessionId,
  answeredPermissions,
  attemptIsLive,
  events,
  onAnswerPermission,
  onStop,
  onSwitchBack,
}: HubTimelineProps) {
  const { locale, localize, t } = useI18n();
  const [openHandoffs, setOpenHandoffs] = useState<Record<number, boolean>>({});

  return (
    <section className="timeline" aria-live="polite">
      {events.length === 0 ? (
        <div className="empty-state">
          <span className="empty-icon">↗</span>
          <h2>{t("timeline.empty")}</h2>
          <p>{t("timeline.emptyDescription")}</p>
        </div>
      ) : (
        events.map((entry, index) => (
          <article
            className={`timeline-card timeline-${entry.status}${entry.role ? ` timeline-role-${entry.role}` : ""}`}
            key={`${entry.product_session_id}-${index}`}
          >
            <div className="timeline-meta">
              <span>{entry.role === "user" ? t("timeline.you") : entry.engine_id}</span>
              <span>{localize(entry.status)}</span>
            </div>
            <p>
              {entry.message ? (entry.role ? localize(entry.message) : localizeSystemText(locale, entry.message, t("system.diagnosticUnavailable"))) :
                (entry.status === "running"
                  ? t("timeline.executing")
                  : t("timeline.finished"))}
            </p>
            {entry.activity ? (
              <small className="timeline-hint">
                {entry.activity.tool_name}
                {entry.activity.path ? ` · ${entry.activity.path}` : ""}
                {entry.activity.duration_ms !== null ? ` · ${entry.activity.duration_ms}ms` : ""}
              </small>
            ) : null}
            {entry.status === "interrupted" ? (
              <small className="timeline-hint">
                {t("timeline.interruptedHint")}
              </small>
            ) : null}
            {entry.permission_request?.resource ? (
              <small className="timeline-hint">
                {t("timeline.resource", { resource: entry.permission_request.resource })}
              </small>
            ) : null}
            {entry.permission_request ? (
              <div className="permission-card">
                {entry.permission_request.description ? (
                  <small>{localizeSystemText(locale, entry.permission_request.description, t("system.diagnosticUnavailable"))}</small>
                ) : null}
                {answeredPermissions[entry.permission_request.request_id] ? (
                  <small className="permission-answered">
                    {answeredPermissions[entry.permission_request.request_id] ===
                    "allowed"
                      ? t("timeline.allowed")
                      : t("timeline.denied")}
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
                      {t("timeline.allow")}
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
                      {t("timeline.deny")}
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
                  {t("timeline.stop")}
                </Button>
                <Button
                  disabled={!entry.fallback.handoff}
                  onClick={() => {
                    const source = entry.fallback?.handoff?.source;
                    if (source) onSwitchBack(source);
                  }}
                  size="small"
                  variant="ghost"
                >
                  {t("timeline.switchBack", { engine: entry.fallback.source_engine_id })}
                </Button>
                {!attemptIsLive ? (
                  <small className="timeline-hint">
                    {t("timeline.attemptFinished")}
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
                  {openHandoffs[index] ? t("timeline.hideHandoff") : t("timeline.viewHandoff")}
                </Button>
              </div>
            ) : null}
            {entry.fallback?.handoff && openHandoffs[index] ? (
              <div className="handoff-view">
                <strong>
                  {t("timeline.handoffKind", { kind: localize(entry.fallback.handoff.kind) })} ·{" "}
                  {entry.fallback.handoff.source.engine_id} →{" "}
                  {entry.fallback.handoff.target.engine_id}
                  {entry.fallback.handoff.used_at ? ` · ${t("timeline.delivered")}` : ` · ${t("timeline.pending")}`}
                </strong>
                <small>{t("timeline.reason", { reason: localizeSystemText(locale, entry.fallback.handoff.reason, t("system.diagnosticUnavailable")) })}</small>
                {entry.fallback.handoff.user_goal ? (
                  <small>{t("timeline.goal", { goal: entry.fallback.handoff.user_goal })}</small>
                ) : null}
                {entry.fallback.handoff.latest_instruction ? (
                  <small>
                    {t("timeline.latestInstruction", { instruction: entry.fallback.handoff.latest_instruction })}
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
                    {t("timeline.files", { files: entry.fallback.handoff.changed_files.join(", ") })}
                  </small>
                ) : null}
                {entry.fallback.handoff.workspace_summary ? (
                  <small>{entry.fallback.handoff.workspace_summary}</small>
                ) : null}
                {entry.fallback.handoff.available_skills.length > 0 ? (
                  <div className="handoff-skills">
                    <strong>{t("timeline.workspaceSkills")}</strong>
                    {entry.fallback.handoff.available_skills.map((skill) => (
                      <small key={skill.path}>
                        {skill.name} — {skill.description} <code>{skill.path}</code>
                      </small>
                    ))}
                  </div>
                ) : null}
                {entry.fallback.handoff.available_mcp.length > 0 ? (
                  <div className="handoff-skills">
                    <strong>{t("timeline.mcpServers")}</strong>
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
                      {t("timeline.filteredTranscript", { count: entry.fallback.handoff.transcript.length })}
                    </strong>
                    {entry.fallback.handoff.transcript.map((turn, turnIndex) => (
                      <small key={`transcript-${turnIndex}`}>
                        » {turn.prompt}{turn.result ? ` — ${turn.result}` : ""}
                      </small>
                    ))}
                  </>
                ) : (
                  <small>{t("timeline.transcriptPrivate")}</small>
                )}
              </div>
            ) : null}
          </article>
        ))
      )}
    </section>
  );
}
