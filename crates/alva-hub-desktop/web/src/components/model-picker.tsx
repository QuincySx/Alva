// INPUT:  Available engine/model routes, readiness, and selected route
// OUTPUT: Cindy-style route catalog with a row-anchored model-options flyout
// POS:    Domain selector shared by new-session and active-session composers

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
} from "react";
import { Bot, Check, ChevronRight, Search } from "lucide-react";
import { useI18n } from "../i18n";
import {
  Badge,
  Button,
  Disclosure,
  PopoverContent,
  PopoverRoot,
  PopoverTrigger,
  StatusListItem,
  Switch,
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
  reasoningEfforts: string[];
  reasoningEffort: string | null;
  supportsFastMode: boolean;
  fastMode: boolean;
  status?: string;
  disabled?: boolean;
  disabledReason?: string;
}

export interface ModelPickerProps {
  routes: HubModelRoute[];
  selectedEngineId: string;
  selectedModelId: string;
  selectedProviderId: string;
  onSelect: (route: HubModelRoute) => void;
  onOptionsChange: (
    route: HubModelRoute,
    update: { reasoningEffort?: string; fastMode?: boolean },
  ) => void;
}

function routeKey(
  route: Pick<HubModelRoute, "engineId" | "providerId" | "modelId">,
) {
  return JSON.stringify([route.engineId, route.providerId, route.modelId]);
}

