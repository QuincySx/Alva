# ALVA Hub Desktop V1 Release Readiness

> 规划启动：2026-08-30  
> 状态：待执行  
> 假设：下一阶段不扩展产品能力，只把已经实现的 Hub Desktop V1 推进到可审计、可分发、可宣布接受的 Release Candidate。

## 为什么现在做

UI System 任务 00–12 已完成，当前代码已经具备多 Engine session、branch、handoff、fallback、TeamRun、Workbench 与持久化能力；自动化回归、视觉回归和本地 Tauri 打包也已通过。

正式验收仍有两类证据缺口：

1. packaged Desktop GUI 尚未逐个驱动所有发布范围内的真实 Engine；
2. 六种 continuity failure 尚未形成“自动化分类 + packaged 确定性场景 + 可行的真实外部观察”三层证据。

本规划把这两个缺口扩展成一套完整但有限的发布闸门，避免继续用功能开发掩盖发布问题。

## 目标

- 冻结一个可复现的 Hub Desktop RC 基线；
- 用真实 packaged WebView 验证发布范围内的 Engine、fallback、恢复和权限路径；
- 建立不会进入生产包的确定性故障场景运行器；
- 完成 macOS Developer ID 签名、notarization、冷安装和升级验证；
- 关闭安全、隐私、可访问性和文档证据；
- 只有全部 Gate 通过后，才把 `docs/ALVA-HUB-V1.md` 从 accepted target 更新为 accepted implementation。

## 非目标

- 不新增 Engine、自动质量路由、云同步、账号、组织管理或远程 Worker；
- 不重写现有 Hub runtime、store、UI System 或 engine adapter；
- 不把测试故障注入入口编进正式发布包；
- 不为验收修改 kernel / `agent-core`，不破坏 Rule 17 / Rule 18；
- 不把 injected failure 描述成真实 Provider 故障证据；
- 不在仓库、日志、截图或 handoff 中保存真实凭证。

## 证据分级

| 级别 | 含义 | 可以证明 | 不能替代 |
|---|---|---|---|
| A · Automated | Rust / Vitest / Playwright 可重复断言 | 合约、状态机、投影、边界 | packaged WebView 和真实 CLI |
| P · Packaged | `.app` 内确定性 fixture 或本地 runtime | Tauri command、store、WebView、恢复链路 | 真实 Provider 行为 |
| L · Live external | 真实 CLI / Provider / OS 条件 | 外部行为与分类器契合 | 可重复的故障覆盖 |
| D · Distribution | 签名、notarization、冷安装、升级 | 用户可安装与系统信任 | 产品行为正确性 |

任何验收结论必须写明证据级别。外部条件不可安全复现时，可以记录为限制，但不得把 P 级 fixture 冒充 L 级观察。

## 里程碑

| 里程碑 | Tickets | 出口条件 |
|---|---|---|
| M0 · RC 冻结 | 00–01 | 基线、范围和 packaged scenario runner 可复现 |
| M1 · Runtime Proof | 02–04 | Engine、fallback、restart/repair 矩阵有证据 |
| M2 · Ship Gate | 05–07 | 分发、安全和 packaged UX 闸门通过 |
| M3 · V1 Accept | 08 | 规范、验收矩阵、发布说明和回滚说明一致 |

执行主路径：`00 → 01 → {02, 03, 04, 06} → {05, 07} → 08`。

## 发布闸门

| Gate | 必须满足 |
|---|---|
| G0 · Truth | 支持范围、版本、测试数、artifact hash 与文档一致 |
| G1 · Runtime | 每个发布 Engine 至少一个 packaged GUI fresh run；支持 resume 的 Engine 再做一次 restart/resume |
| G2 · Continuity | 六种 continuity failure 有 A + P；安全可复现的条件补 L；fallback 不重复 target、原因可见且 source 可返回 |
| G3 · Durability | 关键 persistence phase 的 crash/restart、schema upgrade、orphan repair 通过 |
| G4 · Distribution | Developer ID 签名、notarization、Gatekeeper、冷安装和覆盖升级通过 |
| G5 · Safety | secret redaction、权限不降级、DB/日志权限、依赖审计和诊断导出通过 |
| G6 · UX | packaged app 的键盘、焦点、中英文、窄窗口和 reduced-motion smoke 通过 |

## 规划产物

- [`TASKS.md`](./TASKS.md)：任务状态、依赖和 Gate 映射；
- [`issues/`](./issues/)：00–08 可直接执行的 tickets；
- [`visual-plan/index.html`](./visual-plan/index.html)：当前事实与下一阶段执行路线图；
- 正式验收权威仍是 `docs/ALVA-HUB-V1.md` 与 `crates/alva-hub-desktop/ACCEPTANCE.md`，仅在 Ticket 08 达标后更新结论。

