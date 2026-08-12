// INPUT:  Engine catalog, editable fallback policy, and persistence callbacks
// OUTPUT: Explicit ordered fallback authorization editor
// POS:    Fallback tab business block inside the Hub workbench

import { useState } from "react";
import type { EngineTarget, FallbackPolicy } from "../bridge";
import { Button, Checkbox, TextField } from "../ui";

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
  const [collapsed, setCollapsed] = useState(true);

  return (
    <section className="fallback-panel" aria-labelledby="fallback-panel-title">
      <div className="session-heading">
        <Button
          aria-expanded={!collapsed}
          aria-label="Toggle fallback chain"
          className="section-collapse"
          onClick={() => setCollapsed((value) => !value)}
          size="icon"
          variant="ghost"
        >
          {collapsed ? "▸" : "▾"}
        </Button>
        <span className="eyebrow" id="fallback-panel-title">
          {activeSession ? "Session fallback chain" : "Default fallback chain"}
        </span>
        <small>{policy.targets.length}</small>
      </div>
      {collapsed ? null : (
        <>
          <p className="policy-note">
            Only checked targets may run automatically. Manual engine switching is always explicit.
          </p>
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
              <div>
                <strong>{index + 1}. {target.engine_id}</strong>
                <TextField
                  aria-label={`Model for ${target.engine_id}`}
                  className="policy-model-input"
                  placeholder="engine-default"
                  value={target.model_id}
                  onChange={(event) => onUpdateTarget(index, { model_id: event.target.value })}
                />
                <TextField
                  aria-label={`Provider for ${target.engine_id}`}
                  className="policy-model-input"
                  placeholder="provider id"
                  value={target.provider_id}
                  onChange={(event) => onUpdateTarget(index, { provider_id: event.target.value })}
                />
              </div>
              <small>{target.account_source}</small>
              <small>{target.billing_route}</small>
              <Checkbox
                checked={target.automatic_use_authorized}
                label="Authorize automatic use"
                onCheckedChange={(checked) =>
                  onUpdateTarget(index, { automatic_use_authorized: checked === true })
                }
              />
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
          <Button
            className="secondary-button policy-save"
            loading={saving}
            onClick={onSave}
          >
            {saving ? "Saving…" : activeSession ? "Save session chain" : "Save default chain"}
          </Button>
        </>
      )}
    </section>
  );
}
