# agent/runtime/security
> Agent 运行时安全层（Sub-7）

## 地位
在每次工具执行前进行安全检查，组合敏感路径过滤、授权根目录检查、HITL 权限管理和沙箱配置四个子系统。

## 逻辑
`SecurityGuard` 是统一安全网关，`check_tool_call()` 依次检查：提取路径 -> 敏感路径过滤 -> 授权根检查 -> 危险工具 HITL 审批。决策结果为 Allow/Deny/NeedHumanApproval。

## 约束
- macOS 使用 sandbox-exec Seatbelt profile
- 危险工具列表硬编码在 guard.rs
- HITL 通过 oneshot channel 实现异步等待

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| guard | `guard.rs` | SecurityGuard：统一安全网关、SecurityDecision |
| permission | `permission.rs` | PermissionManager：HITL 权限缓存与审批流 |
| sensitive_paths | `sensitive_paths.rs` | SensitivePathFilter：敏感文件/目录/扩展名/正则过滤 |
| authorized_roots | `authorized_roots.rs` | AuthorizedRoots：授权根目录管理 |
| sandbox | `sandbox.rs` | SandboxConfig、SandboxMode：macOS Seatbelt 沙箱配置 |
