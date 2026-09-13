// INPUT: App and the existing integration-test controlled bridge fixtures
// OUTPUT: regression for message visibility after a live completed event
// POS: review-only probe; history control passes before the terminal-event assertion
//
// The live hub_timeline channel emits raw events (no fallback controls); the
// control-bearing card is projected by projectTimeline(detail) when a session
// is opened. These tests therefore drive the real openSession path instead of
// injecting a synthetic payload through the listener.

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type {
  EngineEligibility,
  EngineReadiness,
  EngineTarget,
  FallbackPolicy,
  HandoffPackage,
  HubSessionDetail,
  ModelRoute,
  PermissionGroup,
  RepairAction,
  RepairResult,
  TeamRunDetail,
} from "./bridge";

async function addWorkbenchTab(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(screen.getByRole("button", { name: "Add workbench tab" }));
  await user.click(screen.getByRole("menuitem", { name }));
}

const bridge = vi.hoisted(() => {
  const listeners: Array<(event: unknown) => void> = [];
  const state = {
    sessions: [] as Array<{
      product_session_id: string;
      workspace: string;
      last_prompt: string | null;
      engine_id: string;
      status: string;
      updated_at?: number;
    }>,
    detail: null as HubSessionDetail | null,
    teamRun: null as TeamRunDetail | null,
  };
  return {
    listeners,
    state,
    fns: {
      chooseWorkspace: vi.fn(
        async (): Promise<string | null> => null,
      ),
      createSessionWorkspace: vi.fn(
        async () => ({ kind: "session" as const, path: "/tmp/alva-session-1" }),
      ),
      discoverModelsForProvider: vi.fn(),
      testModelRoute: vi.fn(),
      runHubPrompt: vi.fn(),
      switchHubEngine: vi.fn(),
      listModelProviders: vi.fn(),
      saveModelProvider: vi.fn(),
      listModelRoutes: vi.fn(),
      getDefaultModelRoute: vi.fn(),
      setDefaultModelRoute: vi.fn(),
      listHubSessions: vi.fn(async () => state.sessions),
      getHubSession: vi.fn(async () => state.detail),
      addSessionMemory: vi.fn(
        async (_productSessionId: string, entry: string) => {
          if (state.detail) {
            state.detail.memory = [...state.detail.memory, entry.trim()];
          }
          return state.detail?.memory ?? [];
        },
      ),
      getGlobalFallbackPolicy: vi.fn(
        async (): Promise<FallbackPolicy> => ({
          targets: [],
          updated_at: 0,
        }),
      ),
      setGlobalFallbackPolicy: vi.fn(
        async (_targets: EngineTarget[]): Promise<FallbackPolicy> => ({
          targets: [],
          updated_at: 0,
        }),
      ),
      setSessionFallbackPolicy: vi.fn(),
      getPermissionEligibility: vi.fn(
        async (): Promise<EngineEligibility[]> => [],
      ),
      respondHubPermission: vi.fn(),
      setTranscriptSharing: vi.fn(),
      stopHubAttempt: vi.fn(),
      createTeamRun: vi.fn(),
      getTeamRunForLead: vi.fn(
        async (): Promise<TeamRunDetail | null> => state.teamRun,
      ),
      dispatchWorkerTask: vi.fn(),
      addTeamWorker: vi.fn(),
      getPermissionGroups: vi.fn(
        async (): Promise<PermissionGroup[]> => [],
      ),
      respondPermissionGroup: vi.fn(),
      setTeamAutonomy: vi.fn(),
      hibernateWorker: vi.fn(),
      restoreWorker: vi.fn(),
      archiveWorker: vi.fn(),
      completeTeamRun: vi.fn(),
      previewWorkerAdoption: vi.fn(),
      adoptWorkerWork: vi.fn(),
      discardWorkerWorktree: vi.fn(),
      updateWorkerTask: vi.fn(),
      cancelWorkerTask: vi.fn(),
      interruptWorker: vi.fn(),
      openExternal: vi.fn(),
      getRepairPlan: vi.fn(async (): Promise<RepairAction[]> => []),
      runEngineRepair: vi.fn(
        async (): Promise<RepairResult> => ({
          outcome: { success: false, log: "unused" },
          readiness: {
            engine_id: "alva",
            display_name: "Alva",
            status: "healthy",
            binary_path: null,
            version: null,
            minimum_version: "0.1.0",
            capabilities: [],
            authenticated: null,
            stream_health: "untested",
            reasons: [],
          },
        }),
      ),
      getEngineReadiness: vi.fn(async (): Promise<EngineReadiness[]> => []),
      listenTimeline: vi.fn(async (callback: (event: unknown) => void) => {
        listeners.push(callback);
        return () => {
          const index = listeners.indexOf(callback);
          if (index >= 0) listeners.splice(index, 1);
        };
      }),
    },
  };
});

