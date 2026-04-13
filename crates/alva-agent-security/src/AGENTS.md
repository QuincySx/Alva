# alva-agent-security/src
> Agent 安全子系统的源码实现层：路径过滤、授权根目录、HITL 权限管理、macOS 沙箱配置

## 地位
`alva-agent-security` crate 的全部源码。对外通过 `lib.rs` 的 re-exports 提供 SecurityGuard、PermissionManager、SensitivePathFilter、AuthorizedRoots、SandboxConfig 等公共 API。被 `alva-host-native` 的 `SecurityMiddleware` 包装后作为中间件接入 Agent 执行管线。

## 逻辑
1. `guard.rs` 组合所有安全组件为统一安全门（SecurityGuard），工具执行前依次检查：
   - 敏感路径过滤（sensitive_paths）
   - 授权根目录校验（authorized_roots）
   - HITL 人工审批（permission）
2. `sensitive_paths.rs` 基于目录黑名单、文件扩展名、文件名、正则模式四级过滤敏感路径。
3. `authorized_roots.rs` 管理 Agent 可访问的目录集合，workspace 为主根，可追加额外根。
4. `permission.rs` 实现会话级 HITL 权限管理，支持 AllowOnce / AllowAlways / RejectOnce / RejectAlways 缓存决策，通过 oneshot channel 异步等待人工审批。
5. `sandbox.rs` 配置 macOS Seatbelt 沙箱 profile，提供四级沙箱模式（RestrictiveOpen / RestrictiveClosed / RestrictiveProxied / Permissive）。

## 约束
- SecurityGuard 的检查顺序不可调整：敏感路径 -> 授权根 -> HITL 权限，前置检查失败直接拒绝。
- SandboxMode 目前仅支持 macOS Seatbelt，跨平台需另行实现。
- PermissionManager 的 `AllowAlways` / `RejectAlways` 缓存以 tool name 为 key，整个会话生命周期内生效。
- AuthorizedRoots 的 workspace 路径在构造后不可变更。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | lib.rs | 声明所有安全模块并 re-export 公共 API |
| SecurityGuard | guard.rs | 统一安全门：组合敏感路径过滤、授权根校验、HITL 权限管理 |
| PermissionManager | permission.rs | 会话级 HITL 权限管理器，缓存 allow/deny 决策，异步审批流 |
| SensitivePathFilter | sensitive_paths.rs | 敏感路径过滤：目录黑名单、扩展名、文件名、正则模式四级匹配 |
| AuthorizedRoots | authorized_roots.rs | 管理 Agent 可访问的授权目录集合，默认为 workspace 根 |
| SandboxConfig | sandbox.rs | macOS Seatbelt 沙箱配置，四级沙箱模式控制 shell 命令执行限制 |
