#!/usr/bin/env python3
"""Automated test runner for alva-app-eval test cases.

Usage:
  1. Start mock:  python3 tests/mock_llm.py &
  2. Start eval:  RUST_LOG="warn,alva_kernel_core=debug,alva_llm_provider=debug,alva_host_native=debug,alva_agent_security=debug,alva_agent_tools=info,alva_app_core=info,alva_app_eval=info" cargo run -p alva-app-eval &
  3. Run tests:   python3 tests/run_tests.py
"""

import json, sys, time, urllib.request, os  # noqa: E401

BASE = "http://127.0.0.1:3000"
MOCK = "http://127.0.0.1:8999/v1"
WORKSPACE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

passed = 0
failed = 0
errors = []


def run(scenario, tools, middleware, max_iter=10, extra=None):
    """Fire a run and wait for completion."""
    body = {
        "provider": "openai", "model": "mock-model", "api_key": "sk-mock",
        "base_url": MOCK,
        "system_prompt": f"SCENARIO:{scenario}",
        "user_prompt": "run test",
        "tools": tools,
        "middleware": middleware,
        "max_iterations": max_iter,
        "workspace": WORKSPACE,
    }
    if extra:
        body.update(extra)
    data = json.dumps(body).encode()
    req = urllib.request.Request(f"{BASE}/api/run", data=data, headers={"Content-Type": "application/json"})
    resp = json.loads(urllib.request.urlopen(req).read())
    rid = resp["run_id"]
    # Wait for completion
    for _ in range(30):
        time.sleep(0.5)
        try:
            rec = json.loads(urllib.request.urlopen(f"{BASE}/api/records/{rid}").read())
            if isinstance(rec, dict) and "turns" in rec:
                logs = json.loads(urllib.request.urlopen(f"{BASE}/api/logs/{rid}").read())
                return rid, rec, logs
        except Exception:
            pass
    # Timeout — try one more time
    try:
        rec = json.loads(urllib.request.urlopen(f"{BASE}/api/records/{rid}").read())
        logs = json.loads(urllib.request.urlopen(f"{BASE}/api/logs/{rid}").read())
        return rid, rec, logs
    except Exception:
        return rid, None, []


def check(test_id, desc, cond, detail=""):
    global passed, failed, errors
    if cond:
        print(f"  ✅ {test_id}: {desc}")
        passed += 1
    else:
        print(f"  ❌ {test_id}: {desc}" + (f" — {detail}" if detail else ""))
        failed += 1
        errors.append(f"{test_id}: {desc} — {detail}")


def log_has(logs, keyword):
    return any(keyword.lower() in (l.get("message", "") + str(l.get("fields", {}))).lower() for l in logs)


def log_target_has(logs, target_part):
    return any(target_part in l.get("target", "") for l in logs)


# ═══════════════════════════════════════════════════════════════════
print("═══ 一、Agent 基础执行 ═══")
# ═══════════════════════════════════════════════════════════════════

# A1: pure text
print("A1: 纯文本（无工具调用）")
_, rec, logs = run("a1", [], ["loop_detection"])
check("A1.1", "turns=1", len(rec["turns"]) == 1)
check("A1.2", "stop_reason=end_turn", rec["turns"][0]["llm_call"]["stop_reason"] == "end_turn")
check("A1.3", "无 tool_calls", len(rec["turns"][0]["tool_calls"]) == 0)
check("A1.4", "token 统计", rec["total_input_tokens"] > 0 and rec["total_output_tokens"] > 0)

# A2: single tool
print("A2: 单工具调用")
_, rec, logs = run("a2", ["list_files"], ["loop_detection"])
check("A2.1", "turns=2", len(rec["turns"]) == 2)
check("A2.2", "T1 有 tool_call", len(rec["turns"][0]["tool_calls"]) == 1)
check("A2.3", "T1 tool=list_files", rec["turns"][0]["tool_calls"][0]["tool_call"]["name"] == "list_files")
check("A2.4", "T1 tool 成功", not rec["turns"][0]["tool_calls"][0]["is_error"])
check("A2.5", "T2 stop_reason=end_turn", rec["turns"][1]["llm_call"]["stop_reason"] == "end_turn")

