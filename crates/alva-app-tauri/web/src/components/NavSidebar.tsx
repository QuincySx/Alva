import {
  Bot,
  Clock,
  MessageSquare,
  PanelLeftClose,
  PanelLeftOpen,
  Plug,
  Plus,
  Search,
  Settings as SettingsIcon,
  Sparkles,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { createSession, listSessions, type SessionInfo } from "../agent-bridge";
import { useAppStore } from "../store/appStore";

export type RouteId =
  | "home"
  | "search"
  | "schedule"
  | "skills"
  | "mcp"
  | "agents";

interface NavItem {
  id: RouteId;
  label: string;
  icon: ReactNode;
}

const NAV_ITEMS: NavItem[] = [
  { id: "search", label: "搜索任务", icon: <Search size={16} /> },
  { id: "schedule", label: "定时任务", icon: <Clock size={16} /> },
  { id: "skills", label: "能力", icon: <Sparkles size={16} /> },
  { id: "mcp", label: "MCP", icon: <Plug size={16} /> },
  { id: "agents", label: "我的 Agent", icon: <Bot size={16} /> },
];

interface NavSidebarProps {
  current: RouteId;
  onNavigate: (id: RouteId) => void;
  onOpenSettings?: () => void;
  onCollapse?: () => void;
  collapsed?: boolean;
}

export function NavSidebar({
  current,
  onNavigate,
  onOpenSettings,
  onCollapse,
  collapsed = false,
}: NavSidebarProps) {
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const setActiveSessionId = useAppStore((s) => s.setActiveSessionId);
  const sessionListNonce = useAppStore((s) => s.sessionListNonce);
  const bumpSessionList = useAppStore((s) => s.bumpSessionList);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [creating, setCreating] = useState(false);

  const handleNewTask = async () => {
    if (creating) return;
    setCreating(true);
    try {
      const fresh = await createSession();
      setActiveSessionId(fresh.id);
      bumpSessionList();
      if (current !== "home") onNavigate("home");
    } catch (e) {
      console.error("create session failed", e);
    } finally {
      setCreating(false);
    }
  };

  // Re-fetch when explicitly nudged (new task, rename, AgentEnd, etc.)
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await listSessions();
        if (!cancelled) setSessions(list);
      } catch (e) {
        console.error("NavSidebar listSessions failed", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionListNonce]);

  const handleSelectSession = (id: string) => {
    setActiveSessionId(id);
    onNavigate("home");
  };

  return (
    <nav className="h-full flex flex-col bg-neutral-950 border-r border-neutral-900 text-neutral-200">
      {/* Header: brand + collapse toggle (expanded) or just the toggle (collapsed) */}
      <div
        className={`shrink-0 flex items-center border-b border-neutral-900 ${
          collapsed ? "justify-center py-3" : "justify-between px-3 py-3"
        }`}
      >
        {!collapsed && (
          <span className="text-sm font-semibold tracking-wide truncate">
            Alva
          </span>
        )}
        {onCollapse && (
          <button
            type="button"
            onClick={onCollapse}
            className="text-neutral-500 hover:text-white"
            title={collapsed ? "展开侧栏 (⌘B)" : "收起侧栏 (⌘B)"}
          >
            {collapsed ? <PanelLeftOpen size={14} /> : <PanelLeftClose size={14} />}
          </button>
        )}
      </div>

      {/* New task button */}
      <div className={`shrink-0 ${collapsed ? "px-1.5 py-2" : "px-3 py-2"}`}>
        <button
          type="button"
          onClick={handleNewTask}
          disabled={creating}
          className={`w-full rounded-md bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white text-sm font-medium transition-colors flex items-center ${
            collapsed ? "justify-center h-9" : "gap-2 px-3 h-9"
          }`}
          title="新建任务"
        >
          <Plus size={16} className="shrink-0" />
          {!collapsed && <span>新建任务</span>}
        </button>
      </div>

      {/* Nav items — do NOT scroll. shrink-0 keeps them pinned. */}
      <ul className="shrink-0 py-2">
        {NAV_ITEMS.map((item) => {
          const active = item.id === current;
          return (
            <li key={item.id}>
              <button
                type="button"
                onClick={() => onNavigate(item.id)}
                title={collapsed ? item.label : undefined}
                className={`w-full flex items-center transition-colors ${
                  collapsed
                    ? "justify-center py-2.5"
                    : "gap-2 px-3 py-2 text-sm text-left"
                } ${
                  active
                    ? "bg-neutral-800 text-white"
                    : "hover:bg-neutral-900 text-neutral-300"
                }`}
              >
                <span
                  className={`shrink-0 ${
                    active ? "text-white" : "text-neutral-400"
                  }`}
                >
                  {item.icon}
                </span>
                {!collapsed && <span className="truncate">{item.label}</span>}
              </button>
            </li>
          );
        })}
      </ul>

      {/* History — scrollable. Hidden in collapsed mode (no space for titles). */}
      {!collapsed ? (
        <>
          <div className="shrink-0 px-3 py-1.5 text-[10px] uppercase tracking-wider text-neutral-500 border-t border-neutral-900">
            最近任务
          </div>
          <div className="flex-1 min-h-0 overflow-auto">
            {sessions.length === 0 ? (
              <div className="px-3 py-2 text-[11px] text-neutral-600">
                还没有任务
              </div>
            ) : (
              sessions.map((s) => {
                const active = s.id === activeSessionId;
                return (
                  <button
                    key={s.id}
                    type="button"
                    onClick={() => handleSelectSession(s.id)}
                    className={`w-full flex items-start gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                      active
                        ? "bg-neutral-800 text-white"
                        : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200"
                    }`}
                  >
                    <MessageSquare
                      size={12}
                      className={`mt-0.5 shrink-0 ${
                        active ? "text-blue-400" : "text-neutral-600"
                      }`}
                    />
                    <span className="flex-1 min-w-0 truncate">{s.title}</span>
                  </button>
                );
              })
            )}
          </div>
        </>
      ) : (
        <div className="flex-1" />
      )}

      {/* Footer: settings */}
      {onOpenSettings && (
        <div className="shrink-0 border-t border-neutral-900 p-2">
          <button
            type="button"
            onClick={onOpenSettings}
            title="设置"
            className={`w-full rounded-md h-9 text-neutral-300 hover:bg-neutral-800 hover:text-white transition-colors flex items-center ${
              collapsed ? "justify-center" : "gap-2 px-3 text-sm"
            }`}
          >
            <SettingsIcon size={16} className="shrink-0" />
            {!collapsed && <span>设置</span>}
          </button>
        </div>
      )}
    </nav>
  );
}