vi.mock("./bridge", () => bridge.fns);

const WORKSPACE_DEFAULT_TARGET: EngineTarget = {
  engine_id: "alva",
  model_id: "workspace-config",
  provider_id: "workspace-config",
  account_source: "Workspace provider configuration",
  billing_route: "Workspace provider account",
  automatic_use_authorized: false,
  required_capabilities: [],
  reasoning_effort: "high",
  fast_mode: false,
};

const MODEL_ROUTES: ModelRoute[] = [
  {
    target: WORKSPACE_DEFAULT_TARGET,
    engine_label: "Alva",
    model_label: "Workspace model",
    provider_label: "Workspace provider",
    description: "Uses the provider configuration resolved for this workspace.",
    context_window: null,
    capabilities: [],
    reasoning_efforts: ["low", "medium", "high"],
    default_reasoning_effort: "high",
    supports_fast_mode: false,
    available: true,
    unavailable_reason: null,
  },
  {
    target: {
      engine_id: "claude",
      model_id: "engine-default",
      provider_id: "claude-cli",
      account_source: "Claude Code subscription",
      billing_route: "Anthropic account via Claude Code",
      automatic_use_authorized: false,
      required_capabilities: [],
      reasoning_effort: "high",
      fast_mode: false,
    },
    engine_label: "Claude Code",
    model_label: "Default model",
    provider_label: "Claude Code",
    description: "Uses the selected model through your local CLI subscription.",
    context_window: null,
    capabilities: [],
    reasoning_efforts: ["low", "medium", "high", "xhigh", "max"],
    default_reasoning_effort: "high",
    supports_fast_mode: false,
    available: true,
    unavailable_reason: null,
  },
  {
    target: {
      engine_id: "codex",
      model_id: "engine-default",
      provider_id: "codex-cli",
      account_source: "Codex subscription",
      billing_route: "OpenAI account via Codex",
      automatic_use_authorized: false,
      required_capabilities: [],
      reasoning_effort: "high",
      fast_mode: false,
    },
    engine_label: "Codex",
    model_label: "Default model",
    provider_label: "Codex",
    description: "Uses the selected model through your local CLI subscription.",
    context_window: null,
    capabilities: [],
    reasoning_efforts: ["minimal", "low", "medium", "high", "xhigh"],
    default_reasoning_effort: "high",
    supports_fast_mode: true,
    available: true,
    unavailable_reason: null,
  },
];

const WORKSPACE_DEFAULT_ROUTE = MODEL_ROUTES[0];

const HEALTHY_ENGINES: EngineReadiness[] = MODEL_ROUTES.map((route) => ({
  engine_id: route.target.engine_id,
  display_name: route.engine_label,
  status: "healthy",
  binary_path: null,
  version: null,
  minimum_version: "0.1.0",
  capabilities: [],
  authenticated: true,
  stream_health: "ok",
  reasons: [],
}));

function handoff(partial: Partial<HandoffPackage> = {}): HandoffPackage {
  return {
    handoff_id: "h-1",
    product_session_id: "ps-1",
    source_branch_id: "b-alva",
    target_branch_id: "b-claude",
    source: {
      engine_id: "alva",
      model_id: "workspace-config",
      provider_id: "workspace-config",
      account_source: "Workspace provider",
      billing_route: "Workspace provider account",
      automatic_use_authorized: true,
      required_capabilities: [],
      reasoning_effort: "high",
      fast_mode: false,
    },
    target: {
      engine_id: "claude",
      model_id: "engine-default",
      provider_id: "claude-cli",
      account_source: "Claude Code CLI account",
      billing_route: "Anthropic account",
      automatic_use_authorized: true,
      required_capabilities: [],
      reasoning_effort: "high",
      fast_mode: false,
    },
    kind: "full",
    reason: "rate limited",
    user_goal: "Fix the build",
    latest_instruction: "Keep going",
    accepted_decisions: [],
    completed_work: [{ prompt: "Fix the build", result: "Done" }],
    pending_work: [],
    changed_files: ["src/main.rs"],
    recent_commands: [],
    verification_results: [],
    workspace_summary: "Rust workspace",
    team_snapshot: null,
    transcript: [],
    session_memory: [],
    available_skills: [],
    available_mcp: [],
    created_at: 108,
    used_at: null,
    ...partial,
  };
}

