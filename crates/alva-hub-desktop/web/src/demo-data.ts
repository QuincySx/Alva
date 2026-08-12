// INPUT:  bridge types
// OUTPUT: deterministic demo data for `?demo=1` in the vite dev server
// POS:    Lets the wireframe/vision review tools see a DATA state (sessions,
//         timeline cards, team run) without the Tauri runtime. Never active
//         in production: hasTauri=true bypasses every demo branch.
import type {
  EngineReadiness,
  EngineTarget,
  FallbackPolicy,
  HubSessionDetail,
  HubSessionSummary,
  TeamRunDetail,
} from "./bridge";

const engine = (id: string): EngineTarget => {
  const target = ({
    alva: {
      engine_id: "alva",
      model_id: "workspace-config",
      provider_id: "workspace-config",
      account_source: "Workspace provider",
      billing_route: "Workspace provider account",
      automatic_use_authorized: true,
      required_capabilities: [],
    },
    claude: {
      engine_id: "claude",
      model_id: "claude-sonnet-4-5",
      provider_id: "claude-cli",
      account_source: "Claude Code CLI account",
      billing_route: "Anthropic account via Claude CLI",
      automatic_use_authorized: true,
      required_capabilities: [],
    },
    codex: {
      engine_id: "codex",
      model_id: "codex-mini",
      provider_id: "codex-cli",
      account_source: "Codex CLI account",
      billing_route: "OpenAI account via Codex CLI",
      automatic_use_authorized: true,
      required_capabilities: [],
    },
  } as Record<string, EngineTarget>)[id];
  if (!target) throw new Error("unknown demo engine: " + id);
  return target;
};

export const demoSessions: HubSessionSummary[] = [
  {
    product_session_id: "ps-demo-1",
    workspace: "/Users/dev/projects/alva-agent",
    active_branch_id: "b-claude",
    engine_id: "claude",
    model_id: "claude-sonnet-4-5",
    provider_id: "claude-cli",
    engine_session_id: "native-claude-1",
    status: "parked",
    updated_at: 1725000001000,
    last_prompt: "Refactor the handoff redaction filter",
    last_result: "Moved 40+ secret shapes into one table-driven pass.",
    permission_profile: "workspace_write",
    include_full_transcript: false,
  },
  {
    product_session_id: "ps-demo-2",
    workspace: "/Users/dev/projects/other-repo",
    active_branch_id: "b-codex",
    engine_id: "codex",
    model_id: "codex-mini",
    provider_id: "codex-cli",
    engine_session_id: "native-codex-1",
    status: "active",
    updated_at: 1725000100000,
    last_prompt: "Add a worker queue status endpoint",
    last_result: null,
    permission_profile: "workspace_write",
    include_full_transcript: true,
  },
  {
    product_session_id: "ps-demo-3",
    workspace: "/Users/dev/projects/scratch",
    active_branch_id: "b-alva",
    engine_id: "alva",
    model_id: "workspace-config",
    provider_id: "workspace-config",
    engine_session_id: null,
    status: "completed",
    updated_at: 1724999000000,
    last_prompt: "Prototype the claims conflict algebra",
    last_result: "Matrix of 12 conflict pairs verified.",
    permission_profile: "interactive",
    include_full_transcript: false,
  },
];

