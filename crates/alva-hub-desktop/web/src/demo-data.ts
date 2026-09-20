// INPUT:  bridge types
// OUTPUT: deterministic session, timeline, team, readiness, and Inspector demo data for `?demo=1`
// POS:    Lets the wireframe/vision review tools see a DATA state (sessions,
//         timeline cards, team run) without the Tauri runtime. Never active
//         in production: hasTauri=true bypasses every demo branch.
import type {
  EngineReadiness,
  EngineTarget,
  FallbackPolicy,
  HubInspectionSnapshot,
  HubSessionDetail,
  HubSessionSummary,
  ModelRoute,
  ProviderProfile,
  TeamRunDetail,
} from "./bridge";

const engine = (id: string): EngineTarget => {
  const target = ({
    alva: {
      engine_id: "alva",
      model_id: "workspace-config",
      provider_id: "workspace-config",
      account_source: "工作区供应商",
      billing_route: "工作区供应商账号",
      automatic_use_authorized: true,
      required_capabilities: [],
      reasoning_effort: null,
      fast_mode: false,
    },
    claude: {
      engine_id: "claude",
      model_id: "engine-default",
      provider_id: "claude-cli",
      account_source: "Claude Code CLI 账号",
      billing_route: "通过 Claude CLI 使用 Anthropic 账号",
      automatic_use_authorized: true,
      required_capabilities: [],
      reasoning_effort: null,
      fast_mode: false,
    },
    codex: {
      engine_id: "codex",
      model_id: "gpt-5.6-sol",
      provider_id: "codex-cli",
      account_source: "Codex CLI 账号",
      billing_route: "通过 Codex CLI 使用 OpenAI 账号",
      automatic_use_authorized: true,
      required_capabilities: [],
      reasoning_effort: "high",
      fast_mode: false,
    },
    pi: {
      engine_id: "pi",
      model_id: "claude-opus-4-7",
      provider_id: "anthropic",
      account_source: "Anthropic API key",
      billing_route: "Anthropic API usage",
      automatic_use_authorized: true,
      required_capabilities: [],
      reasoning_effort: "high",
      fast_mode: false,
    },
  } as Record<string, EngineTarget>)[id];
  if (!target) throw new Error("unknown demo engine: " + id);
  return target;
};

export function demoModelRoutes(): ModelRoute[] {
  const anthropicTarget = (engineId: "alva" | "claude" | "pi"): EngineTarget => ({
    ...engine(engineId === "pi" ? "pi" : engineId),
    engine_id: engineId,
    model_id: "claude-opus-4-7",
    provider_id: "anthropic",
    account_source: "Anthropic API key",
    billing_route: "Anthropic API usage",
    reasoning_effort: "high",
  });
  const responsesTarget = (engineId: "alva" | "codex" | "pi"): EngineTarget => ({
    ...engine(engineId === "pi" ? "pi" : engineId),
    engine_id: engineId,
    model_id: "gpt-5.4",
    provider_id: "openai-responses",
    account_source: "OpenAI Responses API key",
    billing_route: "OpenAI API usage",
    reasoning_effort: "high",
  });
  const routes: Array<[EngineTarget, string, string, string, boolean, string | null]> = [
    [engine("alva"), "Alva", "工作区模型", "工作区供应商", true, null],
    [anthropicTarget("alva"), "Alva", "Claude Opus 4.7", "Anthropic", true, null],
    [responsesTarget("alva"), "Alva", "GPT-5.4", "OpenAI Responses", false, "请先在设置中添加 API 密钥。"],
    [engine("claude"), "Claude Code", "默认模型", "Claude Code 订阅", true, null],
    [anthropicTarget("claude"), "Claude Code", "Claude Opus 4.7", "Anthropic", true, null],
    [engine("codex"), "Codex", "GPT-5.6-Sol", "Codex 订阅", true, null],
    [responsesTarget("codex"), "Codex", "GPT-5.4", "OpenAI Responses", false, "请先在设置中添加 API 密钥。"],
    [anthropicTarget("pi"), "Pi", "Claude Opus 4.7", "Anthropic", true, null],
    [responsesTarget("pi"), "Pi", "GPT-5.4", "OpenAI Responses", false, "请先在设置中添加 API 密钥。"],
  ];
  return routes.map(([target, engineLabel, modelLabel, providerLabel, available, unavailableReason]) => {
    const isClaudeOpus = target.model_id === "claude-opus-4-7";
    const isGpt54 = target.model_id === "gpt-5.4";
    const isSol = target.model_id === "gpt-5.6-sol";
    const reasoningEfforts = isClaudeOpus
      ? ["low", "medium", "high", "xhigh", "max"]
      : isSol
        ? [
            "low",
            "medium",
            "high",
            "xhigh",
            "max",
            ...(target.engine_id === "codex" ? ["ultra"] : []),
          ]
        : isGpt54
          ? ["low", "medium", "high", "xhigh"]
          : [];
    return {
      target: {
        ...target,
        reasoning_effort: reasoningEfforts.includes(target.reasoning_effort ?? "")
          ? target.reasoning_effort
          : null,
      },
      engine_label: engineLabel,
      model_label: modelLabel,
      provider_label: providerLabel,
      description: "用于视觉验收的固定路由",
      context_window: null,
      capabilities: [],
      reasoning_efforts: reasoningEfforts,
      default_reasoning_effort: reasoningEfforts.includes("high") ? "high" : null,
      supports_fast_mode: target.engine_id === "codex" && (isSol || isGpt54),
      available,
      unavailable_reason: unavailableReason,
    };
  });
}

