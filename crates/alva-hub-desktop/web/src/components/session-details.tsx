// INPUT:  Durable Hub session detail, engine diagnostics, and memory save action
// OUTPUT: Expandable session memory, branches, attempts, handoffs, and diagnostics
// POS:    Optional debug/details surface beside the primary conversation flow

import { useState } from "react";
import type { EngineReadiness, HubSessionDetail } from "../bridge";
import { Button, Disclosure, TextField } from "../ui";

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
  const [memoryDraft, setMemoryDraft] = useState("");
  const [openHandoffs, setOpenHandoffs] = useState<Record<string, boolean>>({});

  function saveMemory() {
    const entry = memoryDraft.trim();
    if (!entry || memorySaving) return;
    onAddMemory(entry);
    setMemoryDraft("");
  }

  return (
    <aside className="details-drawer" aria-label="Session details">
      <section>
        <span className="eyebrow">Memory</span>
        <small className="timeline-hint">
          Corrections every engine in this session carries across fallbacks and handoffs.
        </small>
        {detail.memory.length === 0 ? (
          <small className="session-empty">
            No memory yet. Teach the session once; every engine keeps it.
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
            aria-label="Add session memory"
            placeholder="Always run cargo test before finishing…"
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
            {memorySaving ? "Saving…" : "Remember"}
          </Button>
        </div>
      </section>
      <Disclosure
        className="details-section"
        description="Engine conversations kept for this session"
        meta={detail.branches.length}
        title="Engine history"
      >
        {detail.branches.length === 0 ? (
          <small className="session-empty">No branches yet.</small>
        ) : null}
        {detail.branches.map((branch) => (
          <div className="details-row" key={branch.branch_id}>
            <strong>{branch.engine_id} · {branch.status}</strong>
            <small>
              {branch.model_id} · {branch.provider_id}
              {branch.engine_session_id ? " · native session kept" : ""}
            </small>
          </div>
        ))}
      </Disclosure>
      <Disclosure
        className="details-section"
        description="Runs, failures, and tool activity"
        meta={detail.turns.reduce((count, turn) => count + turn.attempts.length, 0)}
        title="Run details"
      >
        {detail.turns.every((turn) => turn.attempts.length === 0) ? (
          <small className="session-empty">No attempts yet.</small>
        ) : null}
        {detail.turns.flatMap((turn) =>
          turn.attempts.map((attempt) => (
            <div className="details-row" key={attempt.attempt_id}>
              <strong>{attempt.engine_id} · {attempt.status}</strong>
              {attempt.failure_reason ? <small>{attempt.failure_reason}</small> : null}
              {attempt.usage ? (
                <small>
                  {attempt.usage.input_tokens}in/{attempt.usage.output_tokens}out ·{" "}
                  {attempt.usage.duration_ms}ms
                </small>
              ) : null}
              {attempt.side_effects.length > 0 ? (
                <small>
                  {attempt.side_effects.length} tool call(s)
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
        description="Context carried when changing engines"
        meta={detail.handoffs.length}
        title="Engine handoffs"
      >
        {detail.handoffs.length === 0 ? (
          <small className="session-empty">No handoffs yet.</small>
        ) : (
          detail.handoffs.map((handoff) => (
            <div className="details-row" key={handoff.handoff_id}>
              <strong>
                {handoff.kind} · {handoff.source.engine_id} → {handoff.target.engine_id}
                {handoff.used_at ? " · delivered" : " · pending"}
              </strong>
              <small>{handoff.reason}</small>
              {handoff.team_snapshot ? <small>{handoff.team_snapshot}</small> : null}
              {handoff.session_memory.length > 0 ||
              handoff.available_skills.length > 0 ||
              handoff.available_mcp.length > 0 ? (
                <small>
                  envelope: {handoff.session_memory.length} memory ·{" "}
                  {handoff.available_skills.length} skills ·{" "}
                  {handoff.available_mcp.length} MCP
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
                {openHandoffs[handoff.handoff_id] ? "Hide envelope" : "Show envelope"}
              </Button>
              {openHandoffs[handoff.handoff_id] ? (
                <div className="handoff-view">
                  {handoff.session_memory.length > 0 ? (
                    <>
                      <strong>Memory carried</strong>
                      {handoff.session_memory.map((entry, index) => (
                        <small key={`dm-${index}`}>{entry}</small>
                      ))}
                    </>
                  ) : null}
                  {handoff.available_skills.length > 0 ? (
                    <>
                      <strong>Skills pointed</strong>
                      {handoff.available_skills.map((skill) => (
                        <small key={skill.path}>
                          {skill.name} — {skill.description} <code>{skill.path}</code>
                        </small>
                      ))}
                    </>
                  ) : null}
                  {handoff.available_mcp.length > 0 ? (
                    <>
                      <strong>MCP servers</strong>
                      {handoff.available_mcp.map((server) => (
                        <small key={server.id}>
                          {server.display_name} <code>{server.transport}</code>
                        </small>
                      ))}
                    </>
                  ) : null}
                  {handoff.transcript.length > 0 ? (
                    <>
                      <strong>Filtered transcript ({handoff.transcript.length} turns)</strong>
                      {handoff.transcript.map((turn, index) => (
                        <small key={`dt-${index}`}>
                          » {turn.prompt}{turn.result ? ` — ${turn.result}` : ""}
                        </small>
                      ))}
                    </>
                  ) : (
                    <small>Transcript not shared (default).</small>
                  )}
                </div>
              ) : null}
            </div>
          ))
        )}
      </Disclosure>
      <Disclosure
        className="details-section"
        description="Versions, streaming health, and setup issues"
        meta={readiness.length}
        title="Diagnostics"
      >
        {readiness.map((engineReadiness) => (
          <div className="details-row" key={engineReadiness.engine_id}>
            <strong>
              {engineReadiness.display_name} · {readinessLabel[engineReadiness.status]}
            </strong>
            <small>
              {engineReadiness.version ?? "version unknown"} · stream{" "}
              {engineReadiness.stream_health}
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
