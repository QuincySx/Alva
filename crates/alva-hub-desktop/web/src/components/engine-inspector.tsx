// INPUT:  HubSessionDetail, stable HubInspectionSnapshot IPC, shared Hub controls
// OUTPUT: Live/historical recursive Agent Inspector with detail-page navigation and back stack
// POS:    Workbench domain component; projects durable engine records without consuming raw adapter events

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  Bot,
  BrainCircuit,
  Braces,
  RefreshCw,
  Timer,
  Wrench,
} from "lucide-react";
import {
  getHubInspection,
  type HubInspectionSnapshot,
  type HubSessionDetail,
  type InspectorHookRecord,
  type InspectorLlmCallRecord,
  type InspectorToolCallRecord,
} from "../bridge";
import { useI18n } from "../i18n";
import {
  Badge,
  Button,
  Disclosure,
  EmptyState,
  ListRow,
  Notice,
  PanelSection,
} from "../ui";

type InspectorLoader = (
  productSessionId: string,
  nativeSessionId?: string | null,
) => Promise<HubInspectionSnapshot>;

type DetailPage =
  | { kind: "llm"; turnIndex: number }
  | { kind: "tool"; turnIndex: number; toolIndex: number }
  | { kind: "hook"; hook: InspectorHookRecord; title: string }
  | { kind: "turnHooks"; turnIndex: number };

export interface EngineInspectorPanelProps {
  detail: HubSessionDetail | null;
  loadInspection?: InspectorLoader;
  /** R33: external focus request — open this native session (e.g. a product
   *  attempt selected elsewhere in the Workbench). Cleared once consumed. */
  focusSession?: string | null;
  onFocusConsumed?: () => void;
}

function shortId(value: string): string {
  return value.slice(0, 8);
}

function pretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function inspectionRoot(detail: HubSessionDetail | null): string | null {
  if (!detail) return null;
  const active = detail.branches.find(
    (branch) => branch.branch_id === detail.session.active_branch_id,
  );
  if (active?.engine_id === "alva" && active.engine_session_id) {
    return active.engine_session_id;
  }
  return detail.branches
    .filter((branch) => branch.engine_id === "alva" && branch.engine_session_id)
    .sort((left, right) => right.updated_at - left.updated_at)[0]
    ?.engine_session_id ?? null;
}

function hookFailures(hooks: InspectorHookRecord[]): number {
  return hooks.filter((hook) => hook.error !== null || hook.outcome === "error").length;
}

function tokensOrUnknown(input: number, output: number): string {
  return input === 0 && output === 0 ? "—" : `${input}/${output}`;
}

/** Middleware defaults to a collapsed count so successful hooks never bury LLM/Tool steps. */
function HookRows({
  hooks,
  onOpen,
}: {
  hooks: InspectorHookRecord[];
  onOpen: (hook: InspectorHookRecord, originKey: string) => void;
}) {
  const { t } = useI18n();
  if (hooks.length === 0) return null;
  const failures = hookFailures(hooks);
  return (
    <Disclosure
      defaultOpen={false}
      description={t("inspector.inclusiveHint")}
      meta={failures > 0 ? `${hooks.length} · ${failures} ✕` : `${hooks.length}`}
      title={t("inspector.middleware")}
    >
      {hooks.map((hook, index) => (
        <ListRow
          actions={(
            <Button
              data-inspector-origin={`mw:${index}`}
              onClick={() => onOpen(hook, `mw:${index}`)}
              size="small"
              variant="ghost"
            >
              {t("common.open")}
            </Button>
          )}
          description={`${hook.hook} · ${hook.outcome}`}
          heading={hook.middleware_name}
          key={`${hook.middleware_name}-${hook.hook}-${index}`}
          meta={`${hook.duration_ms}ms`}
          leading={<Timer aria-hidden="true" size={15} />}
        />
      ))}
    </Disclosure>
  );
}

