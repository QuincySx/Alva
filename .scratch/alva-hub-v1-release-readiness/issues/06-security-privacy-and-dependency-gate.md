# 06 Security, Privacy, and Dependency Gate

## 目标

用发布范围的证据关闭 secret、权限、文件边界、子进程与依赖风险。

## 工作

- 对 handoff、timeline、Inspector Bundle、diagnostics、截图和 scenario evidence 执行 secret canary；
- 验证 permission profile 只可保持或收紧，弱化映射 fail closed；
- 检查 API key 只经允许的配置/子进程环境传递，UI 不回显；
- 检查 Hub DB、日志、临时 provider 配置和证据目录为 owner-only；
- 审计外部链接、CSP/Tauri capability、workspace 路径与 symlink 拒绝；
- 对 npm/Cargo 依赖记录 advisories、可达性、缓解与延期责任人；
- 验证正式 bundle 不含 fixture、测试账号、源码绝对路径或调试开关。

## 验收

- canary secret 不出现在任何跨 Engine、导出或日志表面；
- permission downgrade 与未授权路径访问有自动化和 packaged 证据；
- 每个未修 advisory 有 scope、可达性、临时缓解和后续 ticket；
- 安全报告不包含真实 secret 或本机私有路径明文样本。

## 依赖

Ticket 00，可与 01–04 并行。

