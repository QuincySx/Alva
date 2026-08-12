// INPUT:  Engine catalog, editable fallback policy, and persistence callbacks
// OUTPUT: Explicit ordered fallback authorization editor
// POS:    Fallback tab business block inside the Hub workbench

import type { EngineTarget, FallbackPolicy } from "../bridge";
import { Button, Checkbox, Disclosure, PanelSection, TextField } from "../ui";

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
  return (
    <PanelSection
      actions={
        <Button
          aria-label={activeSession ? "Save session chain" : "Save default chain"}
          loading={saving}
          onClick={onSave}
          size="small"
        >
          {saving ? "Saving…" : activeSession ? "Save session" : "Save default"}
        </Button>
      }
      className="fallback-panel"
      description="Hub tries these engines in order when the current one cannot continue."
      heading={activeSession ? "Session fallback" : "Default fallback"}
      meta={`${policy.targets.length} selected`}
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
                <small>{target.account_source}</small>
              </div>
              <Checkbox
                checked={target.automatic_use_authorized}
                label="Allow automatic use"
                onCheckedChange={(checked) =>
                  onUpdateTarget(index, { automatic_use_authorized: checked === true })
                }
              />
              <Disclosure
                className="policy-target__details"
                description="Model, provider, and billing route"
                title="Advanced routing"
              >
                <div className="policy-target__fields">
                  <TextField
                    aria-label={`Model for ${target.engine_id}`}
                    className="policy-model-input"
                    placeholder="Default model"
                    value={target.model_id}
                    onChange={(event) => onUpdateTarget(index, { model_id: event.target.value })}
                  />
                  <TextField
                    aria-label={`Provider for ${target.engine_id}`}
                    className="policy-model-input"
                    placeholder="Provider"
                    value={target.provider_id}
                    onChange={(event) => onUpdateTarget(index, { provider_id: event.target.value })}
                  />
                  <small>{target.billing_route}</small>
                </div>
              </Disclosure>
              <div className="policy-order">
                <Button
                  aria-label={`Move ${target.engine_id} earlier`}
                  disabled={index === 0}
                  onClick={() => onMoveTarget(index, -1)}
                  size="icon"
                  variant="ghost"
                >
                  ↑
                </Button>
                <Button
                  aria-label={`Move ${target.engine_id} later`}
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