export function demoProviderProfiles(): ProviderProfile[] {
  return [
    {
      id: "claude-cli",
      display_name: "Claude Code 订阅",
      kind: "claude-subscription",
      base_url: "",
      enabled: true,
      api_key_configured: false,
      requires_api_key: false,
      endpoint_editable: false,
      managed: true,
      max_output_tokens: 8192,
      deletable: false,
      supported_engines: ["claude"],
      models: [
        { id: "engine-default", display_name: "默认模型", enabled: true, registry_id: null, context_window: null, capabilities: [], supported_engines: ["claude"] },
        { id: "claude-opus-4-7", display_name: "Claude Opus 4.7", enabled: true, registry_id: "claude-opus-4-7", context_window: 1000000, capabilities: ["reasoning"], supported_engines: ["claude"] },
      ],
    },
    {
      id: "codex-cli",
      display_name: "Codex 订阅",
      kind: "codex-subscription",
      base_url: "",
      enabled: true,
      api_key_configured: false,
      requires_api_key: false,
      endpoint_editable: false,
      managed: true,
      max_output_tokens: 8192,
      deletable: false,
      supported_engines: ["codex"],
      models: [
        { id: "engine-default", display_name: "默认模型", enabled: true, registry_id: null, context_window: null, capabilities: [], supported_engines: ["codex"] },
        { id: "gpt-5.6-sol", display_name: "GPT-5.6-Sol", enabled: true, registry_id: "gpt-5.6-sol", context_window: 372000, capabilities: ["reasoning"], supported_engines: ["codex"] },
      ],
    },
    {
      id: "anthropic",
      display_name: "Anthropic",
      kind: "anthropic",
      base_url: "https://api.anthropic.com",
      enabled: true,
      api_key_configured: true,
      requires_api_key: true,
      endpoint_editable: false,
      managed: false,
      max_output_tokens: 8192,
      deletable: false,
      supported_engines: ["alva", "claude", "pi"],
      models: [{ id: "claude-opus-4-7", display_name: "Claude Opus 4.7", enabled: true, registry_id: "claude-opus-4-7", context_window: 200000, capabilities: ["reasoning"], supported_engines: ["alva", "claude", "pi"] }],
    },
    {
      id: "openai-responses",
      display_name: "OpenAI Responses",
      kind: "openai-responses",
      base_url: "https://api.openai.com/v1",
      enabled: true,
      api_key_configured: false,
      requires_api_key: true,
      endpoint_editable: false,
      managed: false,
      max_output_tokens: 8192,
      deletable: false,
      supported_engines: ["alva", "codex", "pi"],
      models: [{ id: "gpt-5.4", display_name: "GPT-5.4", enabled: true, registry_id: "gpt-5.4", context_window: 200000, capabilities: ["reasoning"], supported_engines: ["alva", "codex", "pi"] }],
    },
  ];
}

