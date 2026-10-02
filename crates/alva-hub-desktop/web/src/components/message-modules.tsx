// INPUT: Versioned alva.display JSON data and trusted renderer registrations
// OUTPUT: MessageModules, createMessageModuleRegistry and built-in table/chart/diagram renderers
// POS: Hub-only presentation extension; messages never supply executable code.
import { Component, createContext, useContext, type ReactNode } from "react";
import { useI18n } from "../i18n";

export interface MessageModuleRenderer {
  type: string;
  version: number;
  validate: (data: unknown) => boolean;
  render: (data: unknown) => ReactNode;
}

/** Registered by trusted application composition, never loaded from a message. */
export function createMessageModuleRegistry(renderers: readonly MessageModuleRenderer[]) {
  const registry = new Map<string, MessageModuleRenderer>();
  for (const renderer of renderers) {
    const key = `${renderer.type}@${renderer.version}`;
    if (registry.has(key)) throw new Error(`Duplicate message renderer: ${key}`);
    registry.set(key, renderer);
  }
  return registry as ReadonlyMap<string, MessageModuleRenderer>;
}

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const text = (value: unknown): value is string => typeof value === "string" && value.length <= 500;
const cell = (value: unknown) => text(value) || typeof value === "boolean"
  || (typeof value === "number" && Number.isFinite(value)) || value === null;
type Table = { title: string; columns: string[]; rows: (string | number | boolean | null)[][] };
type Chart = { title: string; style: "bar" | "line"; points: { label: string; value: number }[] };
type Diagram = { title: string; nodes: { id: string; label: string }[]; edges: { from: string; to: string; label?: string }[] };

export const builtinMessageModuleRenderers: readonly MessageModuleRenderer[] = [
  {
    type: "alva.table", version: 1,
    validate: (data) => object(data) && text(data.title) && Array.isArray(data.columns)
      && data.columns.length > 0 && data.columns.length <= 12 && data.columns.every(text)
      && Array.isArray(data.rows) && data.rows.length <= 100 && data.rows.every((row) =>
        Array.isArray(row) && row.length === (data.columns as unknown[]).length && row.every(cell)),
    render: (data) => {
      const table = data as Table;
      return <table className="chat-table"><caption>{table.title}</caption><thead><tr>
        {table.columns.map((column, i) => <th scope="col" key={i}>{column}</th>)}
      </tr></thead><tbody>{table.rows.map((row, i) => <tr key={i}>
        {row.map((value, j) => <td key={j}>{String(value ?? "")}</td>)}
      </tr>)}</tbody></table>;
    },
  },
  {
    type: "alva.chart", version: 1,
    validate: (data) => object(data) && text(data.title) && ["bar", "line"].includes(String(data.style))
      && Array.isArray(data.points) && data.points.length > 0 && data.points.length <= 100
      && data.points.every((point) => object(point) && text(point.label)
        && typeof point.value === "number" && Number.isFinite(point.value) && Math.abs(point.value) <= 1e12),
    render: (data) => {
      const chart = data as Chart;
      const values = chart.points.map((point) => point.value);
      const min = Math.min(0, ...values);
      const span = Math.max(0, ...values) - min || 1;
      const y = (value: number) => 180 - (value - min) / span * 160;
      const x = (i: number) => 20 + (i + 0.5) / values.length * 460;
      return <figure className="message-module"><figcaption>{chart.title}</figcaption>
        <svg viewBox="0 0 500 200" role="img" aria-label={chart.title}>
          <line x1="20" y1={y(0)} x2="480" y2={y(0)} stroke="currentColor" />
          {chart.style === "line" ? <polyline points={values.map((value, i) => `${x(i)},${y(value)}`).join(" ")}
            fill="none" stroke="var(--color-focus)" strokeWidth="2" />
            : values.map((value, i) => <rect key={i} x={x(i) - 170 / values.length}
              y={Math.min(y(value), y(0))} width={340 / values.length}
              height={Math.max(1, Math.abs(y(value) - y(0)))} fill="var(--color-focus)"><title>{chart.points[i].label}: {value}</title></rect>)}
        </svg>
        <table className="chat-table"><tbody>{chart.points.map((point, i) => <tr key={i}>
          <th scope="row">{point.label}</th><td>{point.value}</td></tr>)}</tbody></table>
      </figure>;
    },
  },
  {
    type: "alva.diagram", version: 1,
    validate: (data) => {
      if (!object(data) || !text(data.title) || !Array.isArray(data.nodes) || !Array.isArray(data.edges)
        || data.nodes.length === 0 || data.nodes.length > 30 || data.edges.length > 60) return false;
      if (!data.nodes.every((node) => object(node) && text(node.id) && node.id.length > 0 && text(node.label))) return false;
      const ids = data.nodes.map((node) => (node as { id: string }).id);
      return new Set(ids).size === ids.length && data.edges.every((edge) => object(edge)
        && text(edge.from) && text(edge.to) && ids.includes(edge.from) && ids.includes(edge.to)
        && (edge.label === undefined || text(edge.label)));
    },
    render: (data) => {
      const diagram = data as Diagram;
      const positions = new Map(diagram.nodes.map((node, i) => [node.id, { x: 90 + i % 3 * 180, y: 40 + Math.floor(i / 3) * 100 }]));
      return <figure className="message-module"><figcaption>{diagram.title}</figcaption>
        <svg viewBox={`0 0 540 ${Math.ceil(diagram.nodes.length / 3) * 100}`} role="img" aria-label={diagram.title}>
          {diagram.edges.map((edge, i) => { const a = positions.get(edge.from)!; const b = positions.get(edge.to)!;
            return <g key={i}><line x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="currentColor" />
              <text x={(a.x + b.x) / 2} y={(a.y + b.y) / 2 - 8} textAnchor="middle">{edge.label ?? "→"}</text></g>; })}
          {diagram.nodes.map((node) => { const p = positions.get(node.id)!; return <g key={node.id}>
            <rect x={p.x - 75} y={p.y - 20} width="150" height="40" rx="4" fill="var(--color-surface)" stroke="currentColor" />
            <text x={p.x} y={p.y + 5} textAnchor="middle">{node.label}</text></g>; })}
        </svg>
        <ul>{diagram.nodes.map((node) => <li key={node.id}>{node.label}</li>)}</ul>
        <ul>{diagram.edges.map((edge, i) => <li key={i}>{diagram.nodes.find((node) => node.id === edge.from)?.label}
          {" → "}{diagram.nodes.find((node) => node.id === edge.to)?.label}{edge.label ? `: ${edge.label}` : ""}</li>)}</ul>
      </figure>;
    },
  },
];