/** Full per-step hook chain; lives on the step's second-level page, not the overview. */
function HookChain({ hooks }: { hooks: InspectorHookRecord[] }) {
  const { t } = useI18n();
  if (hooks.length === 0) return null;
  return (
    <Disclosure
      defaultOpen={false}
      description={t("inspector.inclusiveHint")}
      meta={`${hookFailures(hooks) > 0 ? `${hookFailures(hooks)} ✕ · ` : ""}${hooks.length}`}
      title={t("inspector.middleware")}
    >
      {hooks.map((hook, index) => (
        <ListRow
          description={`${hook.hook} · ${hook.outcome}`}
          heading={hook.middleware_name}
          key={`${hook.middleware_name}-${hook.hook}-${index}`}
          meta={`${hook.duration_ms}ms`}
          leading={<Timer aria-hidden="true" size={15} />}
        />
      ))}
    </Disclosure>
  );
}

function LlmDetail({ llm }: { llm: InspectorLlmCallRecord }) {
  const { t } = useI18n();
  return (
    <div className="hub-engine-inspector__detail">
      <PanelSection
        description={`${llm.stream_transport ?? "—"} · ${llm.stream_outcome ?? llm.stop_reason}`}
        heading={t("inspector.llmCall")}
        meta={`${llm.duration_ms}ms`}
      >
        <div className="hub-engine-inspector__metrics">
          <span><small>{t("inspector.firstOutput")}</small><strong>{llm.time_to_first_output_ms ?? "—"}ms</strong></span>
          <span><small>{t("inspector.firstToken")}</small><strong>{llm.time_to_first_token_ms ?? "—"}ms</strong></span>
          <span><small>{t("inspector.streamDuration")}</small><strong>{llm.stream_duration_ms ?? "—"}ms</strong></span>
          <span><small>{t("inspector.events")}</small><strong>{llm.visible_stream_event_count}/{llm.stream_event_count}</strong></span>
          <span><small>{t("inspector.tokens")}</small><strong>{llm.input_tokens}/{llm.output_tokens}</strong></span>
        </div>
      </PanelSection>
      <Disclosure title={t("inspector.messagesSent")} meta={`${llm.messages_sent_count}`}>
        <pre className="hub-engine-inspector__json">{pretty(llm.messages_sent)}</pre>
      </Disclosure>
      <Disclosure title={t("inspector.response")}>
        <pre className="hub-engine-inspector__json">{pretty(llm.response)}</pre>
      </Disclosure>
      <HookChain hooks={llm.middleware_hooks} />
      {llm.error_message ? (
        <Notice heading={t("inspector.failure")} description={llm.error_message} tone="danger" />
      ) : null}
    </div>
  );
}

function ToolDetail({ tool }: { tool: InspectorToolCallRecord }) {
  const { t } = useI18n();
  return (
    <div className="hub-engine-inspector__detail">
      <PanelSection
        description={tool.tool_call.id}
        heading={`${t("inspector.toolCall")} · ${tool.tool_call.name}`}
        meta={`${tool.duration_ms}ms`}
      >
        <Badge tone={tool.is_error ? "danger" : "success"}>
          {tool.is_error ? t("inspector.failed") : t("inspector.succeeded")}
        </Badge>
      </PanelSection>
      <Disclosure defaultOpen title={t("inspector.arguments")}>
        <pre className="hub-engine-inspector__json">{pretty(tool.tool_call.arguments)}</pre>
      </Disclosure>
      <Disclosure title={t("inspector.result")}>
        <pre className="hub-engine-inspector__json">{pretty(tool.result)}</pre>
      </Disclosure>
      <HookChain hooks={tool.middleware_hooks} />
    </div>
  );
}

function HookDetail({ hook, title }: { hook: InspectorHookRecord; title: string }) {
  const { t } = useI18n();
  return (
    <PanelSection description={hook.hook} heading={title} meta={`${hook.duration_ms}ms`}>
      <div className="hub-engine-inspector__metrics">
        <span><small>{t("inspector.outcome")}</small><strong>{hook.outcome}</strong></span>
        <span><small>{t("inspector.toolCallId")}</small><strong>{hook.tool_call_id ?? "—"}</strong></span>
      </div>
      {hook.error ? <Notice heading={t("inspector.failure")} description={hook.error} tone="danger" /> : null}
    </PanelSection>
  );
}

