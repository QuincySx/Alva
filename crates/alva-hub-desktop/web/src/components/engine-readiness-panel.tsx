// INPUT:  Engine readiness, repair actions, logs, and refresh callbacks
// OUTPUT: Read-only engine health cards with explicit repair affordances
// POS:    Engines tab business block inside the Hub workbench

import type {
  EngineReadiness,
  RepairAction,
  RepairKind,
} from "../bridge";
import { Badge, Button, Disclosure, Notice, PanelSection } from "../ui";
import { useI18n, type Translate } from "../i18n";

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

function summarizeReason(reason: EngineReadiness["reasons"][number], t: Translate): string {
  switch (reason.code) {
    case "dependency_unavailable":
      return t("readiness.install");
    case "binary_not_found":
      return t("readiness.install");
    case "version_unreadable":
      return t("readiness.verifyFailed");
    case "version_incompatible":
      return t("readiness.update");
    case "unauthenticated":
      return t("readiness.signInFirst");
    case "provider_key_missing":
      return t("readiness.addKey");
    case "stream_failing":
      return t("readiness.notResponding");
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
  const { locale, localize, t } = useI18n();
  const repairTitle = (action: RepairAction, engine: string) => {
    if (locale === "en-US") return action.title;
    switch (action.kind) {
      case "install": return t("readiness.repairInstall", { engine });
      case "upgrade": return t("readiness.repairUpgrade", { engine });
      case "authenticate": return t("readiness.repairAuthenticate", { engine });
      case "configure": return t("readiness.repairConfigure");
    }
  };
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
            {busy ? t("common.checking") : t("common.refresh")}
          </Button>
        }
        className="readiness-panel"
        description={t("readiness.description")}
        heading={t("workbench.engines")}
      >
        <div className="readiness-list">
          {readiness.length === 0 ? (
            <small className="session-empty">{t("readiness.empty")}</small>
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
                    {localize(label[engine.status])}
                  </Badge>
                </div>
                <small>
                  {engine.authenticated === true
                    ? t("readiness.ready")
                    : engine.authenticated === false
                      ? t("readiness.signIn")
                      : engine.status === "healthy"
                        ? t("readiness.ready")
                        : t("readiness.attention")}
                </small>
                {engine.status !== "healthy" && engine.reasons[0] ? (
                  <small className="readiness-reason">
                    {summarizeReason(engine.reasons[0], t)}
                  </small>
                ) : null}
                <Disclosure
                  className="readiness-details"
                  description={t("readiness.technicalDescription")}
                  title={t("readiness.technical")}
                >
                  <div className="readiness-details__body">
                    <small>
                      {engine.version ? t("readiness.version", { version: engine.version }) : t("readiness.versionUnknown")}
                      {` · ${t("readiness.requires", { version: engine.minimum_version })}`}
                      {` · ${t("readiness.streaming", { health: localize(engine.stream_health) })}`}
                    </small>
                    {engine.capabilities.length > 0 ? (
                      <small>{engine.capabilities.map(localize).join(" · ")}</small>
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
                          {repairingEngine === action.engine_id ? t("common.working") : repairTitle(action, engine.display_name)}
                        </Button>
                      ) : (
                        <>
                          <small>{repairTitle(action, engine.display_name)}</small>
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
                          {t("readiness.officialGuide")}
                        </Button>
                      ) : null}
                    </div>
                  ))}
                {logs[engine.engine_id] ? (
                  <small className="repair-log">{localize(logs[engine.engine_id])}</small>
                ) : null}
              </div>
            ))
          )}
        </div>
      </PanelSection>
      <Notice
        description={t("readiness.switchDescription")}
        heading={t("readiness.switchHeading")}
      />
    </>
  );
}
