// INPUT:  Projected Hub events, permission decisions, and fallback actions
// OUTPUT: Three-lane execution overview plus continuous conversation axis, expandable tools, and handoff envelopes
// POS:    Central business view between the topbar and composer

import { useMemo, useState } from "react";
import {
  Bot,
  ChevronDown,
  ChevronRight,
  ShieldAlert,
  User,
  Wrench,
} from "lucide-react";
import type { EngineTarget, HandoffPackage, TimelineEvent } from "../bridge";
import { Button } from "../ui";
import { localizeSystemText, useI18n } from "../i18n";

export type PermissionAnswer = "allowed" | "denied";

export type HubTimelineEvent = TimelineEvent & {
  role?: "user" | "assistant";
  /** First observation time retained while live deltas merge into one item. */
  started_at?: number;
  fallback?: {
    source_engine_id: string;
    handoff: HandoffPackage | null;
  };
};

type TimelineLane = "input" | "agent" | "tools";

function laneFor(entry: HubTimelineEvent): TimelineLane {
  if (entry.activity) return "tools";
  if (entry.status === "user_message" || entry.status === "permission_required") {
    return "input";
  }
  return "agent";
}

function compactDuration(durationMs: number): string {
  if (durationMs < 1_000) return `${Math.max(0, Math.round(durationMs))}ms`;
  return `${(durationMs / 1_000).toFixed(durationMs < 10_000 ? 1 : 0)}s`;
}