# A3: multi-turn chain
print("A3: 多轮工具链")
_, rec, logs = run("a3", ["list_files", "read_file"], ["loop_detection", "tool_timeout"])
check("A3.1", "turns=3", len(rec["turns"]) == 3)
check("A3.2", "T1 list_files", rec["turns"][0]["tool_calls"][0]["tool_call"]["name"] == "list_files")
check("A3.3", "T2 read_file", rec["turns"][1]["tool_calls"][0]["tool_call"]["name"] == "read_file")
check("A3.4", "T3 文本", rec["turns"][2]["llm_call"]["stop_reason"] == "end_turn")
check("A3.5", "T2 read_file 结果非空", bool(rec["turns"][1]["tool_calls"][0].get("result")))

# A4: multi-tool one turn
print("A4: 单轮多工具")
_, rec, logs = run("a4", ["list_files"], ["loop_detection"])
check("A4.1", "turns=2", len(rec["turns"]) == 2)
check("A4.2", "T1 有 2 个 tool_calls", len(rec["turns"][0]["tool_calls"]) == 2)
t1_tools = [tc["tool_call"]["name"] for tc in rec["turns"][0]["tool_calls"]]
check("A4.3", "都是 list_files", all(t == "list_files" for t in t1_tools))
# Check different args
args = [json.dumps(tc["tool_call"]["arguments"]) for tc in rec["turns"][0]["tool_calls"]]
check("A4.4", "参数不同", args[0] != args[1], f"args={args}")

# A5: max_iterations
print("A5: 达到 max_iterations")
_, rec, logs = run("a5", ["list_files"], ["loop_detection"], max_iter=3)
check("A5.1", "turns≤3", len(rec["turns"]) <= 3)

# ═══════════════════════════════════════════════════════════════════
print("\n═══ 二、工具系统 ═══")
# ═══════════════════════════════════════════════════════════════════

# T1: basic tool success
print("T1: 工具成功（基本验证）")
_, rec, logs = run("a2", ["list_files"], ["loop_detection"])
t1_tc = rec["turns"][0]["tool_calls"][0] if rec["turns"][0]["tool_calls"] else None
check("T1.1", "is_error=false", t1_tc is not None and not t1_tc["is_error"])
check("T1.2", "result 非空", t1_tc is not None and t1_tc.get("result") is not None)
check("T1.3", "duration_ms≥0", t1_tc is not None and t1_tc["duration_ms"] >= 0)
if t1_tc and t1_tc.get("result"):
    rlen = sum(len(b.get("text", "")) for b in t1_tc["result"].get("content", []) if b.get("type") == "text")
    check("T1.4", "result 有内容", rlen > 0, f"result_len={rlen}")

# T2: nonexistent file (tool returns error)
print("T2: 工具执行错误（文件不存在）")
_, rec, logs = run("t2", ["read_file"], ["loop_detection"])
t1_tcs = rec["turns"][0]["tool_calls"]
if t1_tcs:
    check("T2.1", "is_error=true", t1_tcs[0]["is_error"])
    result_text = ""
    if t1_tcs[0].get("result"):
        for b in t1_tcs[0]["result"].get("content", []):
            if b.get("type") == "text":
                result_text += b.get("text", "")
    check("T2.2", "result 含错误信息", "not found" in result_text.lower() or "error" in result_text.lower() or "no such" in result_text.lower() or "denied" in result_text.lower() or "blocked" in result_text.lower(), result_text[:80])
else:
    # Tool arg parse error → caught at agent level, no tool_call record
    err = rec["turns"][0]["llm_call"].get("error_message", "")
    check("T2.1", "agent 级错误捕获", "error" in err.lower() or "tool" in err.lower(), err[:80])
    check("T2.2", "stop_reason=error 或 tool_use", rec["turns"][0]["llm_call"]["stop_reason"] in ("error", "tool_use"))

