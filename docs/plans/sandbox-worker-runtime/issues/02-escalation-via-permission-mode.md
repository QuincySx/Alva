# 02 — 升级通道：升级请求 = 工具调用走 PermissionMode

**What to build:** worker 遇到能力缺口（如需要跑测试/构建）时，发出一个"升级请求"工具调用，请求宿主在沙箱外执行指定命令。该请求作为普通工具调用进入现有 PermissionMode 审批流：Interactive 弹给用户逐条批、Auto 走分类器判安全性、Bypass 直接执行。执行结果（stdout/exit code）作为工具结果喂回 worker。不发明任何专用审批机制。

**Blocked by:** 01 — 修子 agent HITL 绕过洞（审批体系必须先不可绕，升级通道才有兜底意义）。

**Status:** implemented (pending review)

- [x] `request_escalation` 随 native `ShellPlugin` 注册，参数显式包含 `command` 与 `cwd`
- [x] 五种实际 PermissionMode 各有 golden：Ask 挂起并覆盖批准/拒绝；AcceptShell 放行安全/未知、拦 Destructive；Bypass 直行；AcceptEdits 仍审批；Plan 不执行
- [x] stdout、stderr、exit code 不截断地喂回 worker 上下文，worker 可基于失败输出继续迭代
- [x] 脚本化 mock worker 端到端演示："改文件→升级请求跑测试→按结果修复→复测"闭环
- [x] 子 agent 调用 `request_escalation` 继承父审批门，拒绝后无副作用

## 实现契约

实际 app 模式映射为：Interactive=`Ask`，Auto=`AcceptShell`，另有
`AcceptEdits`、`Plan`、`Bypass`。升级工具通过 `Tool::check_permissions`
返回 `Ask`；`SecurityMiddleware` 将该声明合入原有 `SecurityGuard` 流程，
因此继续复用 PermissionMode、BashClassifier、ApprovalNotifier、pending action
和 PermissionManager，没有第二套审批协议。

native 下，升级工具与 `execute_shell` 的执行能力实质相同：两者最终都在本机
shell 执行命令。差别是语义身份和可替换边界：`execute_shell` 表示 agent 直接
使用本地 shell；`request_escalation` 表示 worker 请求宿主越过其隔离边界执行，
且其执行动作由 `EscalationExecutor` 注入。native 默认实现仅委托现有
`ToolFs::exec`，没有在工具中绑定 `std::process::Command`。

Ticket 08 需要实现一个 WASI host-import 版 `EscalationExecutor`，在 guest 中以
`RequestEscalationTool::new(executor)` 注册，并在宿主侧执行权限审批、路径翻译和
命令执行。本票不把工具注册进 wasm guest，也不新增 guest/host ABI。
