import { useEffect, useRef, useState } from "react";
import { NavSidebar, type RouteId } from "./components/NavSidebar";
import { ResizableSplit } from "./components/ResizableSplit";
import { SettingsModal } from "./components/SettingsModal";
import Home from "./routes/Home";
import Mcp from "./routes/Mcp";
import Placeholder from "./routes/Placeholder";
import Skills from "./routes/Skills";
import { useAppStore } from "./store/appStore";

function renderRoute(id: RouteId, onNavigate: (next: RouteId) => void) {
  switch (id) {
    case "home":
      return <Home onNavigate={onNavigate} />;
    case "search":
      return (
        <Placeholder
          title="搜索任务"
          description="历史任务搜索。下一批接 alva-app-eval 的 session 持久化 + 全文检索。"
        />
      );
    case "schedule":
      return (
        <Placeholder
          title="定时任务"
          description="Cron 任务列表。下一批接 alva-agent-extension-builtin::schedule。"
        />
      );
    case "skills":
      return <Skills />;
    case "mcp":
      return <Mcp />;
    case "agents":
      return (
        <Placeholder
          title="我的 Agent"
          description="自定义 agent 配置(system prompt + 预置工具 + 预置技能)可以在 设置 → 我的 Agent 中管理。"
        />
      );
  }
}

export default function App() {
  const [route, setRoute] = useState<RouteId>("home");
  const settingsOpen = useAppStore((s) => s.settingsOpen);
  const openSettings = useAppStore((s) => s.openSettings);
  const closeSettings = useAppStore((s) => s.closeSettings);
  const navCollapsed = useAppStore((s) => s.navCollapsed);
  const toggleNavCollapsed = useAppStore((s) => s.toggleNavCollapsed);
  const setNavCollapsed = useAppStore((s) => s.setNavCollapsed);

  // Auto-collapse the sidebar on narrow viewports (mobile-like split-view,
  // small desktop windows) where 220px of nav eats ~50% of horizontal
  // space. Only acts on THRESHOLD CROSSINGS, not every resize event — so a
  // user who manually expands the sidebar while narrow stays expanded
  // (effect doesn't fight). On expand back past the threshold, we revert
  // ONLY the collapses we initiated. (React rules: 逻辑值 用 useRef.)
  const autoCollapsedRef = useRef(false);
  const prevNarrowRef = useRef<boolean | null>(null);
  useEffect(() => {
    const NARROW = 640;
    const apply = () => {
      const narrow = window.innerWidth < NARROW;
      const wasNarrow = prevNarrowRef.current;
      prevNarrowRef.current = narrow;
      const state = useAppStore.getState();
      if (wasNarrow === null) {
        if (narrow && !state.navCollapsed) {
          autoCollapsedRef.current = true;
          setNavCollapsed(true);
        }
        return;
      }
      if (!wasNarrow && narrow) {
        if (!state.navCollapsed) {
          autoCollapsedRef.current = true;
          setNavCollapsed(true);
        }
      } else if (wasNarrow && !narrow && autoCollapsedRef.current) {
        autoCollapsedRef.current = false;
        setNavCollapsed(false);
      }
    };
    apply();
    window.addEventListener("resize", apply);
    return () => window.removeEventListener("resize", apply);
  }, [setNavCollapsed]);

  // Cmd/Ctrl+B — macOS standard for sidebar toggle (VS Code / Mail both use it).
  // Suppressed while a modal is open so the modal owns the keyboard — otherwise
  // ⌘B on muscle memory while Settings is up silently collapses the sidebar
  // behind the overlay.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (settingsOpen) return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggleNavCollapsed();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleNavCollapsed, settingsOpen]);

  return (
    <div className="relative h-screen w-screen bg-neutral-950 text-neutral-100 overflow-hidden">
      <ResizableSplit
        storageKey="alva.navWidth"
        defaultWidth={220}
        minWidth={160}
        maxWidth={420}
        collapsed={navCollapsed}
        collapsedWidth={48}
        left={
          <NavSidebar
            current={route}
            onNavigate={setRoute}
            onOpenSettings={() => openSettings()}
            onCollapse={toggleNavCollapsed}
            collapsed={navCollapsed}
          />
        }
        right={<div className="h-full w-full">{renderRoute(route, setRoute)}</div>}
      />

      <SettingsModal open={settingsOpen} onClose={closeSettings} />
    </div>
  );
}