function TimelineOverview({ events }: { events: HubTimelineEvent[] }) {
  const { locale } = useI18n();
  const projection = useMemo(() => {
    const timed = events.flatMap((entry, index) => {
      if (typeof entry.created_at !== "number" || !Number.isFinite(entry.created_at)) {
        return [];
      }
      const duration = entry.activity?.duration_ms ?? null;
      const observedStart = entry.started_at ?? entry.created_at;
      const start = duration !== null
        ? Math.min(observedStart, entry.created_at - Math.max(0, duration))
        : observedStart;
      const end = duration !== null
        ? Math.max(entry.created_at, start + duration)
        : Math.max(entry.created_at, start);
      return [{ entry, index, lane: laneFor(entry), start, end }];
    });
    if (timed.length < 2) return null;
    const origin = Math.min(...timed.map((item) => item.start));
    const finish = Math.max(...timed.map((item) => item.end));
    const span = Math.max(1, finish - origin);
    return { timed, origin, span };
  }, [events]);

  if (!projection) return null;
  const labels: Record<TimelineLane, string> = locale === "zh-CN"
    ? { input: "输入", agent: "Agent", tools: "工具" }
    : { input: "Input", agent: "Agent", tools: "Tools" };
  const lanes: TimelineLane[] = ["input", "agent", "tools"];

  return (
    <div
      aria-label={locale === "zh-CN" ? "Agent 执行时间线概览" : "Agent execution timeline overview"}
      className="timeline-overview"
      role="img"
    >
      <div className="timeline-overview__header">
        <strong>{locale === "zh-CN" ? "执行时间线" : "Execution timeline"}</strong>
        <span>{compactDuration(projection.span)}</span>
      </div>
      <div className="timeline-overview__axis" aria-hidden="true">
        <span>0</span>
        <span>{compactDuration(projection.span / 2)}</span>
        <span>{compactDuration(projection.span)}</span>
      </div>
      {lanes.map((lane) => (
        <div className="timeline-overview__lane" key={lane}>
          <span>{labels[lane]}</span>
          <div className="timeline-overview__track">
            {projection.timed
              .filter((item) => item.lane === lane)
              .map((item) => {
                const left = ((item.start - projection.origin) / projection.span) * 100;
                const width = Math.max(
                  ((item.end - item.start) / projection.span) * 100,
                  0.65,
                );
                const clampedLeft = Math.min(Math.max(left, 0), 99.35);
                return (
                  <i
                    className={`timeline-overview__mark timeline-overview__mark--${lane}`}
                    key={`${item.entry.item_id ?? item.entry.status}-${item.index}`}
                    style={{
                      left: `${clampedLeft}%`,
                      width: `${Math.min(width, 100 - clampedLeft)}%`,
                    }}
                    title={`${item.entry.engine_id} · ${item.entry.message ?? item.entry.status} · +${compactDuration(item.start - projection.origin)}`}
                  />
                );
              })}
          </div>
        </div>
      ))}
    </div>
  );
}

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
  const [openActivities, setOpenActivities] = useState<Record<string, boolean>>({});

  const toggleHandoff = (index: number) => {
    setOpenHandoffs((current) => ({
      ...current,
      [index]: !current[index],
    }));
  };

  const toggleActivity = (key: string) => {
    setOpenActivities((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  return (
    <section className="timeline" aria-live="polite">
      {events.length === 0 ? (
        <div className="empty-state">
          <span className="empty-icon">↗</span>
          <h2>{t("timeline.empty")}</h2>
          <p>{t("timeline.emptyDescription")}</p>
        </div>
      ) : (
        <>
          <TimelineOverview events={events} />
          {events.map((entry, index) => {
          const isUser = entry.role === "user";
          const actKey = `${entry.product_session_id}-${index}-act`;
          const isActOpen = Boolean(openActivities[actKey]);

          return (
            <article
              className={`timeline-card timeline-${entry.status}${
                entry.role ? ` timeline-role-${entry.role}` : ""
              }`}
              key={`${entry.product_session_id}-${index}`}
            >
              {/* Message Header */}
              <div className="timeline-meta">
                <span style={{ display: "inline-flex", alignItems: "center", gap: "6px" }}>
                  {isUser ? (
                    <User size={14} aria-hidden="true" />
                  ) : (
                    <Bot size={14} aria-hidden="true" />
                  )}
                  {isUser ? t("timeline.you") : entry.engine_id || "ALVA Agent"}
                </span>
                <span>{localize(entry.status)}</span>
              </div>

              {/* Message Body */}
              <p>
                {entry.message ? (
                  entry.role ? (
                    localize(entry.message)
                  ) : (
                    localizeSystemText(
                      locale,
                      entry.message,
                      t("system.diagnosticUnavailable"),
                    )
                  )
                ) : entry.status === "running" ? (
                  t("timeline.executing")
                ) : (
                  t("timeline.finished")
                )}
              </p>

              {/* Expandable Compact Activity Row (Tool Call) */}
              {entry.activity ? (
                <div
                  className="timeline-activity-row"
                  onClick={() => toggleActivity(actKey)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      toggleActivity(actKey);
                    }
                  }}
                >
                  <div className="timeline-activity-row__header">
                    <span className="timeline-activity-row__title">
                      {isActOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                      <Wrench size={13} style={{ color: "var(--color-focus)" }} />
                      <strong>{entry.activity.tool_name}</strong>
                      {entry.activity.path ? (
                        <code className="timeline-activity-path">{entry.activity.path}</code>
                      ) : null}
                    </span>
                    {entry.activity.duration_ms !== null ? (
                      <small className="timeline-activity-duration">
                        {entry.activity.duration_ms}ms
                      </small>
                    ) : null}
                  </div>
                  {isActOpen ? (
                    <div className="timeline-activity-row__details">
                      <div>
                        <small>
                          {locale === "zh-CN" ? "工具执行详情：" : "Execution Details:"}
                        </small>
                      </div>
                      <pre className="timeline-activity-output">
                        {entry.activity.tool_name} {entry.activity.path ?? ""}
                      </pre>
                    </div>
                  ) : null}
                </div>
              ) : null}

              {entry.status === "interrupted" ? (
                <small className="timeline-hint">{t("timeline.interruptedHint")}</small>
              ) : null}

              {/* Permission Request Card */}
              {entry.permission_request ? (
                <div className="permission-card">
                  <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                    <ShieldAlert size={16} style={{ color: "var(--color-warning)" }} />
                    <strong>{locale === "zh-CN" ? "操作授权请求" : "Permission Request"}</strong>
                  </div>
                  {entry.permission_request.resource ? (
                    <small className="timeline-hint">
                      {t("timeline.resource", {
                        resource: entry.permission_request.resource,
                      })}
                    </small>
                  ) : null}
                  {entry.permission_request.description ? (
                    <small>
                      {localizeSystemText(
                        locale,
                        entry.permission_request.description,
                        t("system.diagnosticUnavailable"),
                      )}
                    </small>
                  ) : null}
                  {answeredPermissions[entry.permission_request.request_id] ? (
                    <small className="permission-answered">
                      {answeredPermissions[entry.permission_request.request_id] === "allowed"
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
                        variant="primary"
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

              {/* Fallback Banner & Controls */}
              {entry.fallback ? (
                <div className="timeline-controls" style={{ marginTop: "8px" }}>
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
                    {t("timeline.switchBack", {
                      engine: entry.fallback.source_engine_id,
                    })}
                  </Button>
                  {entry.fallback.handoff ? (
                    <Button
                      onClick={() => toggleHandoff(index)}
                      size="small"
                      variant="ghost"
                    >
                      {openHandoffs[index]
                        ? t("timeline.hideHandoff")
                        : t("timeline.viewHandoff")}
                    </Button>
                  ) : null}
                </div>
              ) : null}

              {/* Handoff View Details */}
              {entry.fallback?.handoff && openHandoffs[index] ? (
                <div className="handoff-view">
                  <strong>
                    {t("timeline.handoffKind", {
                      kind: localize(entry.fallback.handoff.kind),
                    })}{" "}
                    · {entry.fallback.handoff.source.engine_id} →{" "}
                    {entry.fallback.handoff.target.engine_id}
                    {entry.fallback.handoff.used_at
                      ? ` · ${t("timeline.delivered")}`
                      : ` · ${t("timeline.pending")}`}
                  </strong>
                  <small>
                    {t("timeline.reason", {
                      reason: localizeSystemText(
                        locale,
                        entry.fallback.handoff.reason,
                        t("system.diagnosticUnavailable"),
                      ),
                    })}
                  </small>
                  {entry.fallback.handoff.user_goal ? (
                    <small>
                      {t("timeline.goal", {
                        goal: entry.fallback.handoff.user_goal,
                      })}
                    </small>
                  ) : null}
                  {entry.fallback.handoff.latest_instruction ? (
                    <small>
                      {t("timeline.latestInstruction", {
                        instruction: entry.fallback.handoff.latest_instruction,
                      })}
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
                      {t("timeline.files", {
                        files: entry.fallback.handoff.changed_files.join(", "),
                      })}
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
                        {t("timeline.filteredTranscript", {
                          count: entry.fallback.handoff.transcript.length,
                        })}
                      </strong>
                      {entry.fallback.handoff.transcript.map((turn, turnIndex) => (
                        <small key={`transcript-${turnIndex}`}>
                          » {turn.prompt}
                          {turn.result ? ` — ${turn.result}` : ""}
                        </small>
                      ))}
                    </>
                  ) : (
                    <small>{t("timeline.transcriptPrivate")}</small>
                  )}
                </div>
              ) : null}
            </article>
          );
          })}
        </>
      )}
    </section>
  );
}