# T3: large result
print("T3: 工具大结果")
_, rec, logs = run("t3", ["grep_search"], ["loop_detection"])
if rec["turns"][0]["tool_calls"]:
    tc = rec["turns"][0]["tool_calls"][0]
    rlen = sum(len(b.get("text", "")) for b in (tc.get("result") or {}).get("content", []) if b.get("type") == "text")
    check("T3.1", "result>200字符", rlen > 200, f"result_len={rlen}")
    check("T3.2", "tool 成功", not tc["is_error"])

# T4: execute_shell
print("T4: execute_shell")
_, rec, logs = run("t4", ["execute_shell"], ["loop_detection", "tool_timeout"])
if rec["turns"][0]["tool_calls"]:
    tc = rec["turns"][0]["tool_calls"][0]
    check("T4.1", "tool=execute_shell", tc["tool_call"]["name"] == "execute_shell")
    check("T4.2", "成功", not tc["is_error"])
    result_text = ""
    if tc.get("result"):
        for b in tc["result"].get("content", []):
            if b.get("type") == "text":
                result_text += b.get("text", "")
    check("T4.3", "result 含输出", "hello" in result_text.lower(), result_text[:50])

# T5: file_edit
print("T5: create_file + file_edit")
_, rec, logs = run("t5", ["create_file", "file_edit"], ["loop_detection"])
check("T5.1", "turns≥2", len(rec["turns"]) >= 2)
if len(rec["turns"]) >= 2:
    check("T5.2", "T1 create_file 成功", not rec["turns"][0]["tool_calls"][0]["is_error"] if rec["turns"][0]["tool_calls"] else False)
    if rec["turns"][1]["tool_calls"]:
        check("T5.3", "T2 file_edit 有结果", rec["turns"][1]["tool_calls"][0].get("result") is not None)
    else:
        check("T5.3", "T2 有 tool_call", False)

# T6: create_file
print("T6: create_file")
_, rec, logs = run("t6", ["create_file"], ["loop_detection"])
if rec["turns"][0]["tool_calls"]:
    tc = rec["turns"][0]["tool_calls"][0]
    check("T6.1", "tool=create_file", tc["tool_call"]["name"] == "create_file")
    check("T6.2", "成功", not tc["is_error"])

# ═══════════════════════════════════════════════════════════════════
print("\n═══ 三、Middleware ═══")
# ═══════════════════════════════════════════════════════════════════

# M1: loop warn
print("M1: Loop Detection - 警告")
_, rec, logs = run("m1", ["list_files"], ["loop_detection"], max_iter=5)
check("M1.1", "turns≥3", len(rec["turns"]) >= 3)
check("M1.2", "日志含 warn threshold", log_has(logs, "warn threshold"))

# M2: loop hard limit
print("M2: Loop Detection - 强制终止")
_, rec, logs = run("m2", ["list_files"], ["loop_detection"], max_iter=10)
check("M2.1", "日志含 hard limit", log_has(logs, "hard limit"))
check("M2.2", "turns≤6", len(rec["turns"]) <= 6, f"turns={len(rec['turns'])}")

# M3: dangling tool call
print("M3: Dangling Tool Call 验证")
_, rec, logs = run("m3", ["list_files"], ["loop_detection", "dangling_tool_check"])
# The LLM returns a call to "nonexistent_tool_xyz" which isn't registered.
# DanglingToolCallMiddleware or agent-core should handle this gracefully.
check("M3.1", "有 turns", len(rec["turns"]) >= 1)
# Check for dangling/unknown tool handling in logs or record
t1 = rec["turns"][0]
has_error = t1["llm_call"].get("error_message") or any(tc["is_error"] for tc in t1.get("tool_calls", []))
has_dangling_log = log_has(logs, "dangling") or log_has(logs, "not found") or log_has(logs, "unknown tool") or log_has(logs, "nonexistent")
check("M3.2", "不存在的工具被处理", has_error or has_dangling_log,
      f"error={(t1['llm_call'].get('error_message') or '')[:60]}")

