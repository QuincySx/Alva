# alva-agent-security/src
> Agent 安全子系统的源码实现层：路径过滤、授权根目录、HITL 权限管理、macOS Seatbelt 与 Linux Landlock 配置

## 地位
`alva-agent-security` crate 的全部源码。对外通过 `lib.rs` 的 re-exports 提供 SecurityGuard、PermissionManager、SensitivePathFilter、AuthorizedRoots、SandboxConfig 等公共 API；以及 `middleware/` 子模块下的 `SecurityMiddleware` + `PlanModeMiddleware`（Phase 2 从 host-native 搬来），将这些安全组件适配为 `alva-kernel-core::middleware::Middleware` trait 实现，直接接入 Agent 执行管线。

## 逻辑
1. `guard.rs` 组合所有安全组件为统一安全门（SecurityGuard），`SecurityMiddleware` 先读取工具自身的 `Tool::check_permissions` 声明，再依次检查：
   - 敏感路径过滤（sensitive_paths）
   - 授权根目录校验（authorized_roots）
   - HITL 人工审批（permission）
2. `sensitive_paths.rs` 基于目录黑名单、文件扩展名、文件名、正则模式四级过滤敏感路径。
3. `authorized_roots.rs` 管理 Agent 可访问的目录集合，workspace 为主根，可追加额外根。
4. `permission.rs` 实现会话级 HITL 权限管理，支持 AllowOnce / AllowAlways / RejectOnce / RejectAlways 缓存决策，通过 oneshot channel 异步等待人工审批。
5. `sandbox.rs` 区分平台可用性与本次运行是否实际受限；macOS os worker 的 canonical 路径通过 `-D` 注入 Seatbelt，Linux os worker 则在 ABI v1 HardRequirement 下添加 canonical Landlock 读写/只读规则并仅接受 FullyEnforced。

## 约束
- SecurityGuard 的检查顺序不可调整：敏感路径 -> 授权根 -> HITL 权限，前置检查失败直接拒绝。
- `is_enforced()` 是实例状态，普通 macOS/Linux 配置不得仅凭平台返回 true；Linux Landlock 必须在创建 sibling threads 前进入，内核/LSM/ABI 不足不可裸跑。
- PermissionManager 的 `AllowAlways` / `RejectAlways` 缓存以 tool name 为 key，整个会话生命周期内生效。
- AuthorizedRoots 的 workspace 路径在构造后不可变更。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | lib.rs | 声明所有安全模块并 re-export 公共 API |
| Middleware Module | middleware/ | `SecurityMiddleware` + `PlanModeMiddleware`：把安全策略适配为 kernel `Middleware` trait |
| SecurityGuard | guard.rs | 统一安全门：合并 ToolPermissionResult 与配置危险工具，再执行模式、分类器、路径与 HITL 裁决 |
| PermissionManager | permission.rs | 会话级 HITL 权限管理器，缓存 allow/deny 决策，异步审批流 |
| SensitivePathFilter | sensitive_paths.rs | 敏感路径过滤：目录黑名单、扩展名、文件名、正则模式四级匹配 |
| AuthorizedRoots | authorized_roots.rs | 管理 Agent 可访问的授权目录集合，默认为 workspace 根 |
| SandboxConfig | sandbox.rs | macOS Seatbelt worker 命令与写圈禁，以及 Linux Landlock ABI v1 读写圈禁和每次运行 enforcement 状态 |
