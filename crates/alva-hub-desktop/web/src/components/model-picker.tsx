// INPUT:  Available engine/model routes, readiness, and selected route
// OUTPUT: Cindy-style anchored engine and model picker with searchable catalog
// POS:    Domain selector shared by new-session and active-session composers

import { useMemo, useState } from "react";
import { Bot, Check, Search } from "lucide-react";
import {
  Badge,
  Button,
  PopoverContent,
  PopoverRoot,
  PopoverTrigger,
  TextField,
  cx,
} from "../ui";

export interface HubModelRoute {
  engineId: string;
  engineLabel: string;
  modelId: string;
  modelLabel: string;
  providerId: string;
  accountSource: string;
  billingRoute: string;
  description?: string;
  capabilities?: string[];
  status?: string;
  disabled?: boolean;
  disabledReason?: string;
}

export interface ModelPickerProps {
  routes: HubModelRoute[];
  selectedEngineId: string;
  selectedModelId: string;
  onSelect: (route: HubModelRoute) => void;
}

function routeKey(route: HubModelRoute) {
  return `${route.engineId}:${route.providerId}:${route.modelId}`;
}

export function ModelPicker({
  routes,
  selectedEngineId,
  selectedModelId,
  onSelect,
}: ModelPickerProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [engineId, setEngineId] = useState(selectedEngineId);
  const engines = useMemo(
    () =>
      Array.from(
        new Map(routes.map((route) => [route.engineId, route.engineLabel])).entries(),
      ),
    [routes],
  );
  const selected =
    routes.find(
      (route) =>
        route.engineId === selectedEngineId && route.modelId === selectedModelId,
    ) ?? routes.find((route) => route.engineId === selectedEngineId) ?? routes[0];
  const visibleRoutes = routes.filter((route) => {
    if (route.engineId !== engineId) return false;
    const haystack = `${route.modelLabel} ${route.modelId} ${route.providerId}`.toLowerCase();
    return haystack.includes(query.trim().toLowerCase());
  });
  const inspected =
    visibleRoutes.find((route) => routeKey(route) === routeKey(selected)) ??
    visibleRoutes[0] ??
    selected;

  return (
    <PopoverRoot
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (nextOpen) {
          setEngineId(selectedEngineId);
          setQuery("");
        }
      }}
    >
      <PopoverTrigger asChild>
        <Button
          aria-label="Model and engine"
          className="hub-model-trigger"
          leadingIcon={<Bot aria-hidden="true" size={15} />}
          variant="ghost"
        >
          {selected
            ? `${selected.engineLabel} · ${selected.modelLabel}`
            : "Choose model"}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="hub-model-picker"
        collisionPadding={20}
        side="top"
        sideOffset={10}
      >
        <aside className="hub-model-picker__detail">
          {inspected ? (
            <>
              <div className="hub-model-picker__detail-heading">
                <span>{inspected.engineLabel}</span>
                <strong>{inspected.modelLabel}</strong>
                <small>{inspected.description ?? inspected.modelId}</small>
              </div>
              <dl className="hub-model-picker__facts">
                <div><dt>Provider</dt><dd>{inspected.providerId}</dd></div>
                <div><dt>Account</dt><dd>{inspected.accountSource}</dd></div>
                <div><dt>Billing</dt><dd>{inspected.billingRoute}</dd></div>
              </dl>
              <div className="hub-model-picker__capabilities">
                {(inspected.capabilities ?? []).length > 0 ? (
                  inspected.capabilities?.map((capability) => (
                    <Badge key={capability}>{capability}</Badge>
                  ))
                ) : (
                  <small>Capabilities are reported by the selected engine.</small>
                )}
              </div>
            </>
          ) : null}
        </aside>
        <section className="hub-model-picker__catalog">
          <div className="hub-model-picker__engines" role="tablist" aria-label="Agent runtime">
            {engines.map(([candidateId, label]) => (
              <button
                aria-selected={engineId === candidateId}
                className={cx(
                  "hub-model-picker__engine",
                  engineId === candidateId && "hub-model-picker__engine--active",
                )}
                key={candidateId}
                onClick={() => setEngineId(candidateId)}
                role="tab"
                type="button"
              >
                {label}
              </button>
            ))}
          </div>
          <label className="hub-model-picker__search">
            <Search aria-hidden="true" size={17} />
            <TextField
              aria-label="Search models"
              placeholder="Search models…"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <div className="hub-model-picker__list">
            {visibleRoutes.length === 0 ? (
              <small className="hub-model-picker__empty">No matching models.</small>
            ) : (
              visibleRoutes.map((route) => {
                const active = routeKey(route) === routeKey(selected);
                return (
                  <button
                    className={cx(
                      "hub-model-picker__route",
                      active && "hub-model-picker__route--active",
                    )}
                    disabled={route.disabled}
                    key={routeKey(route)}
                    onClick={() => {
                      onSelect(route);
                      setOpen(false);
                    }}
                    title={route.disabledReason}
                    type="button"
                  >
                    <Bot aria-hidden="true" size={17} />
                    <span>
                      <strong>{route.modelLabel}</strong>
                      <small>{route.providerId}</small>
                    </span>
                    {route.status ? <Badge>{route.status}</Badge> : null}
                    {active ? <Check aria-hidden="true" size={16} /> : null}
                  </button>
                );
              })
            )}
          </div>
        </section>
      </PopoverContent>
    </PopoverRoot>
  );
}