# M4: tool timeout (verify middleware hook exists)
print("M4: Tool Timeout Middleware")
_, rec, logs = run("a2", ["list_files"], ["tool_timeout"])
mw_hooks = [l for l in logs if "middleware hook" in l.get("message", "")]
timeout_hooks = [l for l in mw_hooks if "tool_timeout" in l.get("fields", {}).get("middleware", "")]
check("M4.1", "tool_timeout middleware hook 存在", len(timeout_hooks) > 0, f"timeout_hooks={len(timeout_hooks)}")
check("M4.2", "含 before_tool_call hook", any(l["fields"].get("hook") == "before_tool_call" for l in timeout_hooks))

# M5: compaction middleware fires
print("M5: Compaction Middleware")
_, rec, logs = run("m5", ["list_files"], ["compaction", "loop_detection"], max_iter=10)
check("M5.1", "turns>1", len(rec["turns"]) > 1)
mw_hooks = [l for l in logs if "middleware hook" in l.get("message", "")]
compaction_hooks = [l for l in mw_hooks if "compaction" in l.get("fields", {}).get("middleware", "")]
check("M5.2", "compaction middleware hook 存在", len(compaction_hooks) > 0, f"count={len(compaction_hooks)}")
check("M5.3", "含 before_llm_call hook", any(l["fields"].get("hook") == "before_llm_call" for l in compaction_hooks))

# M7: checkpoint middleware fires on write tool
print("M7: Checkpoint Middleware")
_, rec, logs = run("m7", ["create_file"], ["checkpoint", "loop_detection"])
check("M7.1", "turns≥1", len(rec["turns"]) >= 1)
mw_hooks = [l for l in logs if "middleware hook" in l.get("message", "")]
checkpoint_hooks = [l for l in mw_hooks if "checkpoint" in l.get("fields", {}).get("middleware", "")]
check("M7.2", "checkpoint middleware hook 存在", len(checkpoint_hooks) > 0, f"count={len(checkpoint_hooks)}")
check("M7.3", "含 before_tool_call hook", any(l["fields"].get("hook") == "before_tool_call" for l in checkpoint_hooks))

# M6: middleware hook timing
print("M6: Middleware Hook 计时")
_, rec, logs = run("a2", ["list_files"], ["loop_detection", "dangling_tool_check", "tool_timeout", "compaction", "checkpoint"])
mw_hooks = [l for l in logs if "middleware hook" in l.get("message", "")]
check("M6.1", "middleware hook 日志存在", len(mw_hooks) > 0, f"count={len(mw_hooks)}")
if mw_hooks:
    mw_names = set(l["fields"].get("middleware", "") for l in mw_hooks)
    check("M6.2", "含 loop_detection", any("loop" in n for n in mw_names), str(mw_names))
    check("M6.3", "含 compaction", any("compaction" in n for n in mw_names), str(mw_names))
    check("M6.4", "含 security", any("security" in n for n in mw_names), str(mw_names))
    hook_types = set(l["fields"].get("hook", "") for l in mw_hooks)
    check("M6.5", "含 on_agent_start", "on_agent_start" in hook_types, str(hook_types))
    check("M6.6", "含 before_tool_call", "before_tool_call" in hook_types, str(hook_types))
    check("M6.7", "含 after_tool_call", "after_tool_call" in hook_types, str(hook_types))
    check("M6.8", "含 duration_ms 字段", all("duration_ms" in l["fields"] for l in mw_hooks))

# ═══════════════════════════════════════════════════════════════════
print("\n═══ 四、Security / 权限 ═══")
# ═══════════════════════════════════════════════════════════════════

