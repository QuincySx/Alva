// INPUT:  Projected Hub events (execution/control), permission decisions, fallback actions
// OUTPUT: ExecutionDiagnotics — running-state execution timeline (no chat
//         messages, no three-lane overview in the main chat)
// POS:    R30: the main chat is a reading flow (chat-transcript). This view
//         renders ONLY while an attempt is live or unresolved (approvals /
//         failures); completed execution detail belongs to the Inspector.

import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  ShieldAlert,
  Wrench,
} from "lucide-react";
import type { EngineTarget, HandoffPackage, TimelineEvent } from "../bridge";
import { Button } from "../ui";
import { localizeSystemText, useI18n } from "../i18n";

export type PermissionAnswer = "allowed" | "denied" | "expired";

export type HubTimelineEvent = TimelineEvent & {
  role?: "user" | "assistant";
  /** First observation time retained while live deltas merge into one item. */
  started_at?: number;
  fallback?: {
    source_engine_id: string;
    handoff: HandoffPackage | null;
  };
};

function compactDuration(durationMs: number): string {
  if (durationMs < 1_000) return `${Math.max(0, Math.round(durationMs))}ms`;
  return `${(durationMs / 1_000).toFixed(durationMs < 10_000 ? 1 : 0)}s`;
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

/** Execution/control events only — chat messages render in ChatTranscript. */
function isExecutionEntry(entry: HubTimelineEvent): boolean {
  if (entry.role === "user") return false;
  // Assistant prose lives in the transcript; assistant-coded FAILURES /
  // interrupts stay here so their actions and reasons remain reachable.
  if (entry.role === "assistant") {
    return entry.status !== "assistant_message";
  }
  return true;
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

  const execution = events.filter(isExecutionEntry);
  if (execution.length === 0) return null;

  return (
    <section aria-label={t("chat.executionInInspector")} aria-live="polite" className="timeline">
      {execution.map((entry, index) => {
          const actKey = `${entry.product_session_id}-${index}-act`;
          const isActOpen = Boolean(openActivities[actKey]);

          return (
            <article
              className={`timeline-card timeline-${entry.status}`}
              key={`${entry.product_session_id}-${index}`}
            >
              {/* Execution Header */}
              <div className="timeline-meta">
                <strong>{entry.engine_id || "ALVA"}</strong>
                <span>{localize(entry.status)}</span>
              </div>

              {/* Reason / progress line — assistant prose renders in the
                  transcript; here we only surface non-prose captions. */}
              {entry.role === "assistant" ? null : (
                <p>
                  {entry.message
                    ? localizeSystemText(
                        locale,
                        entry.message,
                        t("system.diagnosticUnavailable"),
                      )
                    : entry.status === "running"
                      ? t("timeline.executing")
                      : t("timeline.finished")}
                </p>
              )}

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
                        {compactDuration(entry.activity.duration_ms)}
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
                  <small className="timeline-hint">
                    {t("timeline.tool", { tool: entry.permission_request.tool_name })}
                  </small>
                  {entry.permission_request.resource ? (
                    <small className="timeline-hint">
                      {t("timeline.resource", {
                        resource: entry.permission_request.resource,
                      })}
                    </small>
                  ) : null}
                  {/* 工具自述的效果说明是运行期事实，任何 locale 都原样保留，
                      不能套用产品诊断文案的未知英语隐藏规则。 */}
                  {entry.permission_request.description ? (
                    <small>{entry.permission_request.description}</small>
                  ) : null}
                  {answeredPermissions[entry.permission_request.request_id] ? (
                    <small className="permission-answered">
                      {answeredPermissions[entry.permission_request.request_id] === "allowed"
                        ? t("timeline.allowed")
                        : answeredPermissions[entry.permission_request.request_id] === "denied"
                          ? t("timeline.denied")
                          : t("timeline.expired")}
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
    </section>
  );
}
