# Model Eval Suite

> Alva 的大模型能力评测套件：用同一组 capability case 同时做 CI 功能回归和真实模型兼容性/稳定性实验。

## 目标

这套 eval 解决两个问题：

1. **CI 功能测试**：不依赖 API key，用 `MockLanguageModel` 精确发起工具调用，验证工具、插件装配、middleware、权限自动放行、报告生成都没坏。
2. **模型兼容性测试**：接入真实模型，让模型自己读任务、选择工具、串联多步调用，测试模型是否会正确用工具，以及我们的工具 schema / prompt / runtime 是否让它好用。

## 入口

CI 功能测试：

```bash
cargo test -p alva-app-core --test agent_capabilities mock_capability_suite -- --nocapture
```

真实模型测试：

```bash
ALVA_TEST_API_KEY=... \
ALVA_TEST_MODEL=deepseek-v4-flash \
ALVA_TEST_BASE_URL=https://api.example.com/v1 \
ALVA_WRITE_CAPABILITY_REPORT=1 \
cargo test -p alva-app-core --test agent_capabilities real_capability_suite -- --nocapture
```

OpenAI-compatible chat provider 是默认分支；`ALVA_TEST_KIND` 只在需要
`anthropic` / `gemini` / `openai-responses` 时设置。

重复采样，用来判断随机性和稳定性：

```bash
ALVA_TEST_REPEAT=3 \
ALVA_WRITE_CAPABILITY_REPORT=1 \
cargo test -p alva-app-core --test agent_capabilities real_capability_suite -- --nocapture
```

组件 A/B：

```bash
ALVA_TEST_COMPONENTS=core,shell,permission,skills,web \
ALVA_WRITE_CAPABILITY_REPORT=1 \
cargo test -p alva-app-core --test agent_capabilities real_capability_suite -- --nocapture
```

## 报告

默认不写报告。设置 `ALVA_WRITE_CAPABILITY_REPORT=1` 后写到：

```text
crates/alva-app-core/tests/reports/
```

人看的完整报告：

- `run-<timestamp>-<suite>-<model>.json`：完整 case、task、assertion、trace、final_text、失败归因。
- `viewer.html`：双击打开，读取 `data.js`，不需要本地 server。

Agent 看的精简报告：

- `agent-summary-<timestamp>-<suite>-<model>.json`
- `latest-agent-summary.json`

`latest-agent-summary.json` 是后续自动迭代入口。它包含：

- `summary`：总通过率、耗时、失败数。
- `failure_counts`：按失败类型聚合。
- `case_stability`：同一 case 多次 repeat 后的 pass rate，低 pass rate 排前面。
- `failures`：失败 case 的 compact 诊断信息。
- `next_actions`：按 owner 给出下一步方向。

## 失败归因

当前归因是保守启发式，不替代人工判断：

| kind | owner | 含义 |
|------|-------|------|
| `model_no_tool_call` | `model` | 任务需要某工具，但模型没有调用。优先调工具描述、任务 prompt、system prompt。 |
| `tool_execution_error` | `tool` | 工具执行了，但返回 `is_error=true`。优先查工具入参、权限、文件环境。 |
| `runtime_error` | `runtime` | AgentEnd 带 error。优先查 loop、middleware、provider transport。 |
| `timeout` | `runtime` | 测试超时。优先查 HITL、死循环、provider hang。 |
| `assertion_failed` | `assertion` | 工具/模型跑了，但没满足测试不变量。需要判断是测试期望错，还是行为真的错。 |

## 推荐工作流

1. PR/本地开发先跑 mock suite；它失败时不要调 prompt，先修功能。
2. 模型升级、工具 schema 改动、system prompt 改动后跑 real suite。
3. 对随机失败设置 `ALVA_TEST_REPEAT=3` 或更高，看 `case_stability`，不要凭单次失败判断回滚。
4. Agent 自动继续时先读 `latest-agent-summary.json`，只处理最低 pass rate 或最高 failure count 的问题。
5. 改 prompt/schema 后重新跑同一模型、同一 component set、同一 repeat count；如果 pass rate 下降，回退最近一次 prompt/schema 变更。

## Headless CLI (`-p`) 与权限模式

eval suite 验证工具/组件；要手动 headless 驱动同一个 agent（单 prompt、流式 stdout、退出码），用 `alva -p`：

```bash
alva -p "summarize README.md"
echo "explain this error" | alva -p          # prompt 从 stdin 读
```

`-p` 是非交互的，**没有人能回答权限提示**。需要审批的工具（默认 `Ask` 模式下包括所有 `execute_shell`）会被 **fail-closed 拒绝**并打印原因，绝不挂起。用 `--permission-mode` 选策略：

| MODE | 行为 | 适用 |
|------|------|------|
| `ask`（默认） | 写/执行工具需审批；`-p` 下＝拒绝并提示改用更宽模式 | 默认安全 |
| `accept-edits` | 自动放行文件写；shell 仍受控 | 半自动 |
| `accept-shell` | 分类器判定安全/未知的 shell 自动放行，破坏性命令拦截 | **headless/沙箱测试首选** |
| `plan` | 只读，禁止写文件与执行命令 | 只分析 |
| `bypass` | 全放行、不提示（"dangerously skip permissions"，假设有沙箱） | CI / 沙箱内 |

```bash
alva -p --permission-mode accept-shell "run the test suite and report failures"
alva -p --permission-mode bypass "format the repo"     # 仅限 CI / 沙箱
```

测"权限拦截对不对"这类用例时：`accept-shell` 下跑破坏性命令应被拒（owner=`tool`/`runtime`），普通命令应放行；`ask` 下 `-p` 跑任何 shell 应得到拒绝提示而非挂起。完整 flag 见 `alva --help`。