print("S1: 自动审批")
_, rec, logs = run("s1", ["execute_shell"], ["loop_detection", "tool_timeout"])
check("S1.1", "execute_shell 成功", not rec["turns"][0]["tool_calls"][0]["is_error"] if rec["turns"][0]["tool_calls"] else False)
check("S1.2", "日志含 auto-approving", log_has(logs, "auto-approving"))
approval_logs = [l for l in logs if "auto-approving" in l.get("message", "")]
if approval_logs:
    check("S1.3", "审批日志含 tool 名", "execute_shell" in str(approval_logs[0].get("fields", {})))
    check("S1.4", "审批日志含 request_id", "request_id" in approval_logs[0].get("fields", {}))

print("S2: Security Middleware Hook")
check("S2.1", "middleware hook 含 security", any("security" in l.get("fields", {}).get("middleware", "") for l in logs if "middleware hook" in l.get("message", "")))

# ═══════════════════════════════════════════════════════════════════
print("\n═══ 五、错误处理 ═══")
# ═══════════════════════════════════════════════════════════════════

# E1: LLM error
print("E1: LLM 错误（空响应）")
_, rec, logs = run("e1", ["list_files"], ["loop_detection"])
if rec:
    check("E1.1", "有 turns", len(rec["turns"]) >= 1)
    if rec["turns"]:
        llm = rec["turns"][0]["llm_call"]
        check("E1.2", "stop_reason=error", llm["stop_reason"] == "error")
        check("E1.3", "error_message 非空",
              bool(llm.get("error_message") or rec.get("turns", [{}])[-1].get("llm_call", {}).get("error_message")))
else:
    check("E1.1", "record 存在", False, "no record")

# E2: LLM empty SSE body
print("E2: LLM 返回空 SSE body")
_, rec, logs = run("e2", ["list_files"], ["loop_detection"])
if rec:
    check("E2.1", "有 turns", len(rec["turns"]) >= 1)
    if rec["turns"]:
        llm = rec["turns"][0]["llm_call"]
        check("E2.2", "stop_reason=error", llm["stop_reason"] == "error")

# E3: tool error
print("E3: 工具执行错误")
_, rec, logs = run("e3", ["read_file"], ["loop_detection"])
if rec["turns"][0]["tool_calls"]:
    check("E3.1", "is_error=true", rec["turns"][0]["tool_calls"][0]["is_error"])

# E4: error then recovery
print("E4: 错误后恢复")
_, rec, logs = run("e4", ["read_file", "list_files"], ["loop_detection"])
check("E4.1", "turns≥2", len(rec["turns"]) >= 2)
if len(rec["turns"]) >= 1:
    check("E4.2", "T1 有错误", rec["turns"][0]["tool_calls"][0]["is_error"] if rec["turns"][0]["tool_calls"] else False)

# ═══════════════════════════════════════════════════════════════════
print("\n═══ 六、数据完整性 ═══")
# ═══════════════════════════════════════════════════════════════════

print("D1: ConfigSnapshot")
_, rec, logs = run("a3", ["list_files", "read_file", "grep_search"],
                   ["loop_detection", "dangling_tool_check", "tool_timeout"])
snap = rec["config_snapshot"]
check("D1.1", "model_id 非空", bool(snap["model_id"]))
check("D1.2", "tool_names 正确", len(snap["tool_names"]) == 3)
check("D1.3", "tool_definitions 有 schema", len(snap["tool_definitions"]) == 3)
if snap["tool_definitions"]:
    d = snap["tool_definitions"][0]
    check("D1.4", "definition 含 name", bool(d.get("name")))
    check("D1.5", "definition 含 description", bool(d.get("description")))
    check("D1.6", "definition 含 parameters", bool(d.get("parameters")))
check("D1.7", "middleware_names 正确", len(snap["middleware_names"]) == 3)
check("D1.8", "extension_names 存在", "extension_names" in snap)
check("D1.9", "max_iterations 正确", snap["max_iterations"] == 10)

