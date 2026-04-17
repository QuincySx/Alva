import { Boxes, ChevronDown, ChevronRight, Puzzle, Search, Sparkles, Wrench } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  listAllSkills,
  listPlugins,
  listSkillSources,
  setPluginEnabled,
  type PluginInfo,
  type SkillInfo,
  type SkillSourceInfo,
} from "../agent-bridge";

type Tab = "plugins" | "skills";

const TABS: { id: Tab; label: string; icon: ReactNode }[] = [
  { id: "plugins", label: "插件", icon: <Puzzle size={14} /> },
  { id: "skills", label: "技能", icon: <Sparkles size={14} /> },
];

export default function Skills() {
  const [tab, setTab] = useState<Tab>("plugins");
  const [query, setQuery] = useState("");

  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [sources, setSources] = useState<SkillSourceInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const reload = async () => {
    setLoading(true);
    try {
      const [p, s, src] = await Promise.all([
        listPlugins(),
        listAllSkills(),
        listSkillSources(),
      ]);
      setPlugins(p);
      setSkills(s);
      setSources(src);
    } catch (e) {
      console.error("load capabilities failed", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    reload();
  }, []);

  useEffect(() => {
    setQuery("");
  }, [tab]);

  const filteredPlugins = useMemo(
    () =>
      filterByQuery(plugins, query, (p) => [
        p.name,
        p.description,
        p.category,
        ...p.tools.map((t) => t.name),
      ]),
    [plugins, query],
  );
  const filteredSkills = useMemo(
    () => filterByQuery(skills, query, (s) => [s.name, s.description, s.kind]),
    [skills, query],
  );

  const counts = {
    plugins: plugins.length,
    skills: skills.length,
  };

  return (
    <div className="flex h-full flex-col bg-neutral-950 text-neutral-100">
      <header className="flex items-center gap-3 border-b border-neutral-900 px-6 py-4">
        <Boxes size={18} className="text-blue-400" />
        <div className="text-lg font-semibold">能力</div>
        <span className="flex-1" />
        <div className="flex items-center gap-2 rounded-md bg-neutral-900 border border-neutral-800 px-3 py-1.5 w-72">
          <Search size={14} className="text-neutral-500" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索"
            className="flex-1 bg-transparent outline-none text-sm"
          />
        </div>
        <button
          type="button"
          onClick={reload}
          className="rounded bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-3 py-1.5 text-xs"
        >
          刷新
        </button>
      </header>

      {/* Tab strip */}
      <div className="flex border-b border-neutral-900 px-4 shrink-0">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`flex items-center gap-1.5 px-4 py-2.5 text-sm transition-colors ${
              tab === t.id
                ? "text-white border-b-2 border-blue-600"
                : "text-neutral-400 hover:text-white border-b-2 border-transparent"
            }`}
          >
            {t.icon}
            {t.label}
            <span className="text-[10px] text-neutral-500 font-mono">
              ({counts[t.id]})
            </span>
          </button>
        ))}
      </div>

      <main className="flex-1 overflow-auto px-6 py-4">
        {loading && <div className="text-neutral-500 text-sm">加载中…</div>}

        {!loading && tab === "plugins" && (
          <PluginsList plugins={filteredPlugins} total={plugins.length} onToggle={async (name, enabled) => {
            await setPluginEnabled(name, enabled);
            const updated = await listPlugins();
            setPlugins(updated);
          }} />
        )}
        {!loading && tab === "skills" && (
          <SkillsList
            skills={filteredSkills}
            total={skills.length}
            sources={sources}
          />
        )}
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Search helper
// ---------------------------------------------------------------------------

function filterByQuery<T>(
  items: T[],
  query: string,
  getFields: (item: T) => string[],
): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return items;
  const tokens = q.split(/\s+/).filter(Boolean);
  return items.filter((item) => {
    const text = getFields(item).join(" ").toLowerCase();
    return tokens.every((tok) => text.includes(tok));
  });
}

// ---------------------------------------------------------------------------
// Plugins tab
// ---------------------------------------------------------------------------

