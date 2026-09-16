// INPUT:  AgentTool rendered with the deterministic demo TeamRunDetail
// OUTPUT: Regression: the child-agent tree renders Lead → Workers → Tasks
// POS:    Locks the wireframe-03 "child-agent relationships" shape.

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { demoTeamRun } from "../demo-data";
import { AgentTool } from "./workbench-tools";

describe("AgentTool child-agent tree", () => {
  it("renders Lead, Workers, and per-worker Tasks as a nested tree", () => {
    render(<AgentTool run={demoTeamRun()} onFocus={() => {}} />);

    // Lead is the root.
    expect(screen.getByText(/Lead agent|主 Agent/)).toBeTruthy();

    // Both demo workers render as children (worker_id appears as a node title
    // and again as a task's worker_id description, so assert presence only).
    expect(screen.getAllByText("w-demo-1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("w-demo-2").length).toBeGreaterThan(0);

    // Tasks attach under their worker (scope text identifies the task).
    expect(screen.getByText("实现队列状态接口")).toBeTruthy();
    expect(screen.getByText("为 Worker 接口编写 API 文档")).toBeTruthy();
  });

  it("renders the empty state when there are no workers", () => {
    render(<AgentTool run={{ ...demoTeamRun(), workers: [], tasks: [] }} onFocus={() => {}} />);
    expect(screen.getByText(/No child agents|没有子 Agent/)).toBeTruthy();
  });
});

// R67: Inspector 定位必须展开完整祖先路径（根→轮次→尝试→工具），默认
// 全折叠时目标记录根本不挂载；执行概览恢复 Input/Agent/Tools 三泳道并
// 共享同一产品时钟。
import { fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { demoDetail } from "../demo-data";
import { InspectorTool } from "./workbench-tools";
import type { HubTimelineEvent } from "./hub-timeline";
import type { ReactNode } from "react";
import { UIProvider } from "../ui";
import { I18nProvider } from "../i18n";

function wrapR67(child: ReactNode) {
  localStorage.setItem("alva-hub.locale", "en");
  return render(
    <UIProvider>
      <I18nProvider>{child}</I18nProvider>
    </UIProvider>,
  );
}

function locateButtons(container: HTMLElement) {
  return Array.from(container.querySelectorAll("button")).filter(
    (button) =>
      button.textContent?.includes("Locate record") ||
      button.textContent?.includes("定位记录"),
  );
}

const r67Detail = demoDetail();
const r67Events: HubTimelineEvent[] = [
  {
    status: "user_message",
    engine_id: "alva",
    product_session_id: "ps-r67",
    message: "Refine the redaction filter",
    created_at: 1724999000000,
    item_id: "user-r67",
  },
  {
    status: "assistant_message",
    engine_id: "alva",
    product_session_id: "ps-r67",
    message: "On it — merging the table first.",
    created_at: 1724999000050,
    item_id: "assistant-r67",
  },
  {
    status: "activity_completed",
    engine_id: "alva",
    product_session_id: "ps-r67",
    message: "file_edit completed",
    created_at: 1724999000210,
    item_id: "tc-1",
    activity: { tool_name: "file_edit", succeeded: true, duration_ms: 210, path: "src/redact.rs" },
  },
];

describe("InspectorTool locate (R67)", () => {
  it("keeps the product-attempt ancestry collapsed by default", () => {
    const { container } = wrapR67(
      <InspectorTool detail={r67Detail} events={[]} loadInspection={async () => { throw new Error("fixture unavailable"); }} />,
    );
    expect(
      container.querySelector('[data-inspector-tool="tc-1"]'),
    ).toBeNull();
  });

  it("locating a finished tool expands the full ancestry and mounts the focused record", () => {
    const scrollSpy = vi.fn();
    Element.prototype.scrollIntoView = scrollSpy;
    const { container } = wrapR67(
      <InspectorTool detail={r67Detail} events={r67Events} loadInspection={async () => { throw new Error("fixture unavailable"); }} />,
    );
    const button = locateButtons(container)[0];
    expect(button).toBeTruthy();
    expect(button.disabled).toBe(false);
    fireEvent.click(button);
    const record = container.querySelector('[data-inspector-tool="tc-1"]');
    expect(record).not.toBeNull();
    expect(record?.className).toContain("hub-inspector-record--focused");
    // 完整祖先链已挂载：轮次 prompt 与尝试记录同屏可见。
    expect(container.textContent).toContain("重构交接数据的脱敏过滤器");
    expect(scrollSpy).toHaveBeenCalled();
  });

  it("locating a failed tool mounts its own record", () => {
    const failedEvents: HubTimelineEvent[] = [
      {
        status: "activity_completed",
        engine_id: "alva",
        product_session_id: "ps-r67",
        message: "bash failed",
        created_at: 1724999000300,
        item_id: "tc-1-log",
        activity: { tool_name: "bash", succeeded: false, duration_ms: 1840, path: null },
      },
    ];
    const { container } = wrapR67(
      <InspectorTool detail={r67Detail} events={failedEvents} loadInspection={async () => { throw new Error("fixture unavailable"); }} />,
    );
    fireEvent.click(locateButtons(container)[0]);
    const record = container.querySelector('[data-inspector-tool="tc-1-log"]');
    expect(record).not.toBeNull();
    expect(record?.className).toContain("hub-inspector-record--focused");
  });

  it("keyboard activation reaches the same focused record", async () => {
    const user = userEvent.setup();
    const { container } = wrapR67(
      <InspectorTool detail={r67Detail} events={r67Events} loadInspection={async () => { throw new Error("fixture unavailable"); }} />,
    );
    const button = locateButtons(container)[0];
    button.focus();
    await user.keyboard("{Enter}");
    expect(
      container.querySelector('[data-inspector-tool="tc-1"]'),
    ).not.toBeNull();
  });
});

describe("InspectorTool shared-clock execution overview (R67)", () => {
  it("renders Input / Agent / Tools lanes with clock offsets", () => {
    const { container } = wrapR67(
      <InspectorTool detail={r67Detail} events={r67Events} loadInspection={async () => { throw new Error("fixture unavailable"); }} />,
    );
    expect(container.querySelector('[data-overview-lane="input"]')).not.toBeNull();
    expect(container.querySelector('[data-overview-lane="agent"]')).not.toBeNull();
    expect(container.querySelector('[data-overview-lane="tools"]')).not.toBeNull();
    // 输入泳道承载用户 prompt；代理泳道承载 assistant 文案。
    const input = container.querySelector('[data-overview-lane="input"]');
    expect(input?.textContent).toContain("Refine the redaction filter");
    const agent = container.querySelector('[data-overview-lane="agent"]');
    expect(agent?.textContent).toContain("On it — merging the table first.");
    // 共享时钟：相对最早事件的偏移。
    expect(container.textContent).toContain("+50ms");
    expect(container.textContent).toContain("+210ms");
    // 工具行的时长与偏移同屏。
    const tools = container.querySelector('[data-overview-lane="tools"]');
    expect(tools?.textContent).toContain("210 ms");
  });
});