print("D4: Tool Definition Schema 完整性")
for i, d in enumerate(snap.get("tool_definitions", [])):
    check(f"D4.{i+1}a", f"def[{i}] 含 parameters.type", "type" in d.get("parameters", {}), str(d.get("parameters", {}).get("type")))
    check(f"D4.{i+1}b", f"def[{i}] 含 parameters.properties", "properties" in d.get("parameters", {}))

print("D2: Token 统计")
total_in = sum(t["llm_call"]["input_tokens"] for t in rec["turns"])
total_out = sum(t["llm_call"]["output_tokens"] for t in rec["turns"])
check("D2.1", "input_tokens 累加正确", rec["total_input_tokens"] == total_in, f"{rec['total_input_tokens']} vs {total_in}")
check("D2.2", "output_tokens 累加正确", rec["total_output_tokens"] == total_out, f"{rec['total_output_tokens']} vs {total_out}")

print("D3: 时间统计")
check("D3.1", "total_duration_ms>0", rec["total_duration_ms"] > 0)
for i, t in enumerate(rec["turns"]):
    check(f"D3.{i+2}", f"T{i+1} duration_ms≥0", t["duration_ms"] >= 0)

print("D5: 日志捕获覆盖")
targets = set(l["target"].split("::")[0] for l in logs)
check("D5.1", "含 alva_kernel_core", "alva_kernel_core" in targets, str(targets))
check("D5.2", "含 alva_llm_provider", "alva_llm_provider" in targets, str(targets))

print("D6: Middleware Hook 日志")
mw_hooks = [l for l in logs if "middleware hook" in l.get("message", "")]
check("D6.1", "hook 日志存在", len(mw_hooks) > 0)
if mw_hooks:
    check("D6.2", "含 middleware 字段", all("middleware" in l["fields"] for l in mw_hooks))
    check("D6.3", "含 hook 字段", all("hook" in l["fields"] for l in mw_hooks))
    check("D6.4", "含 duration_ms", all("duration_ms" in l["fields"] for l in mw_hooks))

# D8: concurrent runs
print("D8: 并发 run 日志隔离")
body_a = json.dumps({
    "provider": "openai", "model": "mock-model", "api_key": "sk-mock",
    "base_url": MOCK, "system_prompt": "SCENARIO:d8a", "user_prompt": "run",
    "tools": ["list_files"], "middleware": ["loop_detection"], "max_iterations": 5, "workspace": WORKSPACE
}).encode()
body_b = json.dumps({
    "provider": "openai", "model": "mock-model", "api_key": "sk-mock",
    "base_url": MOCK, "system_prompt": "SCENARIO:d8b", "user_prompt": "run",
    "tools": ["list_files"], "middleware": ["loop_detection"], "max_iterations": 5, "workspace": WORKSPACE
}).encode()
req_a = urllib.request.Request(f"{BASE}/api/run", data=body_a, headers={"Content-Type": "application/json"})
req_b = urllib.request.Request(f"{BASE}/api/run", data=body_b, headers={"Content-Type": "application/json"})
resp_a = json.loads(urllib.request.urlopen(req_a).read())
resp_b = json.loads(urllib.request.urlopen(req_b).read())
time.sleep(5)
logs_a = json.loads(urllib.request.urlopen(f"{BASE}/api/logs/{resp_a['run_id']}").read())
logs_b = json.loads(urllib.request.urlopen(f"{BASE}/api/logs/{resp_b['run_id']}").read())
check("D8.1", "run A 有日志", len(logs_a) > 0, f"logs_a={len(logs_a)}")
check("D8.2", "run B 有日志", len(logs_b) > 0, f"logs_b={len(logs_b)}")

# ═══════════════════════════════════════════════════════════════════
print("\n═══ 七、Sub-Agent ═══")
# ═══════════════════════════════════════════════════════════════════

