import {
  Check,
  ChevronRight,
  Plug,
  RefreshCw,
  Settings as SettingsIcon,
  Sparkles,
  Wrench,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  listPlugins,
  listMcpServers,
  type McpServerInfo,
} from "../agent-bridge";

interface ToolEntry {
  name: string;
  description: string;
  category: string;
}
import { useAppStore } from "../store/appStore";

// Map plugin names → Chinese labels for the UI.
const GROUP_LABELS: Record<string, string> = {
  core: "文件读写",
  shell: "命令执行",
  interaction: "人工交互",
  task: "任务管理",
  team: "团队协作",
  planning: "计划与规划",
  utility: "通用工具",
  web: "网络与搜索",
  browser: "浏览器自动化",
  skills: "技能系统",
  mcp: "MCP 集成",
  "sub-agents": "子 Agent",
};

function groupLabel(category: string): string {
  return GROUP_LABELS[category] ?? category;
}

export function ToolPicker() {
  const [open, setOpen] = useState(false);
  const [tools, setTools] = useState<ToolEntry[]>([]);
  // Derive tool entries from plugins — flatten each plugin's tools list
  // and tag them with the plugin name as category.
  const [mcpServers, setMcpServers] = useState<McpServerInfo[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loadingMcp, setLoadingMcp] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const auto = useAppStore((s) => s.toolsAutoMode);
  const selected = useAppStore((s) => s.selectedTools);
  const setAuto = useAppStore((s) => s.setToolsAutoMode);
  const setSelected = useAppStore((s) => s.setSelectedTools);

  // Close on click outside
  useEffect(() => {
    if (!open) return;
    const onDocDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onDocDown);
    return () => window.removeEventListener("mousedown", onDocDown);
  }, [open]);

  // Lazy-load data on first open
  useEffect(() => {
    if (!open || loaded) return;
    (async () => {
      try {
        const [plugins, m] = await Promise.all([
          listPlugins(),
          listMcpServers(),
        ]);
        const entries: ToolEntry[] = [];
        for (const p of plugins) {
          for (const t of p.tools) {
            entries.push({ name: t.name, description: t.description, category: p.name });
          }
        }
        setTools(entries);
        setMcpServers(m);
      } catch (e) {
        console.error("load tools/mcp failed", e);
      } finally {
        setLoaded(true);
      }
    })();
  }, [open, loaded]);

  const groupedTools = useMemo(() => {
    const map = new Map<string, ToolEntry[]>();
    for (const t of tools) {
      const arr = map.get(t.category) ?? [];
      arr.push(t);
      map.set(t.category, arr);
    }
    return [...map.entries()];
  }, [tools]);

  const allToolNames = useMemo(() => tools.map((t) => t.name), [tools]);

  const isSelected = (name: string) => selected.includes(name);
  const setAll = () => {
    setAuto(false);
    setSelected(allToolNames);
  };
  const clearAll = () => {
    setAuto(false);
    setSelected([]);
  };
  const setToAuto = () => {
    setAuto(true);
  };

  const toggleTool = (name: string) => {
    // Any manual click leaves auto mode.
    if (auto) setAuto(false);
    if (selected.includes(name)) {
      setSelected(selected.filter((n) => n !== name));
    } else {
      setSelected([...selected, name]);
    }
  };

  const groupState = (group: ToolEntry[]): "all" | "some" | "none" => {
    const selectedCount = group.filter((t) => isSelected(t.name)).length;
    if (selectedCount === 0) return "none";
    if (selectedCount === group.length) return "all";
    return "some";
  };

  const toggleGroup = (group: ToolEntry[]) => {
    if (auto) setAuto(false);
    const state = groupState(group);
    const names = group.map((t) => t.name);
    if (state === "all") {
      setSelected(selected.filter((n) => !names.includes(n)));
    } else {
      const merged = Array.from(new Set([...selected, ...names]));
      setSelected(merged);
    }
  };

  const refreshMcp = async () => {
    setLoadingMcp(true);
    try {
      setMcpServers(await listMcpServers());
    } catch (e) {
      console.error("refresh mcp failed", e);
    } finally {
      setLoadingMcp(false);
    }
  };

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="relative flex items-center gap-1.5 rounded-md bg-neutral-800/50 hover:bg-neutral-800 px-2 py-1 text-xs"
      >
        <Wrench size={12} />
        工具
        {auto ? (
          // Auto mode → tiny sparkles corner badge (no Chinese text).
          <span className="absolute -top-1 -right-1 flex items-center justify-center w-3.5 h-3.5 rounded-full bg-blue-600 text-white shadow ring-1 ring-neutral-950">
            <Sparkles size={8} />
          </span>
        ) : selected.length > 0 ? (
          <span className="ml-1 rounded bg-blue-700 text-white px-1.5 text-[10px]">
            {selected.length}
          </span>
        ) : (
          <span className="ml-1 rounded bg-neutral-700 text-neutral-300 px-1.5 text-[10px]">
            0
          </span>
        )}
      </button>

      {open && (
        <div className="absolute bottom-full left-0 mb-2 w-[440px] max-h-[520px] rounded-lg border border-neutral-800 bg-neutral-950 shadow-2xl overflow-hidden flex flex-col">
          {/* Header */}
          <div className="flex items-center gap-2 border-b border-neutral-800 px-3 py-2">
            <Wrench size={13} className="text-neutral-400" />
            <span className="text-xs font-semibold">工具</span>
            <span className="flex-1" />
            <button
              type="button"
              onClick={() => setOpen(false)}
              className="text-neutral-500 hover:text-white"
            >
              <X size={12} />
            </button>
          </div>

          {/* Mode section */}
          <div className="px-3 py-3 border-b border-neutral-800 space-y-2">
            <div className="text-[11px] text-neutral-400">
              允许 Alva 在下一次回复时使用所选工具。
            </div>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={setToAuto}
                className={`inline-flex items-center gap-1 rounded px-2.5 py-1 text-[11px] border ${
                  auto
                    ? "bg-blue-950/60 border-blue-800 text-blue-200"
                    : "bg-neutral-900 border-neutral-800 text-neutral-400 hover:border-neutral-700"
                }`}
              >
                <Sparkles size={11} />
                自动
              </button>
              <button
                type="button"
                onClick={setAll}
                className="rounded bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-2.5 py-1 text-[11px] text-neutral-300"
              >
                全选
              </button>
              <button
                type="button"
                onClick={clearAll}
                className="rounded bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-2.5 py-1 text-[11px] text-neutral-300"
              >
                全不选
              </button>
              {!auto && (
                <span className="text-[10px] text-neutral-500 ml-1">
                  已选 {selected.length}/{allToolNames.length}
                </span>
              )}
            </div>
            <div className="text-[10px] text-neutral-500 leading-relaxed">
              {auto
                ? "让 Alva 根据你的消息自动选择最相关的工具。"
                : "只暴露你勾选的工具给这次回复。"}
            </div>
          </div>

          {/* Scrollable body */}
          <div className="flex-1 overflow-auto">
            {!loaded && (
              <div className="px-3 py-4 text-xs text-neutral-500">加载中…</div>
            )}

            {loaded && groupedTools.length === 0 && (
              <div className="px-3 py-4 text-xs text-neutral-500">
                没发现内置工具
              </div>
            )}

            {loaded &&
              groupedTools.map(([category, groupTools]) => (
                <ToolGroup
                  key={category}
                  category={category}
                  tools={groupTools}
                  state={groupState(groupTools)}
                  onToggleGroup={() => toggleGroup(groupTools)}
                  onToggleTool={toggleTool}
                  isSelected={isSelected}
                  disabled={auto}
                />
              ))}

            {/* MCP section */}
            {loaded && (
              <McpSection
                servers={mcpServers}
                loading={loadingMcp}
                onRefresh={refreshMcp}
              />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tool group with master checkbox
// ---------------------------------------------------------------------------

function ToolGroup({
  category,
  tools,
  state,
  onToggleGroup,
  onToggleTool,
  isSelected,
  disabled,
}: {
  category: string;
  tools: ToolEntry[];
  state: "all" | "some" | "none";
  onToggleGroup: () => void;
  onToggleTool: (name: string) => void;
  isSelected: (name: string) => boolean;
  disabled: boolean;
}) {
  const [expanded, setExpanded] = useState(true);

  return (
    <div className="border-b border-neutral-900">
      <div
        className={`flex items-center gap-2 px-3 py-2 ${
          disabled ? "opacity-50" : ""
        }`}
      >
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="text-neutral-500 hover:text-white"
        >
          <ChevronRight
            size={12}
            className={`transition-transform ${expanded ? "rotate-90" : ""}`}
          />
        </button>
        <span className="text-[11px] text-neutral-300 font-medium flex-1">
          {groupLabel(category)}
        </span>
        <span className="text-[10px] text-neutral-600 font-mono">
          {tools.length}
        </span>
        <GroupCheckbox
          state={state}
          onClick={onToggleGroup}
          disabled={disabled}
        />
      </div>
      {expanded && (
        <div className="pb-1">
          {tools.map((t) => (
            <ToolRow
              key={t.name}
              tool={t}
              checked={isSelected(t.name)}
              onToggle={() => onToggleTool(t.name)}
              disabled={disabled}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function GroupCheckbox({
  state,
  onClick,
  disabled,
}: {
  state: "all" | "some" | "none";
  onClick: () => void;
  disabled: boolean;
}) {
  const classes =
    state === "all"
      ? "bg-blue-600 border-blue-600"
      : state === "some"
        ? "bg-blue-900 border-blue-700"
        : "border-neutral-600 bg-transparent";
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        if (!disabled) onClick();
      }}
      disabled={disabled}
      className={`w-3.5 h-3.5 rounded-[3px] border flex items-center justify-center transition-colors ${classes}`}
    >
      {state === "all" && (
        <Check size={9} strokeWidth={3.5} className="text-white" />
      )}
      {state === "some" && (
        <span className="block w-1.5 h-[1px] bg-white" />
      )}
    </button>
  );
}

function ToolRow({
  tool,
  checked,
  onToggle,
  disabled,
}: {
  tool: ToolEntry;
  checked: boolean;
  onToggle: () => void;
  disabled: boolean;
}) {
  return (
    <button
      type="button"
      onClick={() => !disabled && onToggle()}
      disabled={disabled}
      className={`w-full text-left pl-8 pr-3 py-1.5 flex items-start gap-2 ${
        disabled ? "opacity-60" : "hover:bg-neutral-900/60"
      }`}
    >
      <span
        className={`mt-0.5 shrink-0 w-3.5 h-3.5 rounded-[3px] border flex items-center justify-center ${
          checked
            ? "bg-blue-600 border-blue-600"
            : "border-neutral-600"
        }`}
      >
        {checked && (
          <Check size={9} strokeWidth={3.5} className="text-white" />
        )}
      </span>
      <span className="flex-1 min-w-0">
        <span className="block text-[11px] font-medium truncate">
          {tool.name}
        </span>
        <span className="block text-[10px] text-neutral-500 line-clamp-2">
          {tool.description}
        </span>
      </span>
    </button>
  );
}

// ---------------------------------------------------------------------------
// MCP section
// ---------------------------------------------------------------------------

function McpSection({
  servers,
  loading,
  onRefresh,
}: {
  servers: McpServerInfo[];
  loading: boolean;
  onRefresh: () => void;
}) {
  return (
    <div className="border-t border-neutral-900">
      <div className="flex items-center gap-2 px-3 py-2">
        <Plug size={12} className="text-neutral-400" />
        <span className="text-[11px] text-neutral-300 font-medium flex-1">
          MCP 服务器
        </span>
        <span className="text-[10px] text-neutral-600 font-mono">
          {servers.length}
        </span>
        <button
          type="button"
          onClick={onRefresh}
          title="刷新"
          className="text-neutral-500 hover:text-white"
          disabled={loading}
        >
          <RefreshCw size={11} className={loading ? "animate-spin" : ""} />
        </button>
        <McpSettingsButton />
      </div>
      <div className="pb-2">
        {servers.length === 0 ? (
          <div className="px-3 pb-2 text-[10px] text-neutral-500 leading-relaxed">
            还没配置 MCP 服务器。在{" "}
            <code className="text-neutral-400">~/.alva/mcp.json</code> 里添加后刷新。
          </div>
        ) : (
          servers.map((s) => (
            <div
              key={s.id}
              className="pl-8 pr-3 py-1 flex items-center gap-2 text-[11px]"
            >
              <Plug size={10} className="text-neutral-500 shrink-0" />
              <span className="flex-1 min-w-0 truncate">{s.name}</span>
              <span className="text-[9px] font-mono text-neutral-600">
                {s.kind}
              </span>
              <span
                className={`rounded-full px-1.5 py-0.5 text-[9px] border ${
                  s.enabled
                    ? "bg-green-950/50 border-green-900/60 text-green-400"
                    : "bg-neutral-900 border-neutral-800 text-neutral-500"
                }`}
              >
                {s.enabled ? "启用" : "禁用"}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function McpSettingsButton() {
  // Navigating routes from inside a popover would need the route setter
  // from App.tsx — out of scope for now. Keep as a hint-only icon button.
  return (
    <span
      title="到 MCP 页管理(左栏 nav)"
      className="text-neutral-500 inline-flex items-center"
    >
      <SettingsIcon size={11} />
    </span>
  );
}