function PluginsList({
  plugins,
  total,
  onToggle,
}: {
  plugins: PluginInfo[];
  total: number;
  onToggle: (name: string, enabled: boolean) => void;
}) {
  if (total === 0) {
    return <div className="text-neutral-500 text-sm">没发现插件</div>;
  }
  if (plugins.length === 0) {
    return <div className="text-neutral-500 text-sm">无匹配</div>;
  }

  const buckets: { id: string; label: string; items: PluginInfo[] }[] = [
    { id: "tools", label: "工具扩展", items: [] },
    { id: "system", label: "系统扩展", items: [] },
    { id: "middleware", label: "中间件", items: [] },
  ];
  const other: PluginInfo[] = [];
  for (const p of plugins) {
    const b = buckets.find((x) => x.id === p.category);
    if (b) b.items.push(p);
    else other.push(p);
  }

  return (
    <div className="space-y-6">
      {buckets
        .filter((b) => b.items.length > 0)
        .map((b) => (
          <section key={b.id}>
            <div className="text-xs text-neutral-500 uppercase tracking-wider mb-2 font-mono">
              {b.label} · {b.items.length}
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
              {b.items.map((p) => (
                <PluginCard key={p.name} plugin={p} onToggle={onToggle} />
              ))}
            </div>
          </section>
        ))}
      {other.length > 0 && (
        <section>
          <div className="text-xs text-neutral-500 uppercase tracking-wider mb-2 font-mono">
            其他 · {other.length}
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
            {other.map((p) => (
              <PluginCard key={p.name} plugin={p} onToggle={onToggle} />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function PluginCard({ plugin: p, onToggle }: { plugin: PluginInfo; onToggle: (name: string, enabled: boolean) => void }) {
  const [expanded, setExpanded] = useState(false);
  const hasTools = p.tools.length > 0;
  const isCore = p.name === "core" || p.name === "shell";

  return (
    <div className={`rounded-lg border bg-neutral-900/40 hover:bg-neutral-900 transition-colors p-4 flex flex-col gap-2 ${
      p.enabled ? "border-neutral-800" : "border-neutral-800/50 opacity-60"
    }`}>
      <div className="flex items-start gap-2">
        <div className="w-8 h-8 rounded bg-neutral-800 flex items-center justify-center shrink-0">
          <Puzzle size={14} className="text-neutral-400" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium truncate">{p.name}</div>
        </div>
        {isCore ? (
          <span className="rounded-full bg-blue-950/50 border border-blue-900/60 text-blue-400 text-[10px] px-1.5 py-0.5 shrink-0">
            核心
          </span>
        ) : (
          <button
            type="button"
            onClick={() => onToggle(p.name, !p.enabled)}
            className={`relative w-8 h-[18px] rounded-full transition-colors shrink-0 ${
              p.enabled ? "bg-green-600" : "bg-neutral-700"
            }`}
          >
            <span className={`absolute top-[2px] w-[14px] h-[14px] rounded-full bg-white transition-transform ${
              p.enabled ? "left-[16px]" : "left-[2px]"
            }`} />
          </button>
        )}
      </div>
      <div className="text-xs text-neutral-400 line-clamp-3">
        {p.description}
      </div>

      {hasTools && (
        <div className="mt-1">
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1 text-[11px] text-neutral-500 hover:text-neutral-300 transition-colors"
          >
            {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            {p.tools.length} 个工具
          </button>
          {expanded && (
            <div className="mt-1.5 space-y-1 pl-1">
              {p.tools.map((t) => (
                <div
                  key={t.name}
                  className="flex items-start gap-2 text-[11px]"
                >
                  <Wrench
                    size={10}
                    className="text-neutral-600 mt-0.5 shrink-0"
                  />
                  <span className="text-neutral-300 font-mono shrink-0">
                    {t.name}
                  </span>
                  <span className="text-neutral-600 truncate">
                    {t.description}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      <div className="mt-auto pt-2 border-t border-neutral-800">
        <span className="font-mono text-[10px] text-neutral-600">
          {p.category}
        </span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Skills tab
// ---------------------------------------------------------------------------

function SkillsList({
  skills,
  total,
  sources,
}: {
  skills: SkillInfo[];
  total: number;
  sources: SkillSourceInfo[];
}) {
  return (
    <div className="space-y-4">
      <div className="text-xs text-neutral-500">
        来源:{" "}
        {sources.length === 0
          ? "无"
          : sources.map((s, i) => (
              <span key={s.path} className="mr-3">
                {s.label}{" "}
                <span
                  className={s.exists ? "text-green-400" : "text-neutral-600"}
                >
                  {s.exists ? "\u2713" : "\u2014"}
                </span>
                {i < sources.length - 1 ? "  " : ""}
              </span>
            ))}
      </div>

      {total === 0 ? (
        <div className="text-neutral-500 text-sm">
          没发现任何技能。在{" "}
          <code className="text-neutral-400">~/.config/alva/skills</code> 或{" "}
          <code className="text-neutral-400">~/.claude/skills</code>{" "}
          下放技能包后刷新。
        </div>
      ) : skills.length === 0 ? (
        <div className="text-neutral-500 text-sm">无匹配</div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
          {skills.map((s) => (
            <Card
              key={`${s.source_dir}::${s.name}`}
              icon={<Sparkles size={14} className="text-neutral-400" />}
              title={s.name}
              description={s.description || "(没有描述)"}
              badge={
                s.enabled ? (
                  <span className="rounded-full bg-green-950/50 border border-green-900/60 text-green-400 text-[10px] px-1.5 py-0.5">
                    启用
                  </span>
                ) : null
              }
              footer={
                <>
                  <div className="font-mono text-[10px] text-neutral-500">
                    {s.kind}
                  </div>
                  <div className="font-mono text-[10px] text-neutral-600 truncate">
                    {s.source_dir}
                  </div>
                </>
              }
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Shared card (used by Skills tab)
// ---------------------------------------------------------------------------

function Card({
  icon,
  title,
  description,
  badge,
  footer,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  badge?: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div className="rounded-lg border border-neutral-800 bg-neutral-900/40 hover:bg-neutral-900 transition-colors p-4 flex flex-col gap-2">
      <div className="flex items-start gap-2">
        <div className="w-8 h-8 rounded bg-neutral-800 flex items-center justify-center shrink-0">
          {icon}
        </div>
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium truncate">{title}</div>
        </div>
        {badge}
      </div>
      <div className="text-xs text-neutral-400 line-clamp-3">{description}</div>
      {footer && (
        <div className="mt-auto pt-2 border-t border-neutral-800 space-y-0.5">
          {footer}
        </div>
      )}
    </div>
  );
}
