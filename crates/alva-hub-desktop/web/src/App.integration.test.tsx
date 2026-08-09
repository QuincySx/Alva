// INPUT:  App.tsx + a fully mocked ./bridge seam
// OUTPUT: renders App without a webview and asserts the fallback-card
//         controls (ACCEPTANCE.md scenario 8: Stop / Switch back / View
//         handoff), session search, and the empty-state surface.
// POS:    Closes "the three controls' wiring and their enabled/disabled logic
//         are verified only by tsc and by reading" from ACCEPTANCE.md.
//
// The live hub_timeline channel emits raw events (no fallback controls); the
// control-bearing card is projected by projectTimeline(detail) when a session
// is opened. These tests therefore drive the real openSession path instead of
// injecting a synthetic payload through the listener.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type {
  EngineReadiness,
  EngineTarget,
  FallbackPolicy,
  HandoffPackage,
  HubSessionDetail,
  PermissionGroup,
  RepairAction,
  RepairResult,
  TeamRunDetail,
} from "./bridge";

const bridge = vi.hoisted(() => {
  const listeners: Array<(event: unknown) => void> = [];
  const state = {
    sessions: [] as Array<{
      product_session_id: string;
      workspace: string;
      last_prompt: string | null;
      engine_id: string;
      status: string;
    }>,
    detail: null as HubSessionDetail | null,
  };
  return {
    listeners,
    state,
    fns: {
      chooseWorkspace: vi.fn(async () => null),
      runHubPrompt: vi.fn(),
      switchHubEngine: vi.fn(),
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
      getPermissionEligibility: vi.fn(async () => []),
      respondHubPermission: vi.fn(),
      setTranscriptSharing: vi.fn(),
      stopHubAttempt: vi.fn(),
      createTeamRun: vi.fn(),
      getTeamRunForLead: vi.fn(
        async (): Promise<TeamRunDetail | null> => null,
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
    },
    target: {
      engine_id: "claude",
      model_id: "engine-default",
      provider_id: "claude-cli",
      account_source: "Claude Code CLI account",
      billing_route: "Anthropic account",
      automatic_use_authorized: true,
      required_capabilities: [],
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
    bridge.listeners.length = 0;
    vi.clearAllMocks();
  });

  it("renders the empty surface before any session or event", async () => {
    render(<App />);
    expect(
      await screen.findByText("Start one continuous project session"),
    ).toBeInTheDocument();
    expect(screen.getByText("Not selected")).toBeInTheDocument();
    expect(screen.getByText("Hub runs will appear here.")).toBeInTheDocument();
  });

  it("opens a running session and renders the fallback card with live controls", async () => {
    const user = userEvent.setup();
    bridge.state.sessions = [
      {
        product_session_id: "ps-1",
        workspace: "/tmp/proj",
        last_prompt: "Fix the build",
        engine_id: "claude",
        status: "running",
      },
    ];
    bridge.state.detail = detailFixture();

    render(<App />);
    await user.click(await screen.findByText("Fix the build"));

    // The card is projected from the session detail, not the live channel.
    expect(await screen.findByText("Automatic fallback")).toBeInTheDocument();
    expect(screen.getByText(/alva → claude/)).toBeInTheDocument();

    const stop = screen.getByRole("button", { name: "Stop" });
    // A running session means the attempt is live: Stop must be offered.
    expect(stop).toBeEnabled();
    // The composer follows the active engine (claude), so switching back to
    // the parked alva branch is the meaningful control.
    const switchBack = screen.getByRole("button", {
      name: "Switch back to alva",
    });
    expect(switchBack).toBeEnabled();
    expect(screen.getByRole("button", { name: "View handoff" })).toBeEnabled();
  });

  it("expands a handoff with reason, goal, completed work and transcript notice", async () => {
    const user = userEvent.setup();
    bridge.state.sessions = [
      {
        product_session_id: "ps-1",
        workspace: "/tmp/proj",
        last_prompt: "Fix the build",
        engine_id: "claude",
        status: "running",
      },
    ];
    bridge.state.detail = detailFixture();
    bridge.state.detail.handoffs = [
      handoff({
        available_skills: [
          {
            name: "review",
            description: "Review every change",
            path: ".alva/skills/review/SKILL.md",
          },
        ],
        available_mcp: [
          {
            id: "filesystem",
            display_name: "Filesystem",
            transport: "stdio: npx -y @modelcontextprotocol/server-filesystem",
          },
        ],
      }),
    ];

    render(<App />);
    await user.click(await screen.findByText("Fix the build"));
    await user.click(await screen.findByRole("button", { name: "View handoff" }));

    expect(screen.getByText(/full handoff · alva → claude/)).toBeInTheDocument();
    expect(screen.getByText("Reason: rate limited")).toBeInTheDocument();
    expect(screen.getByText("Goal: Fix the build")).toBeInTheDocument();
    expect(screen.getByText("✓ Fix the build — Done")).toBeInTheDocument();
    expect(screen.getByText("Transcript not shared (default).")).toBeInTheDocument();
    // Workspace skills render as pointers the target engine can read.
    expect(screen.getByText("Workspace skills")).toBeInTheDocument();
    expect(screen.getByText(".alva/skills/review/SKILL.md")).toBeInTheDocument();
    // MCP servers render metadata only.
    expect(screen.getByText("MCP servers")).toBeInTheDocument();
    expect(
      screen.getByText("stdio: npx -y @modelcontextprotocol/server-filesystem"),
    ).toBeInTheDocument();
  });

  it("renders engine readiness with status, reasons and repair actions", async () => {
    const user = userEvent.setup();
    bridge.fns.getEngineReadiness.mockResolvedValue([
      {
        engine_id: "alva",
        display_name: "Alva",
        status: "healthy",
        binary_path: "built-in",
        version: "0.1.0",
        minimum_version: "0.1.0",
        capabilities: ["streaming"],
        authenticated: true,
        stream_health: "ok",
        reasons: [],
      },
      {
        engine_id: "claude",
        display_name: "Claude Code",
        status: "unauthenticated",
        binary_path: "/usr/local/bin/claude",
        version: "2.1.226",
        minimum_version: "1.0.0",
        capabilities: [],
        authenticated: false,
        stream_health: "untested",
        reasons: [
          { code: "unauthenticated", remedy: "Run claude once to sign in." },
        ],
      },
    ]);
    bridge.fns.getRepairPlan.mockResolvedValue([
      {
        engine_id: "alva",
        kind: "configure",
        title: "Configure provider key",
        command: null,
        url: null,
        one_click: true,
      },
    ]);
    bridge.fns.runEngineRepair.mockResolvedValue({
      outcome: { success: true, log: "ok" },
      readiness: null as unknown as EngineReadiness,
    });

    render(<App />);
    expect(await screen.findByText("Healthy")).toBeInTheDocument();
    expect(screen.getByText("Sign-in needed")).toBeInTheDocument();
    expect(screen.getByText("Run claude once to sign in.")).toBeInTheDocument();
    expect(screen.getByText(/v2.1.226 · requires ≥ 1.0.0/)).toBeInTheDocument();

    // One-click repair dispatches with the engine and kind.
    await user.click(
      screen.getByRole("button", { name: "Configure provider key" }),
    );
    expect(bridge.fns.runEngineRepair).toHaveBeenCalledWith(
      "alva",
      "configure",
      null,
    );
    // The repair log lands on the card.
    expect(
      await screen.findByText(/Repair finished/),
    ).toBeInTheDocument();
  });

  it("renders team workers with lifecycle controls and dispatches actions", async () => {
    const user = userEvent.setup();
    bridge.state.sessions = [
      {
        product_session_id: "ps-1",
        workspace: "/tmp/proj",
        last_prompt: "Fix the build",
        engine_id: "codex",
        status: "parked",
      },
    ];
    bridge.state.detail = detailFixture();
    bridge.fns.getTeamRunForLead.mockResolvedValue({
      team_run_id: "tr-1",
      autonomous: false,
      lead_session_id: "ps-1",
      status: "active",
      created_at: 100,
      workers: [
        {
          worker_id: "w-codex",
          lifecycle: "active",
          worktree_path: null,
          worktree_branch: null,
          worker_session_id: "ws-codex",
          engine_id: "codex",
          created_at: 101,
        },
        {
          worker_id: "w-alva",
          lifecycle: "hibernated",
          worktree_path: "/tmp/proj/.alva-worktrees/w-alva",
          worktree_branch: "worker/w-alva",
          worker_session_id: "ws-alva",
          engine_id: "alva",
          created_at: 102,
        },
      ],
      tasks: [
        {
          task_id: "t-1",
          team_run_id: "tr-1",
          worker_id: "w-codex",
          scope: "implement parser",
          status: "running",
          result: null,
          changed_files: [],
          verification: [],
          claims: [],
          created_at: 103,
          completed_at: null,
        },
      ],
    });
    bridge.fns.hibernateWorker.mockResolvedValue({
      worker_id: "w-codex",
      lifecycle: "hibernated",
      worktree_path: null,
      worktree_branch: null,
      worker_session_id: "ws-codex",
      engine_id: "codex",
      created_at: 101,
    });
    bridge.fns.previewWorkerAdoption.mockResolvedValue({
      changes: [{ path: "src/parser.rs", status: "modified" }],
      conflicts: ["src/lib.rs"],
      applies_cleanly: false,
      verification: ["cargo test passes"],
    });
    bridge.fns.adoptWorkerWork.mockResolvedValue(undefined);

    render(<App />);
    await user.click(await screen.findByText("Fix the build"));

    // Durable workers render with engine, lifecycle and isolation markers.
    expect(await screen.findByText("Worker · codex")).toBeInTheDocument();
    expect(screen.getByText("Worker · alva")).toBeInTheDocument();
    expect(screen.getByText("hibernated")).toBeInTheDocument();
    expect(screen.getByText("isolated")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Worktree worker/w-alva — changes stay here until you adopt them.",
      ),
    ).toBeInTheDocument();
    // A running task arms the Interrupt control on its worker.
    expect(
      screen.getAllByRole("button", { name: "Interrupt" })[0],
    ).toBeEnabled();

    // Hibernate dispatches the lifecycle action, not a silent state guess.
    await user.click(screen.getByRole("button", { name: "Hibernate" }));
    expect(bridge.fns.hibernateWorker).toHaveBeenCalledWith("w-codex");

    // Adoption is explicit and surfaced: preview first, conflicts shown,
    // then the adopt action carries the run + worker ids.
    await user.click(screen.getByRole("button", { name: "Review changes" }));
    expect(
      await screen.findByText(/1 change\(s\) · does NOT apply cleanly/),
    ).toBeInTheDocument();
    expect(screen.getByText("modified src/parser.rs")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Conflicts with your workspace edits: src/lib.rs",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("cargo test passes")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Adopt" }));
    expect(bridge.fns.adoptWorkerWork).toHaveBeenCalledWith(
      "tr-1",
      "w-alva",
    );
  });

  it("renders a grouped approval and decides it as one card", async () => {
    const user = userEvent.setup();
    bridge.fns.getPermissionGroups.mockResolvedValue([
      {
        group_key: "g-bash",
        tool_name: "bash",
        members: [
          {
            scope: "team-run-1",
            product_session_id: "ps-a",
            request_id: "r1",
            requester: "worker-a",
            tool_name: "bash",
            resource: "src/",
          },
          {
            scope: "team-run-1",
            product_session_id: "ps-b",
            request_id: "r2",
            requester: "worker-b",
            tool_name: "bash",
            resource: "src/",
          },
        ],
      },
    ]);
    bridge.fns.respondPermissionGroup.mockResolvedValue(0);

    render(<App />);
    // Aggregation must never hide who is asking for what.
    expect(
      await screen.findByText(/2 workers want to run 'bash'/),
    ).toBeInTheDocument();
    expect(screen.getByText(/worker-a/)).toBeInTheDocument();
    expect(screen.getByText(/worker-b/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Allow all 2" }));
    // Exactly the displayed request ids travel — never a re-fetch guess.
    expect(bridge.fns.respondPermissionGroup).toHaveBeenCalledWith(
      "g-bash",
      true,
      ["r1", "r2"],
    );
  });

  it("edits the fallback target model and saves the chain", async () => {
    const user = userEvent.setup();
    bridge.fns.getGlobalFallbackPolicy.mockResolvedValue({
      targets: [
        {
          engine_id: "claude",
          model_id: "engine-default",
          provider_id: "claude-cli",
          account_source: "Claude Code CLI account",
          billing_route: "Anthropic account via Claude CLI",
          automatic_use_authorized: true,
          required_capabilities: [],
        },
      ],
      updated_at: 0,
    });
    bridge.fns.setGlobalFallbackPolicy.mockResolvedValue({
      targets: [],
      updated_at: 0,
    });

    render(<App />);
    const modelInput = await screen.findByLabelText("Model for claude");
    expect(modelInput).toHaveValue("engine-default");

    // The composer must show the selected engine's effective config, so
    // pick claude (the engine whose model this test edits).
    await user.selectOptions(screen.getByLabelText("Engine"), "claude");

    await user.clear(modelInput);
    await user.type(modelInput, "claude-sonnet-4-5");
    // The composer reflects the edited model immediately.
    expect(
      await screen.findByText("claude-sonnet-4-5 · claude-cli"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save default chain" }));

    expect(bridge.fns.setGlobalFallbackPolicy).toHaveBeenCalledWith([
      expect.objectContaining({
        engine_id: "claude",
        model_id: "claude-sonnet-4-5",
        provider_id: "claude-cli",
      }),
    ]);
  });

  it("teaches the session memory and refreshes the drawer list", async () => {
    const user = userEvent.setup();
    bridge.state.sessions = [
      {
        product_session_id: "ps-1",
        workspace: "/tmp/proj",
        last_prompt: "Fix the build",
        engine_id: "claude",
        status: "parked",
      },
    ];
    bridge.state.detail = { ...detailFixture(), memory: ["always run tests"] };

    render(<App />);
    await user.click(await screen.findByText("Fix the build"));
    await user.click(screen.getByRole("button", { name: "Details" }));

    // Existing entries render under the Memory heading.
    expect(
      await screen.findByText("always run tests"),
    ).toBeInTheDocument();

    // Teaching the session appends and refreshes via the returned list.
    const input = screen.getByLabelText("Add session memory");
    await user.type(input, "never force-push to main");
    await user.click(screen.getByRole("button", { name: "Remember" }));

    expect(await screen.findByText("never force-push to main")).toBeInTheDocument();
    expect(bridge.fns.addSessionMemory).toHaveBeenCalledWith(
      "ps-1",
      "never force-push to main",
    );
    // The draft clears after a successful save.
    await waitFor(() => expect(input).toHaveValue(""));
  });

  it("filters sessions by workspace, prompt or engine", async () => {
    bridge.state.sessions = [
      {
        product_session_id: "ps-a",
        workspace: "/tmp/alpha",
        last_prompt: "Ship the alpha release",
        engine_id: "alva",
        status: "active",
      },
      {
        product_session_id: "ps-b",
        workspace: "/tmp/beta",
        last_prompt: "Port to codex",
        engine_id: "claude",
        status: "parked",
      },
    ];
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByText("Ship the alpha release")).toBeInTheDocument();
    expect(screen.getByText("Port to codex")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Search sessions"), "codex");
    expect(screen.getByText("Port to codex")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByText("Ship the alpha release")).not.toBeInTheDocument();
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();
  });
});