export function demoDetail(): HubSessionDetail {
  return {
    session: demoSessions[0],
    turns: [
      {
        turn_id: "turn-demo-1",
        prompt: "Refactor the handoff redaction filter",
        result: "Moved 40+ secret shapes into one table-driven pass.",
        error: null,
        status: "completed",
        created_at: 1724999000000,
        completed_at: 1724999100000,
        attempts: [
          {
            attempt_id: "att-demo-1",
            turn_id: "turn-demo-1",
            branch_id: "b-alva",
            engine_id: "alva",
            status: "completed",
            failure_reason: null,
            usage: {
              input_tokens: 8200,
              output_tokens: 1400,
              total_cost_usd: 0.12,
              duration_ms: 42000,
              num_turns: 3,
            },
            side_effects: [
              {
                tool_call_id: "tc-1",
                tool_name: "file_edit",
                succeeded: true,
                duration_ms: 210,
                path: "src/redact.rs",
              },
            ],
            started_at: 1724999000000,
            completed_at: 1724999100000,
          },
        ],
      },
      {
        turn_id: "turn-demo-2",
        prompt: "Port the redaction tests to the new table",
        result: null,
        error: "rate limited: 429 from provider",
        status: "failed",
        created_at: 1724999200000,
        completed_at: null,
        attempts: [
          {
            attempt_id: "att-demo-2",
            turn_id: "turn-demo-2",
            branch_id: "b-alva",
            engine_id: "alva",
            status: "failed",
            failure_reason: "rate limited",
            usage: null,
            side_effects: [],
            started_at: 1724999200000,
            completed_at: null,
          },
          {
            attempt_id: "att-demo-3",
            turn_id: "turn-demo-2",
            branch_id: "b-claude",
            engine_id: "claude",
            status: "completed",
            failure_reason: null,
            usage: {
              input_tokens: 3100,
              output_tokens: 900,
              total_cost_usd: 0.05,
              duration_ms: 18000,
              num_turns: 2,
            },
            side_effects: [
              {
                tool_call_id: "tc-2",
                tool_name: "bash",
                succeeded: true,
                duration_ms: 900,
                path: null,
              },
            ],
            started_at: 1724999220000,
            completed_at: 1724999280000,
          },
        ],
      },
    ],
    branches: [
      {
        branch_id: "b-alva",
        engine_id: "alva",
        model_id: "workspace-config",
        provider_id: "workspace-config",
        engine_session_id: "native-alva-1",
        status: "parked",
        created_at: 1724998000000,
        updated_at: 1724999200000,
      },
      {
        branch_id: "b-claude",
        engine_id: "claude",
        model_id: "claude-sonnet-4-5",
        provider_id: "claude-cli",
        engine_session_id: "native-claude-1",
        status: "active",
        created_at: 1724999220000,
        updated_at: 1725000000000,
      },
    ],
    handoffs: [
      {
        handoff_id: "ho-demo-1",
        product_session_id: "ps-demo-1",
        source_branch_id: "b-alva",
        target_branch_id: "b-claude",
        source: engine("alva"),
        target: engine("claude"),
        kind: "full",
        reason: "rate limited: 429 from provider",
        user_goal: "Refactor the handoff redaction filter",
        latest_instruction: "Port the redaction tests to the new table",
        accepted_decisions: ["keep the secret shapes table-driven"],
        completed_work: [
          { prompt: "Refactor the redaction filter", result: "Done" },
        ],
        pending_work: [
          { prompt: "Port the tests", reason: "rate limited" },
        ],
        changed_files: ["src/redact.rs"],
        recent_commands: ["cargo test -p alva-hub-desktop"],
        verification_results: ["cargo test passes"],
        workspace_summary:
          "Workspace (authoritative; inspect it before trusting transcript evidence): /Users/dev/projects/alva-agent",
        team_snapshot: null,
        transcript: [
          { prompt: "Refactor the redaction filter", result: "Done" },
        ],
        session_memory: ["always run cargo test before finishing"],
        available_skills: [
          {
            name: "review",
            description: "Review every change before landing",
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
        created_at: 1724999220000,
        used_at: 1724999225000,
      },
    ],
    timeline_events: [
      {
        event_id: "te-demo-1",
        kind: "engine_fallback",
        source_branch_id: "b-alva",
        target_branch_id: "b-claude",
        source_engine_id: "alva",
        target_engine_id: "claude",
        reason: "rate limited: 429 from provider",
        created_at: 1724999220000,
      },
    ],
    pending_switch: null,
    fallback_policy: {
      targets: [engine("alva"), engine("claude"), engine("codex")],
      updated_at: 1724998000000,
    },
    memory: ["always run cargo test before finishing"],
  };
}

export function demoTeamRun(): TeamRunDetail {
  return {
    team_run_id: "tr-demo-1",
    autonomous: false,
    lead_session_id: "ps-demo-2",
    status: "active",
    created_at: 1725000000000,
    workers: [
      {
        worker_id: "w-demo-1",
        lifecycle: "active",
        worktree_path: null,
        worktree_branch: null,
        worker_session_id: "ps-worker-1",
        engine_id: "codex",
        created_at: 1725000001000,
      },
      {
        worker_id: "w-demo-2",
        lifecycle: "hibernated",
        worktree_path: "/Users/dev/projects/other-repo/.alva-worktrees/w-demo-2",
        worktree_branch: "worker/w-demo-2",
        worker_session_id: "ps-worker-2",
        engine_id: "alva",
        created_at: 1725000002000,
      },
    ],
    tasks: [
      {
        task_id: "t-demo-1",
        team_run_id: "tr-demo-1",
        worker_id: "w-demo-1",
        scope: "implement the queue status endpoint",
        status: "running",
        result: null,
        changed_files: [],
        verification: [],
        claims: [{ kind: "path", path: "src/queue.rs", mode: "write" }],
        created_at: 1725000003000,
        completed_at: null,
      },
      {
        task_id: "t-demo-2",
        team_run_id: "tr-demo-1",
        worker_id: "w-demo-2",
        scope: "write API docs for the worker endpoints",
        status: "queued",
        result: null,
        changed_files: [],
        verification: [],
        claims: [{ kind: "path", path: "docs", mode: "read" }],
        created_at: 1725000004000,
        completed_at: null,
      },
    ],
  };
}

export function demoReadiness(): EngineReadiness[] {
  return [
    {
      engine_id: "alva",
      display_name: "Alva",
      status: "healthy",
      binary_path: "built-in",
      version: "0.1.0",
      minimum_version: "0.1.0",
      capabilities: ["streaming", "tool_control", "resume"],
      authenticated: true,
      stream_health: "ok",
      reasons: [],
    },
    {
      engine_id: "claude",
      display_name: "Claude Code",
      status: "healthy",
      binary_path: "/usr/local/bin/claude",
      version: "2.1.226",
      minimum_version: "1.0.0",
      capabilities: ["streaming", "resume"],
      authenticated: true,
      stream_health: "ok",
      reasons: [],
    },
    {
      engine_id: "codex",
      display_name: "Codex CLI",
      status: "unauthenticated",
      binary_path: "/usr/local/bin/codex",
      version: "0.147.0",
      minimum_version: "0.20.0",
      capabilities: ["streaming"],
      authenticated: false,
      stream_health: "untested",
      reasons: [
        { code: "unauthenticated", remedy: "Run codex login in a terminal." },
      ],
    },
  ];
}

export function demoFallbackPolicy(): FallbackPolicy {
  return {
    targets: [engine("alva"), engine("claude"), engine("codex")],
    updated_at: 1724998000000,
  };
}
