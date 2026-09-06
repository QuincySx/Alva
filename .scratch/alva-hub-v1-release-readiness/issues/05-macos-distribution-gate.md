# 05 macOS Distribution Gate

## 目标

把本地 ad-hoc `.app` 提升为用户可安装、Gatekeeper 可验证、可以覆盖升级的 macOS Release Candidate。

## 工作

- 使用独立 Developer ID Application 身份构建，不把证书或密码写入仓库；
- 对 bundle 完成 hardened runtime、签名、notarization 和 stapling；
- 在干净用户目录执行下载/冷安装/首次启动/Gatekeeper 检查；
- 从上一候选版本覆盖升级，确认私有 DB 迁移和 session 保留；
- 验证卸载 app 不意外删除用户数据，并记录显式数据清理方法；
- 生成 artifact SHA-256、签名信息、notary 结果和最低 macOS 版本记录。

## 验收

- `codesign --verify --deep --strict`、`spctl --assess` 与 stapler validation 通过；
- 冷安装和覆盖升级均能启动并完成基础 session smoke；
- 发布说明明确架构、最低系统版本、数据路径和已知限制；
- 缺少 Apple Developer 权限时标记为 external gate，不用 ad-hoc 签名冒充完成。

## 依赖

Tickets 02–04。

