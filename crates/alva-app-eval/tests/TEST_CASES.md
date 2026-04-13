# alva-app-eval 全量测试用例

## 一、Agent 基础执行

| ID | 用例 | Mock 行为 | 验证点 |
|----|------|-----------|--------|
| A1 | 纯文本（无工具调用） | 直接返回文本 | turns=1, stop_reason=end_turn, 无 tool_calls |
| A2 | 单工具调用 | 调 list_files → 文本总结 | turns=2, T1 有 1 个 tool_call, T2 stop_reason=end_turn |
| A3 | 多轮工具链 | list_files → read_file → 文本 | turns=3, 每轮 tool 参数和结果正确 |
| A4 | 单轮多工具 | 一次返回 2 个 tool_call | T1 有 2 个 tool_calls, 各自独立结果 |
| A5 | 达到 max_iterations | 持续返回 tool call | turns=max_iterations, 最后一轮无 tool 执行 |

## 二、工具系统

| ID | 用例 | Mock 行为 | 验证点 |
|----|------|-----------|--------|
| T1 | 工具成功 | 调 list_files(path=".") | is_error=false, result 非空, duration_ms>0 |
| T2 | 工具参数错误 | 调 read_file(wrong_param="x") | is_error=true, result 含错误信息 |
| T3 | 工具大结果 | 调 grep_search(pattern="pub fn") | result 字符数>500, token 计数正确 |
| T4 | execute_shell | 调 execute_shell(command="echo hi") | 工具执行成功, result="hi\n" |
| T5 | file_edit | 调 file_edit 修改临时文件 | 文件内容改变, result 含确认 |
| T6 | create_file | 调 create_file 创建临时文件 | 文件存在, result 含路径 |

## 三、Middleware

| ID | 用例 | Mock 行为 | 验证点 |
|----|------|-----------|--------|
| M1 | Loop Detection - 警告 | 连续 3 次相同 tool call | 日志含 "warn threshold", count=3 |
| M2 | Loop Detection - 强制终止 | 连续 5 次相同 tool call | 日志含 "hard limit", tool_calls 被 strip |
| M3 | Dangling Tool Call | 返回格式正确的 tool call | middleware hook 日志含 dangling_tool_call |
| M4 | Tool Timeout | 注册工具, mock 不返回 | tool_timeout middleware 触发（需长时间等待） |
| M5 | Compaction | 发送大量消息填满 context | 日志含 compaction 事件（需设置 context_window） |
| M6 | Middleware Hook 计时 | 任意正常流程 | 日志含每个 middleware 的 hook + duration_ms |
| M7 | Checkpoint | 调用写工具(create_file) | checkpoint middleware hook 日志出现 |

## 四、Security / 权限

| ID | 用例 | Mock 行为 | 验证点 |
|----|------|-----------|--------|
| S1 | 自动审批 | 调 execute_shell | 日志含 "auto-approving", tool+request_id+args |
| S2 | Security Middleware Hook | 任意工具调用 | middleware hook 日志含 security 条目 |

## 五、错误处理

| ID | 用例 | Mock 行为 | 验证点 |
|----|------|-----------|--------|
| E1 | LLM HTTP 错误 | 返回 HTTP 500 | stop_reason=error, error_message 非空 |
| E2 | LLM 返回空 body | 返回 200 + 空 SSE | stop_reason=error 或 streaming fallback |
| E3 | 工具执行错误 | 错误参数导致工具失败 | is_error=true, error 信息在 result 中 |
| E4 | 错误后恢复 | 第1次错误, 第2次成功 | turns≥2, T1 有 error, 后续 turn 正常 |

## 六、数据完整性

| ID | 用例 | 验证点 |
|----|------|--------|
| D1 | ConfigSnapshot | model_id, tool_names, tool_definitions(含 schema), middleware_names, extension_names, max_iterations 全部非空 |
| D2 | Token 统计 | total_input_tokens = 各 turn input 之和, total_output_tokens 同理 |
| D3 | 时间统计 | total_duration_ms > 0, 每个 turn.duration_ms > 0, LLM duration > 0 |
| D4 | Tool Definition Schema | tool_definitions 中每个 def 有 name + description + parameters(JSON Schema) |
| D5 | 日志捕获覆盖 | 日志 target 包含 alva_kernel_core, alva_llm_provider; debug 级别含 alva_host_native |
| D6 | Middleware Hook 日志 | 每个注册的 middleware 在每个 hook 阶段有记录 |
| D8 | 并发 run 日志隔离 | 两个同时运行的 run 各自有独立的日志 |

## 七、Sub-Agent

| ID | 用例 | Mock 行为 | 验证点 |
|----|------|-----------|--------|
| SA1 | Sub-agent spawn | enable_sub_agents=true, 调 agent tool | 日志含 "sub-agent spawned" + depth + parent_scope_id |
| SA2 | Sub-agent complete | sub-agent 执行完毕 | 日志含 "sub-agent completed" + success + output_len |