export function ModelPicker({
  routes,
  selectedEngineId,
  selectedModelId,
  selectedProviderId,
  onSelect,
  onOptionsChange,
}: ModelPickerProps) {
  const { localize, t } = useI18n();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [engineId, setEngineId] = useState(selectedEngineId);
  const [inspection, setInspection] = useState<{ key: string; top: number } | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const detailRef = useRef<HTMLElement | null>(null);
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const engines = useMemo(
    () =>
      Array.from(
        new Map(routes.map((route) => [route.engineId, route.engineLabel])).entries(),
      ),
    [routes],
  );
  const selectedKey = routeKey({
    engineId: selectedEngineId,
    modelId: selectedModelId,
    providerId: selectedProviderId,
  });
  const selected = routes.find((route) => routeKey(route) === selectedKey);
  // R34: a search query spans every engine tab — one unified model search.
  // Without a query the catalog stays scoped to the active engine tab.
  const searching = query.trim().length > 0;
  const visibleRoutes = routes.filter((route) => {
    if (!searching && route.engineId !== engineId) return false;
    const haystack = `${route.modelLabel} ${route.modelId} ${route.providerId} ${route.engineLabel}`.toLowerCase();
    return haystack.includes(query.trim().toLowerCase());
  });
  const availableRoutes = visibleRoutes.filter((route) => !route.disabled);
  const unavailableRoutes = visibleRoutes.filter((route) => route.disabled);
  const inspected = inspection
    ? visibleRoutes.find(
        (route) => !route.disabled && routeKey(route) === inspection.key,
      )
    : undefined;

  useEffect(
    () => () => {
      if (closeTimerRef.current) clearTimeout(closeTimerRef.current);
    },
    [],
  );

  const cancelInspectionClose = () => {
    if (!closeTimerRef.current) return;
    clearTimeout(closeTimerRef.current);
    closeTimerRef.current = null;
  };

  const closeInspectionSoon = () => {
    cancelInspectionClose();
    closeTimerRef.current = setTimeout(() => {
      setInspection(null);
      closeTimerRef.current = null;
    }, 120);
  };

  const inspectRoute = (route: HubModelRoute, anchor: HTMLElement) => {
    cancelInspectionClose();
    if (route.disabled) {
      setInspection(null);
      return;
    }
    const contentTop = contentRef.current?.getBoundingClientRect().top ?? 0;
    const desiredTop = anchor.getBoundingClientRect().top - contentTop;
    // The detail panel is capped at 480px and translated 18px upward in CSS.
    // Clamp against the viewport rather than the catalog so a bottom row does
    // not push its options below the visible screen.
    const panelHeight = Math.min(480, Math.max(0, window.innerHeight - 40));
    const viewportTop = 20 - contentTop + 18;
    const viewportBottom = window.innerHeight - 20 - contentTop - panelHeight + 18;
    setInspection({
      key: routeKey(route),
      top: Math.min(Math.max(desiredTop, viewportTop), viewportBottom),
    });
  };

  const handleRouteBlur = (event: FocusEvent<HTMLElement>) => {
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && detailRef.current?.contains(nextTarget)) {
      cancelInspectionClose();
      return;
    }
    closeInspectionSoon();
  };

  const effortLabel = (effort: string) => {
    switch (effort) {
      case "off":
      case "none":
        return t("modelPicker.effortOff");
      case "minimal":
        return t("modelPicker.effortMinimal");
      case "low":
        return t("modelPicker.effortLow");
      case "medium":
        return t("modelPicker.effortMedium");
      case "high":
        return t("modelPicker.effortHigh");
      case "xhigh":
        return t("modelPicker.effortXHigh");
      case "max":
        return t("modelPicker.effortMax");
      case "ultra":
        return t("modelPicker.effortUltra");
      default:
        return effort;
    }
  };

  const renderRoute = (route: HubModelRoute) => {
    const active = routeKey(route) === selectedKey;
    return (
      <div
        className="hub-model-picker__route-anchor"
        key={routeKey(route)}
        onBlur={handleRouteBlur}
        onFocus={(event) => inspectRoute(route, event.currentTarget)}
        onPointerLeave={closeInspectionSoon}
        onPointerMove={(event) => inspectRoute(route, event.currentTarget)}
      >
        <StatusListItem
          className={cx(
            "hub-model-picker__route",
            active && "hub-model-picker__route--active",
          )}
          description={
            route.disabled
              ? localize(route.disabledReason) || t("modelPicker.unavailableSession")
              : searching
                ? `${route.engineLabel} · ${route.providerId}`
                : route.providerId
          }
          leading={<Bot aria-hidden="true" size={17} />}
          onClick={() => {
            if (route.disabled) return;
            onSelect(route);
            setOpen(false);
          }}
          status={
            route.disabled ? (
              <Badge tone="warning">{t("modelPicker.unavailable")}</Badge>
            ) : route.status ? (
              <Badge>{localize(route.status)}</Badge>
            ) : (
              <Badge tone="success">{t("modelPicker.available")}</Badge>
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
      </div>
    );
  };

  return (
    <PopoverRoot
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (nextOpen) {
          setEngineId(selectedEngineId);
          setQuery("");
        }
        setInspection(null);
      }}
    >
      <PopoverTrigger asChild>
        <Button
          aria-label={t("modelPicker.label")}
          className="hub-model-trigger"
          leadingIcon={<Bot aria-hidden="true" size={15} />}
          variant="ghost"
        >
          {selected
            ? `${selected.engineLabel} · ${selected.modelLabel}`
            : t("modelPicker.choose")}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="hub-model-picker"
        collisionPadding={20}
        ref={contentRef}
        side="top"
        sideOffset={10}
        style={{
          "--hub-model-route-count": Math.min(Math.max(visibleRoutes.length, 1), 4),
        } as CSSProperties}
      >
        <section className="hub-model-picker__catalog">
          {searching ? (
            <small className="hub-model-picker__scope">{t("modelPicker.searchAll")}</small>
          ) : (
            <TabsRoot
              onValueChange={(nextEngineId) => {
                setEngineId(nextEngineId);
                setInspection(null);
              }}
              value={engineId}
            >
              <TabsList aria-label={t("modelPicker.runtime")} className="hub-model-picker__engines">
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
          )}
          <label className="hub-model-picker__search">
            <Search aria-hidden="true" size={17} />
            <TextField
              aria-label={t("modelPicker.search")}
              placeholder={t("modelPicker.searchPlaceholder")}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setInspection(null);
              }}
            />
          </label>
          <div className="hub-model-picker__list">
            {visibleRoutes.length === 0 ? (
              <small className="hub-model-picker__empty">{t("modelPicker.noMatches")}</small>
            ) : (
              <>
                {availableRoutes.map(renderRoute)}
                {unavailableRoutes.length > 0 ? (
                  <section
                    aria-label={t("modelPicker.unavailableGroup")}
                    className="hub-model-picker__unavailable-group"
                  >
                    <div className="hub-model-picker__group-heading">
                      <span>{t("modelPicker.unavailableGroup")}</span>
                      <Badge tone="warning">{unavailableRoutes.length}</Badge>
                    </div>
                    {unavailableRoutes.map(renderRoute)}
                  </section>
                ) : null}
              </>
            )}
          </div>
        </section>
        {inspected ? (
          <aside
            aria-label={t("modelPicker.optionsFor", { model: inspected.modelLabel })}
            className="hub-model-picker__detail"
            onBlur={handleRouteBlur}
            onFocus={cancelInspectionClose}
            onPointerEnter={cancelInspectionClose}
            onPointerLeave={closeInspectionSoon}
            ref={detailRef}
            style={{
              "--hub-model-options-top": `${inspection?.top ?? 8}px`,
            } as CSSProperties}
          >
            <div className="hub-model-picker__detail-heading">
              <span>{t("modelPicker.options")}</span>
              <strong>{inspected.modelLabel}</strong>
              <small>{inspected.description ? localize(inspected.description) : inspected.modelId}</small>
            </div>
            <div className="hub-model-picker__option-controls">
              {inspected.supportsFastMode ? (
                <Switch
                  checked={inspected.fastMode}
                  description={t("modelPicker.fastDescription")}
                  label={t("modelPicker.fast")}
                  onCheckedChange={(checked) =>
                    onOptionsChange(inspected, { fastMode: checked })
                  }
                />
              ) : null}
              {inspected.reasoningEfforts.length > 0 ? (
                <div className="hub-model-picker__effort">
                  <span>{t("modelPicker.reasoning")}</span>
                  <div
                    aria-label={t("modelPicker.reasoning")}
                    className="hub-model-picker__effort-list"
                    role="listbox"
                  >
                    {inspected.reasoningEfforts.map((effort) => {
                      const selected = inspected.reasoningEffort === effort;
                      return (
                        <button
                          aria-selected={selected}
                          className={cx(
                            "hub-model-picker__effort-option",
                            selected && "hub-model-picker__effort-option--selected",
                          )}
                          key={effort}
                          onClick={() =>
                            onOptionsChange(inspected, { reasoningEffort: effort })
                          }
                          role="option"
                          type="button"
                        >
                          <span>{effortLabel(effort)}</span>
                          {selected ? <Check aria-hidden="true" size={15} /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : (
                <small className="hub-model-picker__no-options">
                  {t("modelPicker.reasoningUnavailable")}
                </small>
              )}
            </div>
            <div className="hub-model-picker__capabilities">
              <small style={{ opacity: 0.6 }}>{t("modelPicker.capabilities")}</small>
            </div>
            <Disclosure
              className="hub-model-picker__details"
              description={t("modelPicker.moreDescription")}
              title={t("modelPicker.more")}
            >
              <dl className="hub-model-picker__facts">
                <div><dt>{t("modelPicker.provider")}</dt><dd>{inspected.providerId}</dd></div>
                <div><dt>{t("modelPicker.account")}</dt><dd>{localize(inspected.accountSource)}</dd></div>
                <div><dt>{t("modelPicker.billing")}</dt><dd>{localize(inspected.billingRoute)}</dd></div>
              </dl>
            </Disclosure>
          </aside>
        ) : null}
      </PopoverContent>
    </PopoverRoot>
  );
}
