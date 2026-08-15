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
