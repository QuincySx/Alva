// INPUT:  Durable Hub session detail, engine diagnostics, and memory save action
// OUTPUT: Expandable session memory, branches, attempts, handoffs, and diagnostics
// POS:    Optional debug/details surface beside the primary conversation flow

import { useState } from "react";
import type { EngineReadiness, HubSessionDetail } from "../bridge";
import { Button, Disclosure, TextField } from "../ui";
import { localizeSystemText, useI18n } from "../i18n";

export interface SessionDetailsProps {
  detail: HubSessionDetail;
  memorySaving: boolean;
  onAddMemory: (entry: string) => void;
  readiness: EngineReadiness[];
  readinessLabel: Record<EngineReadiness["status"], string>;
  describeReason: (reason: EngineReadiness["reasons"][number]) => string;
}

export function SessionDetails({
  detail,
  memorySaving,
  onAddMemory,
  readiness,
  readinessLabel,
  describeReason,
}: SessionDetailsProps) {
  const { locale, localize, t } = useI18n();
  const [memoryDraft, setMemoryDraft] = useState("");
  const [openHandoffs, setOpenHandoffs] = useState<Record<string, boolean>>({});

  function saveMemory() {
    const entry = memoryDraft.trim();
    if (!entry || memorySaving) return;
    onAddMemory(entry);
    setMemoryDraft("");
  }

  return (
    <aside className="details-drawer" aria-label={t("details.title")}>
      <section>
        <span className="eyebrow">{t("details.memory")}</span>
        <small className="timeline-hint">
          {t("details.memoryDescription")}
        </small>
        {detail.memory.length === 0 ? (
          <small className="session-empty">
            {t("details.noMemory")}
          </small>
        ) : (
          detail.memory.map((entry, index) => (
            <div className="details-row" key={`memory-${index}`}>
              <small>{entry}</small>
            </div>
          ))
        )}
        <div className="memory-add">
          <TextField
            aria-label={t("details.addMemory")}
            placeholder={t("details.memoryPlaceholder")}
            value={memoryDraft}
            onChange={(event) => setMemoryDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") saveMemory();
            }}
          />
          <Button
            disabled={!memoryDraft.trim()}
            loading={memorySaving}
            onClick={saveMemory}
          >
            {memorySaving ? t("common.saving") : t("details.remember")}
          </Button>
        </div>
      </section>
      <Disclosure
        className="details-section"
        description={t("details.engineHistoryDescription")}
        meta={detail.branches.length}
        title={t("details.engineHistory")}
      >
        {detail.branches.length === 0 ? (
          <small className="session-empty">{t("details.noBranches")}</small>
        ) : null}
        {detail.branches.map((branch) => (
          <div className="details-row" key={branch.branch_id}>
            <strong>{branch.engine_id} · {localize(branch.status)}</strong>
            <small>
              {branch.model_id} · {branch.provider_id}
              {branch.engine_session_id ? ` · ${t("details.nativeKept")}` : ""}
            </small>
          </div>
        ))}
      </Disclosure>
      <Disclosure
        className="details-section"
        description={t("details.runDetailsDescription")}
        meta={detail.turns.reduce((count, turn) => count + turn.attempts.length, 0)}
        title={t("details.runDetails")}
      >
        {detail.turns.every((turn) => turn.attempts.length === 0) ? (
          <small className="session-empty">{t("details.noAttempts")}</small>
        ) : null}
        {detail.turns.flatMap((turn) =>
          turn.attempts.map((attempt) => (
            <div className="details-row" key={attempt.attempt_id}>
              <strong>{attempt.engine_id} · {localize(attempt.status)}</strong>
              {attempt.failure_reason ? <small>{localizeSystemText(locale, attempt.failure_reason, t("system.operationFailed"))}</small> : null}
              {attempt.usage ? (
                <small>
                  {t("details.usage", { input: attempt.usage.input_tokens, output: attempt.usage.output_tokens })} ·{" "}
                  {attempt.usage.duration_ms}ms
                </small>
              ) : null}
              {attempt.side_effects.length > 0 ? (
                <small>
                  {t("details.toolCalls", { count: attempt.side_effects.length })}
                  {attempt.side_effects.map((evidence) => evidence.path).filter(Boolean).length > 0
                    ? ` · ${attempt.side_effects.map((evidence) => evidence.path).filter(Boolean).join(", ")}`
                    : ""}
                </small>
              ) : null}
            </div>
          )),
        )}
      </Disclosure>
      <Disclosure
        className="details-section"
        description={t("details.handoffsDescription")}
        meta={detail.handoffs.length}
        title={t("details.handoffs")}
      >
        {detail.handoffs.length === 0 ? (
          <small className="session-empty">{t("details.noHandoffs")}</small>
        ) : (
          detail.handoffs.map((handoff) => (
            <div className="details-row" key={handoff.handoff_id}>
              <strong>
                {localize(handoff.kind)} · {handoff.source.engine_id} → {handoff.target.engine_id}
                {handoff.used_at ? ` · ${t("timeline.delivered")}` : ` · ${t("timeline.pending")}`}
              </strong>
              <small>{localizeSystemText(locale, handoff.reason, t("system.diagnosticUnavailable"))}</small>
              {handoff.team_snapshot ? <small>{handoff.team_snapshot}</small> : null}
              {handoff.session_memory.length > 0 ||
              handoff.available_skills.length > 0 ||
              handoff.available_mcp.length > 0 ? (
                <small>
                  {t("details.envelope", { memory: handoff.session_memory.length, skills: handoff.available_skills.length, mcp: handoff.available_mcp.length })}
                </small>
              ) : null}
              <Button
                onClick={() =>
                  setOpenHandoffs((current) => ({
                    ...current,
                    [handoff.handoff_id]: !current[handoff.handoff_id],
                  }))
                }
                size="small"
                variant="ghost"
              >
                {openHandoffs[handoff.handoff_id] ? t("details.hideEnvelope") : t("details.showEnvelope")}
              </Button>
              {openHandoffs[handoff.handoff_id] ? (
                <div className="handoff-view">
                  {handoff.session_memory.length > 0 ? (
                    <>
                      <strong>{t("details.memoryCarried")}</strong>
                      {handoff.session_memory.map((entry, index) => (
                        <small key={`dm-${index}`}>{entry}</small>
                      ))}
                    </>
                  ) : null}
                  {handoff.available_skills.length > 0 ? (
                    <>
                      <strong>{t("details.skills")}</strong>
                      {handoff.available_skills.map((skill) => (
                        <small key={skill.path}>
                          {skill.name} — {skill.description} <code>{skill.path}</code>
                        </small>
                      ))}
                    </>
                  ) : null}
                  {handoff.available_mcp.length > 0 ? (
                    <>
                      <strong>{t("timeline.mcpServers")}</strong>
                      {handoff.available_mcp.map((server) => (
                        <small key={server.id}>
                          {server.display_name} <code>{server.transport}</code>
                        </small>
                      ))}
                    </>
                  ) : null}
                  {handoff.transcript.length > 0 ? (
                    <>
                      <strong>{t("timeline.filteredTranscript", { count: handoff.transcript.length })}</strong>
                      {handoff.transcript.map((turn, index) => (
                        <small key={`dt-${index}`}>
                          » {turn.prompt}{turn.result ? ` — ${turn.result}` : ""}
                        </small>
                      ))}
                    </>
                  ) : (
                    <small>{t("timeline.transcriptPrivate")}</small>
                  )}
                </div>
              ) : null}
            </div>
          ))
        )}
      </Disclosure>
      <Disclosure
        className="details-section"
        description={t("details.diagnosticsDescription")}
        meta={readiness.length}
        title={t("details.diagnostics")}
      >
        {readiness.map((engineReadiness) => (
          <div className="details-row" key={engineReadiness.engine_id}>
            <strong>
              {engineReadiness.display_name} · {localize(readinessLabel[engineReadiness.status])}
            </strong>
            <small>
              {engineReadiness.version ?? t("readiness.versionUnknown")} · {t("details.stream", { status: localize(engineReadiness.stream_health) })}
            </small>
            {engineReadiness.reasons.map((reason, index) => (
              <small key={`${reason.code}-${index}`}>{describeReason(reason)}</small>
            ))}
          </div>
        ))}
      </Disclosure>
    </aside>
  );
}