export const defaultMessageModuleRegistry = createMessageModuleRegistry(builtinMessageModuleRenderers);
const MessageModuleContext = createContext(defaultMessageModuleRegistry);

export function MessageModuleProvider({ registry, children }: {
  registry: ReadonlyMap<string, MessageModuleRenderer>; children: ReactNode;
}) {
  return <MessageModuleContext.Provider value={registry}>{children}</MessageModuleContext.Provider>;
}

class RendererBoundary extends Component<{ children: ReactNode; fallback: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() { return this.state.failed ? this.props.fallback : this.props.children; }
}
function RegisteredModule({ renderer, data }: { renderer: MessageModuleRenderer; data: unknown }) {
  return renderer.render(data);
}

/** Returns null for ordinary/partial JSON, leaving the canonical text visible. */
export function MessageModules({ source, registry: suppliedRegistry }: {
  source: string;
  registry?: ReadonlyMap<string, MessageModuleRenderer>;
}) {
  const { t } = useI18n();
  const contextRegistry = useContext(MessageModuleContext);
  const registry = suppliedRegistry ?? contextRegistry;
  if (source.length > 16_384) return null;
  let envelope: unknown;
  try { envelope = JSON.parse(source); } catch { return null; }
  // Existing engine ToolResult transport wraps text blocks; do not invent a
  // new runtime event or rely on the preview's shape.
  if (object(envelope) && envelope.schema === undefined && Array.isArray(envelope.content)
    && envelope.content.length <= 8) {
    const body = envelope.content.filter((block) => object(block) && block.type === "text" && typeof block.text === "string")
      .map((block) => (block as { text: string }).text).join("");
    try { envelope = JSON.parse(body); } catch { return null; }
  }
  if (!object(envelope) || envelope.schema !== "alva.display" || envelope.version !== 1
    || !Array.isArray(envelope.modules) || envelope.modules.length === 0 || envelope.modules.length > 8) return null;
  return <div className="message-modules" aria-label={t("chat.modules")}>
    {envelope.modules.map((module, index) => {
      const fallback = <div role="status"><p>{t("chat.moduleUnavailable")}</p><pre>{JSON.stringify(module, null, 2)}</pre></div>;
      if (!object(module) || typeof module.type !== "string" || !Number.isSafeInteger(module.version)) return <div key={index}>{fallback}</div>;
      const renderer = registry.get(`${module.type}@${module.version}`);
      let valid = false;
      try { valid = renderer?.validate(module.data) ?? false; } catch { /* fail to inert source */ }
      if (!renderer || !valid) return <div key={index}>{fallback}</div>;
      return <RendererBoundary key={`${index}:${source}`} fallback={fallback}>
        <RegisteredModule renderer={renderer} data={module.data} />
      </RendererBoundary>;
    })}
  </div>;
}