print("SA1: Sub-agent spawn + complete")
_, rec, logs = run("sa1",
                   ["list_files", "read_file"],
                   ["loop_detection", "tool_timeout"],
                   max_iter=10,
                   extra={"enable_sub_agents": True})

if rec:
    check("SA1.1", "turns≥1", len(rec["turns"]) >= 1)

    # Parent should have called the "agent" tool
    t1_tcs = rec["turns"][0]["tool_calls"] if rec["turns"] else []
    has_agent_call = any(tc["tool_call"]["name"] == "agent" for tc in t1_tcs)
    check("SA1.2", "T1 调用 agent 工具", has_agent_call)

    if has_agent_call:
        agent_tc = next(tc for tc in t1_tcs if tc["tool_call"]["name"] == "agent")
        check("SA1.3", "agent 工具有结果", agent_tc.get("result") is not None)
        check("SA1.4", "agent 工具 duration>0", agent_tc["duration_ms"] > 0, f"dur={agent_tc['duration_ms']}")

        # Check args contain task and role
        args = agent_tc["tool_call"]["arguments"]
        check("SA1.5", "args 含 task", "task" in args)
        check("SA1.6", "args 含 role", "role" in args)

    # Check tracing logs for sub-agent events
    check("SA1.7", "日志含 sub-agent spawned", log_has(logs, "sub-agent spawned"))
    check("SA1.8", "日志含 sub-agent completed", log_has(logs, "sub-agent completed"))

    # Check sub-agent log fields
    spawn_logs = [l for l in logs if "sub-agent spawned" in l.get("message", "")]
    if spawn_logs:
        fields = spawn_logs[0].get("fields", {})
        check("SA1.9", "spawn 日志含 depth", "depth" in fields, str(fields))
        check("SA1.10", "spawn 日志含 parent_scope_id", "parent_scope_id" in fields, str(fields))
        check("SA1.11", "spawn 日志含 sub_agent_role", "sub_agent_role" in fields, str(fields))

    complete_logs = [l for l in logs if "sub-agent completed" in l.get("message", "")]
    if complete_logs:
        fields = complete_logs[0].get("fields", {})
        check("SA1.12", "complete 日志含 success", "success" in fields, str(fields))
        check("SA1.13", "complete 日志含 output_len", "output_len" in fields, str(fields))

    # Child agent's LLM calls should appear in logs (from alva_kernel_core::run)
    child_turns = [l for l in logs if "turn completed" in l.get("message", "")]
    check("SA1.14", "日志含子 agent 的 turn completed", len(child_turns) >= 2,
          f"child_turns={len(child_turns)} (expect ≥2: parent+child)")

    # SA2: sub-agent complete details
    print("SA2: Sub-agent complete 细节")
    if complete_logs:
        fields = complete_logs[0].get("fields", {})
        check("SA2.1", "success=true", fields.get("success") == "true", str(fields.get("success")))
        check("SA2.2", "output_len>0", int(fields.get("output_len", "0")) > 0, fields.get("output_len"))
        check("SA2.3", "depth=1", fields.get("depth") == "1", fields.get("depth"))
    else:
        check("SA2.1", "sub-agent completed 日志存在", False, "no complete logs")

    # Check child's tool execution in logs
    child_tool_logs = [l for l in logs if "tool execution completed" in l.get("message", "")]
    parent_tool_logs = [l for l in child_tool_logs if l.get("fields", {}).get("tool") == "agent"]
    check("SA2.4", "parent 的 agent tool 执行日志", len(parent_tool_logs) > 0)
else:
    check("SA1.1", "record 存在", False, "no record")

# ═══════════════════════════════════════════════════════════════════
print("\n" + "═" * 60)
print(f"  总计: {passed + failed} 测试, ✅ {passed} 通过, ❌ {failed} 失败")
print("═" * 60)

if errors:
    print("\n失败详情:")
    for e in errors:
        print(f"  ❌ {e}")

sys.exit(0 if failed == 0 else 1)
