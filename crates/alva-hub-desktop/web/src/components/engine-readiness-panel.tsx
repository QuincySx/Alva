// INPUT:  Engine readiness, repair actions, logs, and refresh callbacks
// OUTPUT: Read-only engine health cards with explicit repair affordances
// POS:    Engines tab business block inside the Hub workbench

import type {
  EngineReadiness,
  RepairAction,
  RepairKind,
} from "../bridge";
import { Button } from "../ui";

export interface EngineReadinessPanelProps {
  busy: boolean;
  describeReason: (reason: EngineReadiness["reasons"][number]) => string;
  label: Record<EngineReadiness["status"], string>;
  logs: Record<string, string>;
  onOpenGuide: (url: string) => void;
  onRefresh: () => void;
  onRepair: (engineId: string, kind: RepairKind) => void;
  readiness: EngineReadiness[];
  repairingEngine: string | null;
  repairPlan: RepairAction[];
}

export function EngineReadinessPanel({
  busy,
  describeReason,
  label,
  logs,
  onOpenGuide,
  onRefresh,
  onRepair,
  readiness,
  repairingEngine,
  repairPlan,
}: EngineReadinessPanelProps) {
  return (
    <>
      <section className="readiness-panel" aria-labelledby="readiness-panel-title">
        <div className="session-heading">
          <span className="eyebrow" id="readiness-panel-title">Engines</span>
          <Button
            className="readiness-refresh"
            loading={busy}
            onClick={onRefresh}
            size="small"
            variant="ghost"
          >
            {busy ? "Checking…" : "Refresh"}
          </Button>
        </div>
        <div className="readiness-list">
          {readiness.length === 0 ? (
            <small className="session-empty">Engine health will appear here.</small>
          ) : (
            readiness.map((engine) => (
              <div className="readiness-card" key={engine.engine_id}>
                <div className="readiness-heading">
                  <strong>{engine.display_name}</strong>
                  <span className={`readiness-badge readiness-${engine.status}`}>
                    {label[engine.status]}
                  </span>
                </div>
                <small>
                  {engine.version
                    ? `v${engine.version} · requires ≥ ${engine.minimum_version}`
                    : `requires ≥ ${engine.minimum_version}`}
                  {" · "}
                  {engine.authenticated === true
                    ? "signed in"
                    : engine.authenticated === false
                      ? "not signed in"
                      : "sign-in unknown"}
                  {" · stream "}{engine.stream_health}
                </small>
                {engine.capabilities.length > 0 ? (
                  <small className="readiness-capabilities">
                    {engine.capabilities.join(" · ")}
                  </small>
                ) : null}
                {engine.reasons.map((reason, index) => (
                  <small className="readiness-reason" key={`${reason.code}-${index}`}>
                    {describeReason(reason)}
                  </small>
                ))}
                {repairPlan
                  .filter((action) => action.engine_id === engine.engine_id)
                  .map((action) => (
                    <div className="repair-action" key={`${action.engine_id}-${action.kind}`}>
                      {action.one_click ? (
                        <Button
                          disabled={repairingEngine !== null}
                          loading={repairingEngine === action.engine_id}
                          onClick={() => onRepair(action.engine_id, action.kind)}
                          size="small"
                        >
                          {repairingEngine === action.engine_id ? "Working…" : action.title}
                        </Button>
                      ) : (
                        <>
                          <small>{action.title}</small>
                          {action.command ? <code>{action.command}</code> : null}
                        </>
                      )}
                      {action.url ? (
                        <Button
                          className="repair-link"
                          onClick={() => onOpenGuide(action.url ?? "")}
                          size="small"
                          variant="ghost"
                        >
                          Official guide
                        </Button>
                      ) : null}
                    </div>
                  ))}
                {logs[engine.engine_id] ? (
                  <small className="repair-log">{logs[engine.engine_id]}</small>
                ) : null}
              </div>
            ))
          )}
        </div>
      </section>
      <div className="ticket-note">
        <span>Durable engine branches</span>
        <small>Alva · Claude · Codex · reversible handoff</small>
      </div>
    </>
  );
}