export const demoSessions: HubSessionSummary[] = [
  {
    product_session_id: "ps-demo-1",
    workspace: "/Users/dev/projects/alva-agent",
    workspace_kind: "project",
    active_branch_id: "b-claude",
    engine_id: "claude",
    model_id: "claude-sonnet-4-5",
    provider_id: "claude-cli",
    engine_session_id: "native-claude-1",
    status: "parked",
    updated_at: 1725000001000,
    last_prompt: "重构交接数据的脱敏过滤器",
    last_result: "已将 40 多种敏感信息形态合并为一次表驱动处理。",
    permission_profile: "workspace_write",
    include_full_transcript: false,
  },
  {
    product_session_id: "ps-demo-2",
    workspace: "/Users/dev/projects/other-repo",
    workspace_kind: "project",
    active_branch_id: "b-codex",
    engine_id: "codex",
    model_id: "codex-mini",
    provider_id: "codex-cli",
    engine_session_id: "native-codex-1",
    status: "active",
    updated_at: 1725000100000,
    last_prompt: "添加 Worker 队列状态接口",
    last_result: null,
    permission_profile: "workspace_write",
    include_full_transcript: true,
  },
  {
    product_session_id: "ps-demo-3",
    workspace: "/Users/dev/projects/scratch",
    workspace_kind: "session",
    active_branch_id: "b-alva",
    engine_id: "alva",
    model_id: "workspace-config",
    provider_id: "workspace-config",
    engine_session_id: null,
    status: "completed",
    updated_at: 1724999000000,
    last_prompt: "验证资源声明冲突规则的原型",
    last_result: "已验证包含 12 组冲突组合的矩阵。",
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
        prompt: "重构交接数据的脱敏过滤器",
        result: "已将 40 多种敏感信息形态合并为一次表驱动处理。",
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
              {
                // R36: a long tool output exercises reading-lane wrapping and
                // the Inspector expansion matrix.
                tool_call_id: "tc-1-log",
                tool_name: "bash",
                succeeded: true,
                duration_ms: 1840,
                path: null,
                tool_input: "{\"command\":\"cargo test --workspace\"}",
                tool_output: "running 412 tests\ntest redaction::tests::merges_luhn_variants ... ok\ntest redaction::tests::table_driven_iban ... ok\ntest redaction::tests::keep_paths_intact ... ok\ntest handoff::tests::redacts_transcript_bundles ... ok\ntest hub::tests::shared_clock_projection ... ok\n...\ntest result: ok. 412 passed; 0 failed; 18 ignored; finish in 6.41s",
              },
            ],
            started_at: 1724999000000,
            completed_at: 1724999100000,
          },
        ],
      },
      {
        // R36: a minimal greeting turn gives the small-usage/small-talk
        // visual state (short prompt, zero tool calls, tiny usage).
        turn_id: "turn-demo-3",
        prompt: "你好",
        result: "你好！需要我查看这个项目的哪一部分？",
        error: null,
        status: "completed",
        created_at: 1724999400000,
        completed_at: 1724999411000,
        attempts: [
          {
            attempt_id: "att-demo-4",
            turn_id: "turn-demo-3",
            branch_id: "b-claude",
            engine_id: "claude",
            status: "completed",
            failure_reason: null,
            usage: {
              input_tokens: 42,
              output_tokens: 17,
              total_cost_usd: 0.0001,
              duration_ms: 900,
              num_turns: 1,
            },
            side_effects: [],
            started_at: 1724999400000,
            completed_at: 1724999411000,
          },
        ],
      },
      {
        turn_id: "turn-demo-2",
        prompt: "将脱敏测试迁移到新表结构",
        result: null,
        error: "供应商触发速率限制：HTTP 429",
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
            failure_reason: "触发速率限制",
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
        account_source: "工作区供应商",
        billing_route: "工作区供应商账号",
        automatic_use_authorized: true,
        required_capabilities: [],
        reasoning_effort: "high",
        fast_mode: false,
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
        account_source: "Claude Code CLI 账号",
        billing_route: "Anthropic 账号",
        automatic_use_authorized: true,
        required_capabilities: [],
        reasoning_effort: "high",
        fast_mode: false,
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
        reason: "供应商触发速率限制：HTTP 429",
        user_goal: "重构交接数据的脱敏过滤器",
        latest_instruction: "将脱敏测试迁移到新表结构",
        accepted_decisions: ["保持敏感信息形态的表驱动实现"],
        completed_work: [
          { prompt: "重构脱敏过滤器", result: "已完成" },
        ],
        pending_work: [
          { prompt: "迁移测试", reason: "触发速率限制" },
        ],
        changed_files: ["src/redact.rs"],
        recent_commands: ["cargo test -p alva-hub-desktop"],
        verification_results: ["cargo test 已通过"],
        workspace_summary:
          "工作区（以此处内容为准；采信对话记录前请先检查工作区）：/Users/dev/projects/alva-agent",
        team_snapshot: null,
        transcript: [
          { prompt: "重构脱敏过滤器", result: "已完成" },
        ],
        session_memory: ["完成前始终运行 cargo test"],
        available_skills: [
          {
            name: "review",
            description: "合入前审查每一项变更",
            path: ".alva/skills/review/SKILL.md",
          },
        ],
        available_mcp: [
          {
            id: "filesystem",
            display_name: "文件系统",
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
        reason: "供应商触发速率限制：HTTP 429",
        created_at: 1724999220000,
      },
    ],
    pending_switch: null,
    fallback_policy: {
      targets: [engine("alva"), engine("claude"), engine("codex")],
      updated_at: 1724998000000,
    },
    memory: ["完成前始终运行 cargo test"],
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
        scope: "实现队列状态接口",
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
        scope: "为 Worker 接口编写 API 文档",
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
      status: "healthy",
      binary_path: "/usr/local/bin/codex",
      version: "0.147.0",
      minimum_version: "0.20.0",
      capabilities: ["streaming"],
      authenticated: true,
      stream_health: "untested",
      reasons: [],
    },
    {
      engine_id: "pi",
      display_name: "Pi",
      status: "healthy",
      binary_path: "/usr/local/bin/pi",
      version: "0.52.9",
      minimum_version: "0.1.0",
      capabilities: ["streaming", "resume"],
      authenticated: true,
      stream_health: "untested",
      reasons: [],
    },
  ];
}

