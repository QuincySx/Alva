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
  | { kind: "hook"; hook: InspectorHookRecord; title: string };

export interface EngineInspectorPanelProps {
  detail: HubSessionDetail | null;
  loadInspection?: InspectorLoader;
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

function HookRows({
  hooks,
  onOpen,
}: {
  hooks: InspectorHookRecord[];
  onOpen: (hook: InspectorHookRecord) => void;
}) {
  const { t } = useI18n();
  if (hooks.length === 0) return null;
  return (
    <PanelSection heading={t("inspector.middleware")} meta={`${hooks.length}`}>
      {hooks.map((hook, index) => (
        <ListRow
          actions={(
            <Button onClick={() => onOpen(hook)} size="small" variant="ghost">
              {t("common.open")}
            </Button>
          )}
          description={`${hook.hook} · ${hook.outcome}`}
          heading={hook.middleware_name}
          key={`${hook.middleware_name}-${hook.hook}-${index}`}
          meta={`${hook.duration_ms}ms`}
        />
      ))}
    </PanelSection>
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

  const goBack = () => {
    if (selection) {
      setSelection(null);
      return;
    }
    if (path.length > 1) setPath((currentPath) => currentPath.slice(0, -1));
  };

  const openChild = async (nativeSessionId: string) => {
    const snapshot = await fetchSnapshot(nativeSessionId, true);
    if (!snapshot) return;
    setSelection(null);
    setPath((currentPath) => [...currentPath, snapshot.native_session_id]);
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
  const navigationDepth = path.length + (selection ? 1 : 0);

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
          <span>{path.map(shortId).join(" / ")}{selection ? " / …" : ""}</span>
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

      {selection?.kind === "llm" && llm ? <LlmDetail llm={llm} /> : null}
      {selection?.kind === "tool" && tool ? <ToolDetail tool={tool} /> : null}
      {selection?.kind === "hook" ? <HookDetail hook={selection.hook} title={selection.title} /> : null}

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
              <span><small>{t("inspector.tokens")}</small><strong>{current.run.total_input_tokens}/{current.run.total_output_tokens}</strong></span>
              <span><small>{t("inspector.turns")}</small><strong>{current.run.turns.length}</strong></span>
              <span><small>{t("inspector.children")}</small><strong>{current.children.length}</strong></span>
            </div>
          </PanelSection>

          {current.children.length > 0 ? (
            <PanelSection heading={t("inspector.children")} meta={`${current.children.length}`}>
              {current.children.map((child) => (
                <ListRow
                  actions={<Button onClick={() => void openChild(child.child_session_id)} size="small" variant="ghost">{t("common.open")}</Button>}
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
            onOpen={(hook) => setSelection({ kind: "hook", hook, title: hook.middleware_name })}
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
                  actions={<Button onClick={() => setSelection({ kind: "llm", turnIndex })} size="small" variant="ghost">{t("common.open")}</Button>}
                  description={`${turn.llm_call.input_tokens}/${turn.llm_call.output_tokens} ${t("inspector.tokens")}`}
                  heading={t("inspector.llmCall")}
                  leading={<BrainCircuit aria-hidden="true" size={15} />}
                  meta={`${turn.llm_call.duration_ms}ms`}
                />
                {turn.tool_calls.map((toolCall, toolIndex) => (
                  <ListRow
                    actions={<Button onClick={() => setSelection({ kind: "tool", turnIndex, toolIndex })} size="small" variant="ghost">{t("common.open")}</Button>}
                    description={toolCall.tool_call.id}
                    heading={toolCall.tool_call.name}
                    key={toolCall.tool_call.id}
                    leading={<Wrench aria-hidden="true" size={15} />}
                    meta={`${toolCall.duration_ms}ms`}
                  />
                ))}
                {turn.llm_call.middleware_hooks.map((hook, hookIndex) => (
                  <ListRow
                    actions={<Button onClick={() => setSelection({ kind: "hook", hook, title: hook.middleware_name })} size="small" variant="ghost">{t("common.open")}</Button>}
                    description={hook.hook}
                    heading={hook.middleware_name}
                    key={`${hook.middleware_name}-${hook.hook}-${hookIndex}`}
                    leading={<Timer aria-hidden="true" size={15} />}
                    meta={`${hook.duration_ms}ms`}
                  />
                ))}
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
    </section>
  );
}