export function EngineInspectorPanel({
  detail,
  loadInspection = getHubInspection,
  focusSession = null,
  onFocusConsumed,
}: EngineInspectorPanelProps) {
  const { t } = useI18n();
  const productSessionId = detail?.session.product_session_id ?? null;
  const rootNativeSessionId = useMemo(() => inspectionRoot(detail), [detail]);
  const activeProductRef = useRef(productSessionId);
  const [path, setPath] = useState<string[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, HubInspectionSnapshot>>({});
  const [selection, setSelection] = useState<DetailPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // R33: per-level scroll memory — leaving a level stores its scrollTop, Back
  // restores it, so a deep drill-down never dumps the user at the top.
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const scrollMemory = useRef<Record<string, number>>({});
  // R33: LIFO stack of data-inspector-origin keys. The list that opened a
  // level unmounts while a detail page is shown, so Back re-queries the
  // re-rendered control by key instead of holding a detached DOM node.
  const focusOriginStack = useRef<string[]>([]);
  const navigationKey = `${path.join(">")}#${selection ? selection.kind : ""}`;

  activeProductRef.current = productSessionId;
  const currentSessionId = path.at(-1) ?? null;
  const current = currentSessionId ? snapshots[currentSessionId] ?? null : null;

  const fetchSnapshot = useCallback(async (
    nativeSessionId: string | null,
    showLoading: boolean,
  ) => {
    if (!productSessionId) return null;
    const requestedProduct = productSessionId;
    if (showLoading) setLoading(true);
    try {
      const snapshot = await loadInspection(requestedProduct, nativeSessionId);
      if (activeProductRef.current !== requestedProduct) return null;
      setSnapshots((known) => ({ ...known, [snapshot.native_session_id]: snapshot }));
      setError(null);
      return snapshot;
    } catch (reason) {
      if (activeProductRef.current === requestedProduct) {
        setError(reason instanceof Error ? reason.message : String(reason));
      }
      return null;
    } finally {
      if (showLoading && activeProductRef.current === requestedProduct) setLoading(false);
    }
  }, [loadInspection, productSessionId]);

  useEffect(() => {
    setPath([]);
    setSnapshots({});
    setSelection(null);
    setError(null);
    focusOriginStack.current = [];
    if (!productSessionId || !rootNativeSessionId) return;
    let cancelled = false;
    setLoading(true);
    void loadInspection(productSessionId, rootNativeSessionId)
      .then((snapshot) => {
        if (cancelled || activeProductRef.current !== productSessionId) return;
        setSnapshots({ [snapshot.native_session_id]: snapshot });
        setPath([snapshot.native_session_id]);
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [loadInspection, productSessionId, rootNativeSessionId]);

  useEffect(() => {
    if (!current?.live) return;
    const timer = window.setInterval(() => {
      void fetchSnapshot(current.native_session_id, false);
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [current?.live, current?.native_session_id, fetchSnapshot]);

  // R33: remember the scroll position of the level we are leaving, and
  // restore the level we re-enter (0 for fresh levels).
  const previousNavigation = useRef(navigationKey);
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (scroller && previousNavigation.current !== navigationKey) {
      scrollMemory.current[previousNavigation.current] = scroller.scrollTop;
    }
    previousNavigation.current = navigationKey;
    if (scroller) scroller.scrollTop = scrollMemory.current[navigationKey] ?? 0;
  }, [navigationKey]);

  // R33: an external focus request (product attempt selection) opens the
  // requested native session. If it is already on the stack the path is
  // truncated to that level, so "locate" always lands on the focused session.
  useEffect(() => {
    if (!focusSession || !productSessionId) return;
    const locate = (currentPath: string[]) => {
      const index = currentPath.indexOf(focusSession);
      return index >= 0 ? currentPath.slice(0, index + 1) : [...currentPath, focusSession];
    };
    if (snapshots[focusSession]) {
      focusOriginStack.current = [];
      setSelection(null);
      setPath(locate);
      onFocusConsumed?.();
      return;
    }
    let cancelled = false;
    setLoading(true);
    void loadInspection(productSessionId, focusSession)
      .then((snapshot) => {
        if (cancelled || activeProductRef.current !== productSessionId) return;
        setSnapshots((known) => ({ ...known, [snapshot.native_session_id]: snapshot }));
        focusOriginStack.current = [];
        setSelection(null);
        setPath(locate);
        onFocusConsumed?.();
      })
      .catch((reason) => {
        if (!cancelled && activeProductRef.current === productSessionId) {
          setError(reason instanceof Error ? reason.message : String(reason));
          // Surface the failure once; the parent clears the request so a live
          // polling loop cannot re-trigger the same failed load every second.
          onFocusConsumed?.();
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // onFocusConsumed is a parent callback consumed inline; excluding it from
    // deps keeps the effect from re-firing on every parent render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusSession, productSessionId, snapshots, loadInspection]);

  const goBack = () => {
    if (!selection && path.length <= 1) return;
    // R33: restore focus after the previous level re-renders; the scroll
    // effect restores its position in the same commit cycle.
    const originKey = focusOriginStack.current.pop() ?? null;
    if (selection) {
      setSelection(null);
    } else {
      setPath((currentPath) => currentPath.slice(0, -1));
    }
    if (originKey) {
      requestAnimationFrame(() => {
        scrollerRef.current
          ?.querySelector<HTMLElement>(`[data-inspector-origin="${originKey}"]`)
          ?.focus();
      });
    }
  };

  const openChild = async (nativeSessionId: string, originKey: string) => {
    focusOriginStack.current = [...focusOriginStack.current, originKey];
    const snapshot = await fetchSnapshot(nativeSessionId, true);
    if (!snapshot) return;
    setSelection(null);
    setPath((currentPath) => [...currentPath, snapshot.native_session_id]);
  };

  const openDetail = (page: DetailPage, originKey: string) => {
    focusOriginStack.current = [...focusOriginStack.current, originKey];
    setSelection(page);
  };

  if (!detail) {
    return <EmptyState icon={<Braces size={18} />} title={t("tool.inspectSession")} />;
  }
  if (!rootNativeSessionId) {
    return (
      <EmptyState
        icon={<Braces size={18} />}
        title={t("inspector.unavailable")}
        description={t("inspector.unavailableDescription", { engine: detail.session.engine_id })}
      />
    );
  }
  if (loading && !current) {
    return <EmptyState icon={<RefreshCw size={18} />} title={t("inspector.loading")} />;
  }
  if (error && !current) {
    return (
      <Notice
        actions={<Button onClick={() => void fetchSnapshot(rootNativeSessionId, true)}>{t("common.refresh")}</Button>}
        description={error}
        heading={t("inspector.failure")}
        tone="danger"
      />
    );
  }
  if (!current) return <EmptyState title={t("inspector.noRecords")} />;

  const llm = selection?.kind === "llm"
    ? current.run.turns[selection.turnIndex]?.llm_call
    : null;
  const tool = selection?.kind === "tool"
    ? current.run.turns[selection.turnIndex]?.tool_calls[selection.toolIndex]
    : null;
  const turnHooks = selection?.kind === "turnHooks"
    ? current.run.turns[selection.turnIndex]?.llm_call.middleware_hooks
    : null;
  const navigationDepth = path.length + (selection ? 1 : 0);
  // R33: friendly identity breadcrumbs — engine + role instead of raw
  // truncated session ids; the short id stays as a secondary hint.
  const breadcrumb = path.map((sessionId, index) => {
    const snapshot = snapshots[sessionId];
    const engine = snapshot?.engine_id ?? "?";
    const label = index === 0
      ? `${engine} · ${t("inspector.rootSession")}`
      : `${engine} · ${t("inspector.childSession")}`;
    return index === 0 ? label : `${label} (${shortId(sessionId)})`;
  }).join(" / ");

  return (
    <section
      aria-label={t("inspector.nativeTitle")}
      className="hub-engine-inspector"
      onKeyDown={(event) => {
        if (event.key === "Escape" && navigationDepth > 1) {
          event.stopPropagation();
          goBack();
        }
      }}
      tabIndex={-1}
    >
      <header className="hub-engine-inspector__header">
        <div className="hub-engine-inspector__navigation">
          <Button
            aria-label={t("common.back")}
            disabled={navigationDepth <= 1}
            leadingIcon={<ArrowLeft aria-hidden="true" size={14} />}
            onClick={goBack}
            size="small"
            variant="ghost"
          >
            {t("common.back")}
          </Button>
          <span>{breadcrumb}{selection ? " / …" : ""}</span>
        </div>
        <Button
          aria-label={t("common.refresh")}
          leadingIcon={<RefreshCw aria-hidden="true" size={14} />}
          loading={loading}
          onClick={() => void fetchSnapshot(current.native_session_id, true)}
          size="small"
          variant="ghost"
        >
          {t("common.refresh")}
        </Button>
      </header>

      <div className="hub-engine-inspector__body" ref={scrollerRef}>
      {selection?.kind === "llm" && llm ? <LlmDetail llm={llm} /> : null}
      {selection?.kind === "tool" && tool ? <ToolDetail tool={tool} /> : null}
      {selection?.kind === "hook" ? <HookDetail hook={selection.hook} title={selection.title} /> : null}
      {selection?.kind === "turnHooks" && turnHooks ? (
        <HookChain hooks={turnHooks} />
      ) : null}

      {!selection ? (
        <div className="hub-engine-inspector__session">
          <PanelSection
            actions={<Badge tone={current.live ? "success" : "neutral"}>{current.live ? t("inspector.live") : t("inspector.historical")}</Badge>}
            description={`${current.native_session_id} · ${t("inspector.branch")} ${shortId(current.branch_id)}`}
            heading={`${current.engine_id.toUpperCase()} · ${t("inspector.nativeTitle")}`}
            meta={t("inspector.recordCount", { count: current.records.length })}
          >
            <div className="hub-engine-inspector__metrics">
              <span><small>{t("inspector.duration")}</small><strong>{current.run.total_duration_ms}ms</strong></span>
              <span><small>{t("inspector.tokens")}</small><strong>{tokensOrUnknown(current.run.total_input_tokens, current.run.total_output_tokens)}</strong></span>
              <span><small>{t("inspector.turns")}</small><strong>{current.run.turns.length}</strong></span>
              <span><small>{t("inspector.children")}</small><strong>{current.children.length}</strong></span>
            </div>
          </PanelSection>

          {current.children.length > 0 ? (
            <PanelSection heading={t("inspector.children")} meta={`${current.children.length}`}>
              {current.children.map((child) => (
                <ListRow
                  actions={(
                    <Button
                      data-inspector-origin={`child:${child.child_session_id}`}
                      onClick={() => void openChild(child.child_session_id, `child:${child.child_session_id}`)}
                      size="small"
                      variant="ghost"
                    >
                      {t("common.open")}
                    </Button>
                  )}
                  description={`${t("inspector.spawnCall")} ${child.spawn_tool_call_id}`}
                  heading={`${t("inspector.childAgent")} ${shortId(child.child_session_id)}`}
                  key={child.child_session_id}
                  leading={<Bot aria-hidden="true" size={15} />}
                />
              ))}
            </PanelSection>
          ) : null}

          <HookRows
            hooks={current.run.middleware_hooks}
            onOpen={(hook, originKey) => openDetail({ kind: "hook", hook, title: hook.middleware_name }, originKey)}
          />

          <PanelSection heading={t("inspector.turns")} meta={`${current.run.turns.length}`}>
            {current.run.turns.length === 0 ? (
              <EmptyState title={t("inspector.noTurns")} />
            ) : current.run.turns.map((turn, turnIndex) => (
              <Disclosure
                defaultOpen={turnIndex === current.run.turns.length - 1}
                description={`${turn.duration_ms}ms · ${turn.llm_call.stop_reason}`}
                key={turn.turn_number}
                meta={`${turn.tool_calls.length}`}
                title={`${t("inspector.turn")} ${turn.turn_number}`}
              >
                <ListRow
                  actions={(
                    <Button
                      data-inspector-origin={`llm:${turnIndex}`}
                      onClick={() => openDetail({ kind: "llm", turnIndex }, `llm:${turnIndex}`)}
                      size="small"
                      variant="ghost"
                    >
                      {t("common.open")}
                    </Button>
                  )}
                  description={`${turn.llm_call.input_tokens}/${turn.llm_call.output_tokens} ${t("inspector.tokens")}`}
                  heading={t("inspector.llmCall")}
                  key={`${turn.turn_number}-llm`}
                  leading={<BrainCircuit aria-hidden="true" size={15} />}
                  meta={`${turn.llm_call.duration_ms}ms`}
                  status={turn.llm_call.error_message
                    ? <Badge tone="danger">{t("inspector.failed")}</Badge>
                    : turn.llm_call.stop_reason === "cancelled"
                      ? <Badge tone="warning">{t("inspector.cancelled")}</Badge>
                      : null}
                />
                {turn.tool_calls.map((toolCall, toolIndex) => (
                  <ListRow
                    actions={(
                      <Button
                        data-inspector-origin={`tool:${turnIndex}:${toolIndex}`}
                        onClick={() => openDetail({ kind: "tool", turnIndex, toolIndex }, `tool:${turnIndex}:${toolIndex}`)}
                        size="small"
                        variant="ghost"
                      >
                        {t("common.open")}
                      </Button>
                    )}
                    description={toolCall.tool_call.id}
                    heading={toolCall.tool_call.name}
                    key={toolCall.tool_call.id}
                    leading={<Wrench aria-hidden="true" size={15} />}
                    meta={`${toolCall.duration_ms}ms`}
                    status={toolCall.is_error ? <Badge tone="danger">{t("inspector.failed")}</Badge> : null}
                  />
                ))}
                {turn.llm_call.middleware_hooks.length > 0 ? (
                  <ListRow
                    actions={(
                      <Button
                        data-inspector-origin={`hooks:${turnIndex}`}
                        onClick={() => openDetail({ kind: "turnHooks", turnIndex }, `hooks:${turnIndex}`)}
                        size="small"
                        variant="ghost"
                      >
                        {t("common.open")}
                      </Button>
                    )}
                    description={t("inspector.inclusiveHint")}
                    heading={t("inspector.turnHooks", { count: turn.llm_call.middleware_hooks.length })}
                    key={`${turn.turn_number}-hooks`}
                    leading={<Timer aria-hidden="true" size={15} />}
                    meta={hookFailures(turn.llm_call.middleware_hooks) > 0
                      ? `${turn.llm_call.middleware_hooks.length} · ${hookFailures(turn.llm_call.middleware_hooks)} ✕`
                      : `${turn.llm_call.middleware_hooks.length}`}
                  />
                ) : null}
              </Disclosure>
            ))}
          </PanelSection>

          <Disclosure title={t("inspector.rawRecords")} meta={`${current.records.length}`}>
            <div className="hub-engine-inspector__records">
              {current.records.map((record) => (
                <Disclosure
                  description={`${new Date(record.timestamp).toLocaleTimeString()} · ${record.category}`}
                  key={record.seq}
                  meta={`#${record.seq}`}
                  title={record.type_name}
                >
                  <pre className="hub-engine-inspector__json">{pretty(record)}</pre>
                </Disclosure>
              ))}
            </div>
          </Disclosure>
        </div>
      ) : null}
      </div>
    </section>
  );
}
