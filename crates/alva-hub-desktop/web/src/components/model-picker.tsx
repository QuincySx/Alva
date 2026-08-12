// INPUT:  Available engine/model routes, readiness, and selected route
// OUTPUT: Cindy-style anchored engine and model picker with searchable catalog
// POS:    Domain selector shared by new-session and active-session composers

import { useMemo, useState, type CSSProperties } from "react";
import { Bot, Check, ChevronRight, Search } from "lucide-react";
import {
  Badge,
  Button,
  Disclosure,
  PopoverContent,
  PopoverRoot,
  PopoverTrigger,
  StatusListItem,
  TabsList,
  TabsRoot,
  TabsTrigger,
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
        style={{
          "--hub-model-route-count": Math.min(Math.max(visibleRoutes.length, 1), 4),
        } as CSSProperties}
      >
        <aside className="hub-model-picker__detail">
          {inspected ? (
            <>
              <div className="hub-model-picker__detail-heading">
                <span>{inspected.engineLabel}</span>
                <strong>{inspected.modelLabel}</strong>
                <small>{inspected.description ?? inspected.modelId}</small>
              </div>
              <Disclosure
                className="hub-model-picker__details"
                description="Account, provider, and billing route"
                title="More information"
              >
                <dl className="hub-model-picker__facts">
                  <div><dt>Provider</dt><dd>{inspected.providerId}</dd></div>
                  <div><dt>Account</dt><dd>{inspected.accountSource}</dd></div>
                  <div><dt>Billing</dt><dd>{inspected.billingRoute}</dd></div>
                </dl>
              </Disclosure>
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
          <TabsRoot onValueChange={setEngineId} value={engineId}>
            <TabsList aria-label="Agent runtime" className="hub-model-picker__engines">
              {engines.map(([candidateId, label]) => (
                <TabsTrigger
                  className="hub-model-picker__engine"
                  key={candidateId}
                  value={candidateId}
                >
                  {label}
                </TabsTrigger>
              ))}
            </TabsList>
          </TabsRoot>
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
                  <StatusListItem
                    className={cx(
                      "hub-model-picker__route",
                      active && "hub-model-picker__route--active",
                    )}
                    description={
                      route.disabled
                        ? route.disabledReason ?? "Unavailable for this session"
                        : route.providerId
                    }
                    key={routeKey(route)}
                    leading={<Bot aria-hidden="true" size={17} />}
                    onClick={() => {
                      if (route.disabled) return;
                      onSelect(route);
                      setOpen(false);
                    }}
                    status={
                      route.disabled ? (
                        <Badge tone="warning">Unavailable</Badge>
                      ) : route.status ? (
                        <Badge>{route.status}</Badge>
                      ) : (
                        <Badge tone="success">Available</Badge>
                      )
                    }
                    heading={route.modelLabel}
                    trailing={
                      active ? (
                        <Check aria-hidden="true" size={16} />
                      ) : (
                        <ChevronRight aria-hidden="true" size={15} />
                      )
                    }
                    unavailable={route.disabled}
                  />
                );
              })
            )}
          </div>
        </section>
      </PopoverContent>
    </PopoverRoot>
  );
}
