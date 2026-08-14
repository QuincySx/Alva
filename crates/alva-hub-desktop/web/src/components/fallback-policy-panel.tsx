// INPUT:  Engine catalog, editable fallback policy, and persistence callbacks
// OUTPUT: Explicit ordered fallback authorization editor
// POS:    Fallback tab business block inside the Hub workbench

import type { EngineTarget, FallbackPolicy } from "../bridge";
import { Button, Checkbox, Disclosure, PanelSection, TextField } from "../ui";
import { useI18n } from "../i18n";

export interface FallbackPolicyPanelProps {
  activeSession: boolean;
  catalog: EngineTarget[];
  onMoveTarget: (index: number, direction: -1 | 1) => void;
  onSave: () => void;
  onToggleTarget: (engineId: string) => void;
  onUpdateTarget: (index: number, update: Partial<EngineTarget>) => void;
  policy: FallbackPolicy;
  saving: boolean;
}

export function FallbackPolicyPanel({
  activeSession,
  catalog,
  onMoveTarget,
  onSave,
  onToggleTarget,
  onUpdateTarget,
  policy,
  saving,
}: FallbackPolicyPanelProps) {
  const { localize, t } = useI18n();
  return (
    <PanelSection
      actions={
        <Button
          aria-label={activeSession ? t("fallback.saveSessionChain") : t("fallback.saveDefaultChain")}
          loading={saving}
          onClick={onSave}
          size="small"
        >
          {saving ? t("common.saving") : activeSession ? t("fallback.saveSession") : t("fallback.saveDefault")}
        </Button>
      }
      className="fallback-panel"
      description={t("fallback.description")}
      heading={activeSession ? t("fallback.session") : t("fallback.default")}
      meta={t("fallback.selected", { count: policy.targets.length })}
    >
          <div className="policy-catalog">
            {catalog.map((target) => {
              const selected = policy.targets.some(
                (candidate) => candidate.engine_id === target.engine_id,
              );
              return (
                <Button
                  className={`policy-engine ${selected ? "selected" : ""}`}
                  key={target.engine_id}
                  onClick={() => onToggleTarget(target.engine_id)}
                  size="small"
                  variant="ghost"
                >
                  {selected ? "✓ " : "+ "}{target.engine_id}
                </Button>
              );
            })}
          </div>
          {policy.targets.map((target, index) => (
            <div className="policy-target" key={`${target.engine_id}-${index}`}>
              <div className="policy-target__heading">
                <strong>{index + 1}. {target.engine_id}</strong>
                <small>{localize(target.account_source)}</small>
              </div>
              <Checkbox
                checked={target.automatic_use_authorized}
                label={t("fallback.allowAutomatic")}
                onCheckedChange={(checked) =>
                  onUpdateTarget(index, { automatic_use_authorized: checked === true })
                }
              />
              <Disclosure
                className="policy-target__details"
                description={t("fallback.advancedDescription")}
                title={t("fallback.advanced")}
              >
                <div className="policy-target__fields">
                  <TextField
                    aria-label={t("fallback.modelFor", { engine: target.engine_id })}
                    className="policy-model-input"
                    placeholder={t("fallback.defaultModel")}
                    value={target.model_id}
                    onChange={(event) => onUpdateTarget(index, { model_id: event.target.value })}
                  />
                  <TextField
                    aria-label={t("fallback.providerFor", { engine: target.engine_id })}
                    className="policy-model-input"
                    placeholder={t("modelPicker.provider")}
                    value={target.provider_id}
                    onChange={(event) => onUpdateTarget(index, { provider_id: event.target.value })}
                  />
                  <small>{localize(target.billing_route)}</small>
                </div>
              </Disclosure>
              <div className="policy-order">
                <Button
                  aria-label={t("fallback.moveEarlier", { engine: target.engine_id })}
                  disabled={index === 0}
                  onClick={() => onMoveTarget(index, -1)}
                  size="icon"
                  variant="ghost"
                >
                  ↑
                </Button>
                <Button
                  aria-label={t("fallback.moveLater", { engine: target.engine_id })}
                  disabled={index === policy.targets.length - 1}
                  onClick={() => onMoveTarget(index, 1)}
                  size="icon"
                  variant="ghost"
                >
                  ↓
                </Button>
              </div>
            </div>
          ))}
    </PanelSection>
  );
}
