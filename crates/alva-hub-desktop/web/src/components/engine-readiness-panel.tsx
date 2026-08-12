// INPUT:  Engine readiness, repair actions, logs, and refresh callbacks
// OUTPUT: Read-only engine health cards with explicit repair affordances
// POS:    Engines tab business block inside the Hub workbench

import type {
  EngineReadiness,
  RepairAction,
  RepairKind,
} from "../bridge";
import { Badge, Button, Disclosure, Notice, PanelSection } from "../ui";

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

function summarizeReason(reason: EngineReadiness["reasons"][number]): string {
  switch (reason.code) {
    case "binary_not_found":
      return "Install this engine to make it available.";
    case "version_unreadable":
      return "Hub could not verify this engine.";
    case "version_incompatible":
      return "Update this engine before using it.";
    case "unauthenticated":
      return "Sign in before starting a conversation with this engine.";
    case "provider_key_missing":
      return "Add the required provider key before using this engine.";
    case "stream_failing":
      return "This engine is not responding correctly.";
  }
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
      <PanelSection
        actions={
          <Button
            className="readiness-refresh"
            loading={busy}
            onClick={onRefresh}
            size="small"
            variant="ghost"
          >
            {busy ? "Checking…" : "Refresh"}
          </Button>
        }
        className="readiness-panel"
        description="Choose an available runtime for this conversation."
        heading="Engines"
      >
        <div className="readiness-list">
          {readiness.length === 0 ? (
            <small className="session-empty">Engine health will appear here.</small>
          ) : (
            readiness.map((engine) => (
              <div className="readiness-card" key={engine.engine_id}>
                <div className="readiness-heading">
                  <strong>{engine.display_name}</strong>
                  <Badge
                    tone={
                      engine.status === "healthy"
                        ? "success"
                        : engine.status === "missing" ||
                            engine.status === "incompatible" ||
                            engine.status === "unhealthy"
                          ? "danger"
                          : "warning"
                    }
                  >
                    {label[engine.status]}
                  </Badge>
                </div>
                <small>
                  {engine.authenticated === true
                    ? "Ready to use"
                    : engine.authenticated === false
                      ? "Sign in to use this engine"
                      : engine.status === "healthy"
                        ? "Ready to use"
                        : "Needs attention"}
                </small>
                {engine.status !== "healthy" && engine.reasons[0] ? (
                  <small className="readiness-reason">
                    {summarizeReason(engine.reasons[0])}
                  </small>
                ) : null}
                <Disclosure
                  className="readiness-details"
                  description="Version, capabilities, and diagnostics"
                  title="Technical details"
                >
                  <div className="readiness-details__body">
                    <small>
                      {engine.version ? `Version ${engine.version}` : "Version unknown"}
                      {` · requires ${engine.minimum_version} or newer`}
                      {` · streaming ${engine.stream_health}`}
                    </small>
                    {engine.capabilities.length > 0 ? (
                      <small>{engine.capabilities.join(" · ")}</small>
                    ) : null}
                    {engine.reasons.map((reason, index) => (
                      <small key={`${reason.code}-${index}`}>{describeReason(reason)}</small>
                    ))}
                  </div>
                </Disclosure>
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
      </PanelSection>
      <Notice
        description="Each engine keeps its own conversation state."
        heading="Switch engines without losing your place"
      />
    </>
  );
}
