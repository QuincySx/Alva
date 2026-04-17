import { useEffect, useState } from "react";
import { NavSidebar, type RouteId } from "./components/NavSidebar";
import { ResizableSplit } from "./components/ResizableSplit";
import { SettingsModal } from "./components/SettingsModal";
import Home from "./routes/Home";
import Mcp from "./routes/Mcp";
import Placeholder from "./routes/Placeholder";
import Skills from "./routes/Skills";
import { useAppStore } from "./store/appStore";

function renderRoute(id: RouteId) {
  switch (id) {
    case "home":
      return <Home />;
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

  // Cmd/Ctrl+B — macOS standard for sidebar toggle (VS Code / Mail both use it).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggleNavCollapsed();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleNavCollapsed]);

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
            onOpenSettings={openSettings}
            onCollapse={toggleNavCollapsed}
            collapsed={navCollapsed}
          />
        }
        right={<div className="h-full w-full">{renderRoute(route)}</div>}
      />

      <SettingsModal open={settingsOpen} onClose={closeSettings} />
    </div>
  );
}