function detailFixture(): HubSessionDetail {
  return {
    session: {
      product_session_id: "ps-1",
      workspace: "/tmp/proj",
      workspace_kind: "project",
      active_branch_id: "b-claude",
      engine_id: "claude",
      model_id: "engine-default",
      provider_id: "claude-cli",
      engine_session_id: "ns-1",
      status: "running",
      updated_at: 115,
      last_prompt: "Fix the build",
      last_result: null,
      permission_profile: "workspace_write",
      include_full_transcript: false,
    },
    turns: [],
    branches: [
      {
        branch_id: "b-alva",
        engine_id: "alva",
        model_id: "workspace-config",
        provider_id: "workspace-config",
        account_source: "Workspace provider",
        billing_route: "Workspace provider account",
        automatic_use_authorized: true,
        required_capabilities: [],
        reasoning_effort: "high",
        fast_mode: false,
        engine_session_id: "ns-alva",
        status: "parked",
        created_at: 100,
        updated_at: 108,
      },
      {
        branch_id: "b-claude",
        engine_id: "claude",
        model_id: "engine-default",
        provider_id: "claude-cli",
        account_source: "Claude Code CLI account",
        billing_route: "Anthropic account",
        automatic_use_authorized: true,
        required_capabilities: [],
        reasoning_effort: "high",
        fast_mode: false,
        engine_session_id: "ns-claude",
        status: "active",
        created_at: 108,
        updated_at: 115,
      },
    ],
    handoffs: [handoff()],
    timeline_events: [
      {
        event_id: "e-1",
        kind: "engine_fallback",
        source_branch_id: "b-alva",
        target_branch_id: "b-claude",
        source_engine_id: "alva",
        target_engine_id: "claude",
        reason: "rate limited",
        created_at: 108,
      },
    ],
    pending_switch: null,
    fallback_policy: { targets: [], updated_at: 0 },
    memory: [],
  };
}

describe("App integration", () => {
  beforeEach(() => {
    bridge.state.sessions = [];
    bridge.state.detail = null;
    bridge.state.teamRun = null;
    bridge.listeners.length = 0;
    vi.clearAllMocks();
    bridge.fns.listModelProviders.mockResolvedValue([]);
    bridge.fns.listModelRoutes.mockResolvedValue(MODEL_ROUTES);
    bridge.fns.getDefaultModelRoute.mockResolvedValue(WORKSPACE_DEFAULT_ROUTE);
    bridge.fns.setDefaultModelRoute.mockResolvedValue(WORKSPACE_DEFAULT_ROUTE);
    bridge.fns.discoverModelsForProvider.mockResolvedValue({ models: [], ok: true, latency_ms: 1, message: "Discovered 0 model(s)." });
    bridge.fns.testModelRoute.mockResolvedValue({ models: [], ok: true, latency_ms: 1, message: "Inference succeeded for the selected model." });
    bridge.fns.getEngineReadiness.mockResolvedValue(HEALTHY_ENGINES);
    bridge.fns.getPermissionEligibility.mockResolvedValue([]);
  });

  it("round4 completed live event keeps assistant text visible", async () => {
    const user = userEvent.setup();
    bridge.state.sessions = [{ product_session_id: "ps-1", workspace: "/tmp/proj", last_prompt: "Review greeting", engine_id: "claude", status: "completed" }];
    const detail = detailFixture();
    detail.session.status = "completed";
    detail.session.last_result = "REVIEW_REPLY_MUST_STAY_VISIBLE";
    detail.timeline_events = [];
    detail.turns = [{ turn_id: "review-turn", prompt: "Review greeting", status: "completed", result: "REVIEW_REPLY_MUST_STAY_VISIBLE", error: null, created_at: 100, completed_at: 110, attempts: [] }];
    bridge.state.detail = detail;
    render(<App />);
    await user.click(await screen.findByText("Review greeting"));
    await waitFor(() => expect(bridge.fns.getHubSession).toHaveBeenCalled());
    expect(await screen.findByText("REVIEW_REPLY_MUST_STAY_VISIBLE")).toBeInTheDocument();
    act(() => {
      bridge.listeners.forEach((callback) => callback({ product_session_id: "ps-1", engine_id: "claude", status: "completed", message: "REVIEW_REPLY_MUST_STAY_VISIBLE", item_id: "review-turn:terminal" }));
    });
    expect(await screen.findByText("REVIEW_REPLY_MUST_STAY_VISIBLE")).toBeInTheDocument();
  });

});
