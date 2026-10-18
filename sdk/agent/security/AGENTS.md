# L3 · sdk/agent/security
> 可替换的路径授权、敏感数据与会话 HITL 政策，实际 OS 沙箱由宿主承担。

## 地位
SDK 能力库；标准库 + kernel/wire，不依赖 app、host、UI 或平台原生代码。

## 逻辑
Guard 顺序固定：原始/清理路径敏感过滤、授权根、显式 host resolver、解析后再次敏感/根检查，最后 HITL。
PermissionManager 按实际 session/tool 缓存允许/拒绝；Once 不缓存，Session 只在该会话生效。
Once不会跨执行缓存，但真实session/run/call/tool+完整Effect指纹的同一invocation可合并重复授权层；资源顺序不影响指纹。
指纹也绑定完整arguments SHA256及有效UTF-8有界InputSummary；同call改参必须重新审批。ClearSession使用每会话generation，迟到standalone授权不复活。
PermissionRules为immutable显式snapshot，支持name/input glob与deny优先；显式Ask不能被自动模式或缓存跳过，ReadOnly规则仅元数据不授予未知工具免审。
single-flight+completed memo有界4096，仅逐出completed；取消/infra失败不缓存，mode/session失效令迟到决定不能重新授权。
Plan 只允许读取，AcceptEdits 可直接写，其余执行/网络/未知效果要求显式审批；Ask 没有 handler 则拒绝。

## 约束
CheckPath 的 resolver 必须由宿主提供；纯词法校验不宣称防符号链接/TOCTOU/OS sandbox。
取消后到达的审批不得授权未来调用。实际 filesystem handle/open、Seatbelt/Landlock 不在此库。
NewGuard拒绝typed-nil host resolver，普通nil仍可构造无文件能力guard；缺失或typed-nil审批handler保持ErrApprovalRequired语义。两个函数adapter的nil值直接调用返回错误。resolver回调结束后再次检查取消，拒绝返回路径并保留真实resolver错误。
Plugin wrapper 在 extension/builtin；app 仅装配默认配方，不复制本库政策。
PermissionManager.ModeForSession/SetSessionMode仅供可信host显式控制单会话，失效该session generation/cache/flights而不撤父/兄弟；SetMode明确整体重置、ClearSession回落base。此map不拥有任务树/直接父授权ceiling、不构成审批，产品必须独立检查策略切换；模型侧exit_plan_mode在真正独立policy批准与父ceiling接线前不能注册为假EffectRead免审。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Permissions | `permission.go` | 模式、审批 callback、会话缓存与取消 |
| Explicit rules | `rules.go`, `rules_test.go` | 有界规则快照、参数通配、deny/ask优先与不可伪造只读 |
| Security guard | `guard.go` | 敏感路径、授权根、宿主 resolver 与审批的统一闸门 |
| Regression | `security_test.go` | 越界/敏感/解析逃逸、缓存隔离、迟到审批 |
| Invocation regression | `coalesce_test.go` | Once层间去重、16并发single-flight、执行/效果隔离与失效 |
| Session modes | `session_mode_test.go` | mode/cache/flight仅目标会话失效，父/兄弟不受影响及显式全局重置 |
| Callback boundaries | `boundary_test.go` | typed-nil pointer/func拒绝、optional nil保留、nil adapter报错与resolver取消/错误保留 |
