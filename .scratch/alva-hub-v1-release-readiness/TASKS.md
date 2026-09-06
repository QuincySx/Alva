# Execution Tasks

## 当前状态

新规划已建立，所有执行 tickets 待开始。Ticket 00 是唯一入口；在 RC 范围和证据账本冻结前，不开始签名发布或修改正式 V1 结论。

| ID | 任务 | 依赖 | 状态 | Gate | 可见交付 |
|---|---|---|---|---|---|
| 00 | 冻结 RC 基线与支持范围 | 无 | 待开始 | G0 | commit/artifact/toolchain 清单与 Engine release/preview 决策 |
| 01 | 建立 packaged acceptance scenario runner | 00 | 待开始 | G0/G1 | 不进入生产包的确定性场景运行器与证据信封 |
| 02 | 完成真实 Engine Desktop GUI 矩阵 | 01 | 待开始 | G1 | 每个发布 Engine 的 fresh、restart、resume 证据 |
| 03 | 完成 continuity failure / fallback 矩阵 | 01 | 待开始 | G2 | 六触发三层证据、可见原因、无重复 target、可 switch back |
| 04 | 完成 crash、repair、migration soak | 01 | 待开始 | G3 | persistence phase 故障矩阵和恢复审计 |
| 05 | 完成 macOS 分发链路 | 02–04 | 待开始 | G4 | Developer ID 签名、notarization、冷安装和升级证据 |
| 06 | 完成安全、隐私与依赖闸门 | 00 | 待开始 | G5 | redaction、权限、文件权限、依赖和诊断导出报告 |
| 07 | 完成 packaged UX release smoke | 02、05、06 | 预检完成，待修复与正式包 | G6 | 键盘、焦点、语言、窄窗口和 reduced-motion 证据 |
| 08 | 关闭 V1 验收并发布 RC | 03–07 | 待开始 | 全部 | accepted implementation 文档、release notes、checksums、rollback |

## 并行关系

```text
00 ── 01 ─┬─ 02 ─┐
          ├─ 03 ─┼─ 05 ─┐
          └─ 04 ─┘      ├─ 07 ── 08
00 ───────── 06 ─────────┘
```

## 通用完成门槛

- 每个结论标记 A / P / L / D 证据级别并链接原始日志或截图；
- 记录 app 版本、commit、Cargo.lock hash、package-lock hash、macOS 与 Engine CLI 版本；
- fixture、测试凭证与故障入口不进入生产 bundle；
- 运行日志和截图先过 secret scan，再进入证据目录；
- 不把真实 Provider 限流、消费或凭证破坏当作默认测试手段；
- 任何 scope 变化必须先修改 Ticket 00 的支持范围，不在中途静默扩展；
- `cargo test -p alva-hub-desktop`、Web tests/build、Playwright、Rule 17/18 与 Tauri package 持续通过；
- 外部凭证或 Apple Developer 权限缺失时，明确标记 external gate，不伪造完成状态。