export function demoFallbackPolicy(): FallbackPolicy {
  return {
    targets: [engine("alva"), engine("claude"), engine("codex"), engine("pi")],
    updated_at: 1724998000000,
  };
}

export function demoInspection(nativeSessionId = "native-alva-1"): HubInspectionSnapshot {
  return {
    product_session_id: "ps-demo-1",
    branch_id: "branch-demo-alva",
    root_native_session_id: "native-alva-1",
    engine_id: "alva",
    native_session_id: nativeSessionId,
    parent_session_id: nativeSessionId === "native-alva-1" ? null : "native-alva-1",
    high_water_seq: 12,
    live: false,
    capabilities: {
      level: "full",
      historical: true,
      live: true,
      raw_records: true,
      recursive_children: true,
      export_jsonl_bundle: true,
    },
    run: {
      total_duration_ms: 842,
      total_input_tokens: 820,
      total_output_tokens: 164,
      middleware_hooks: [{
        middleware_name: "security",
        hook: "on_agent_start",
        duration_ms: 2,
        outcome: "success",
        error: null,
        tool_call_id: null,
      }],
      user_messages: [{ before_turn_number: 1, text: "Inspect the workspace", timestamp_ms: 1725000000000 }],
      turns: [{
        turn_number: 1,
        duration_ms: 820,
        llm_call: {
          messages_sent: [{ role: "user", content: "Inspect the workspace" }],
          messages_sent_count: 1,
          response: { role: "assistant", content: "I will inspect the project." },
          input_tokens: 820,
          output_tokens: 164,
          duration_ms: 612,
          stop_reason: "tool_use",
          error_message: null,
          middleware_hooks: [{
            middleware_name: "security",
            hook: "wrap_llm_call",
            duration_ms: 614,
            outcome: "success",
            error: null,
            tool_call_id: null,
          }],
          first_event_ms: 31,
          time_to_first_output_ms: 48,
          time_to_first_token_ms: 48,
          stream_duration_ms: 610,
          stream_event_count: 18,
          visible_stream_event_count: 14,
          stream_transport: "stream",
          stream_outcome: "completed",
        },
        tool_calls: [{
          tool_call: { id: "tool-demo-1", name: "read_file", arguments: { path: "src/main.rs" } },
          result: { content: "…" },
          is_error: false,
          duration_ms: 104,
          middleware_hooks: [],
        }],
      }],
    },
    records: [
      { seq: 1, record_id: "r1", parent_record_id: null, timestamp: 1725000000000, category: "execution", type_name: "run_start", emitter: { kind: "runtime" }, payload: {} },
      { seq: 8, record_id: "r8", parent_record_id: "r3", timestamp: 1725000000610, category: "execution", type_name: "llm_stream_summary", emitter: { kind: "runtime" }, payload: { event_count: 18 } },
      { seq: 12, record_id: "r12", parent_record_id: "r1", timestamp: 1725000000842, category: "execution", type_name: "run_end", emitter: { kind: "runtime" }, payload: {} },
    ],
    children: nativeSessionId === "native-alva-1" ? [{
      child_session_id: "demo-native-child",
      parent_session_id: "native-alva-1",
      parent_run_id: "run-demo-1",
      spawn_tool_call_id: "tool-demo-spawn",
    }] : [],
  };
}
