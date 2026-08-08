# ALVA Hub 0→1 演进 for 循环 — 契约

> 用户指令：参考 makecindy/cindy，把 Hub（alva-hub-desktop）从 0→1 全做起来，自主演进。
> 约束：上下文不能炸；每圈清 Rust 构建残留（Mac 空间不足）；小步 commit。

## 循环定义（一圈 = 六步）

```text
for iteration in 1..10000:
    DISCOVER    # 从 cindy 功能面 + ALVA-HUB-V1 §17 验收矩阵找下一个最高价值缺口
    IMPLEMENT   # 实现 + 小步 commit（feat:/fix:/refactor:/chore:）
    REVIEW      # 对照 AGENTS.md 架构边界（Rule 17/18、bus 防退化、SDK 不接受功能扩展）
    BEST-PRACTICE  # 对照 cindy 找可借鉴做法，写进实现
    FIX+VERIFY  # cargo test / npm test / cargo build --release / scripts/ci-check-deps.sh
    CLEANUP     # 清 target 残留 + 追加 STATE.md 一行日志（压缩后靠它恢复）
```

## 初始状态（2026-08-08 盘点）

- V1 §17 共 18 场景：14 个 ✅ 全自动化；4 个 ⏳（场景1 真实引擎运行、3 真实触发、
  8 的 UI 部分、签名打包产物）
- **前端 web/ 零测试基建**（outstanding #3，仓库内可关）
- 真实引擎 CLI 已装：claude（mise node/24）、codex（npm-openai-codex）、alva（~/.cargo/bin）
- 磁盘：15Gi 可用，target 目录当前干净
- 架构：alva-hub-desktop 是 L7 入口，直接消费 alva-hub-core（仅依赖 engine contract）；
  不得依赖 alva-app-* 后端；Rule 18：alva-app-cli/tauri 不得依赖 alva-engine-*

## cindy（makecindy/cindy）功能面 → Hub 演进池

| cindy 功能 | Hub 现状 | 演进价值 |
|---|---|---|
| Memory 跨 harness 共享（"纠正一次处处正确"） | 无跨引擎 memory | 高：HubSession 级 memory 渲染进 handoff/上下文 |
| Skills 一次教法处处复用 | 仅 Alva 有 SkillsPlugin | 高：跨引擎 skills 渲染进 handoff 文本 |
| 引擎混合 + 任务中途切换 | ✅ 已完成（fallback/switch） | 维持 |
| Provider/model 管理 UX | readiness.rs 已做安装/auth 修复 | 中：每引擎 model 选择器 |
| 任务并行（Orca Lead/Worker） | ✅ 已完成（TeamRun/queues/claims/worktree） | 维持 |
| MCP 接入 | Alva 有，跨引擎无 | 低优先级 |
| 自动化/IM/移动端 | V1 non-goal | 跳过 |

## 迭代队列（按价值排序）

1. Web 测试基建（vitest + testing-library）+ 场景8 fallback 卡片控制 UI 测试 + 纯函数测试 → 关 outstanding #3
2. 打包构建验证（npm build + cargo build --release -p alva-hub-desktop）→ 关打包路径
3. 真实引擎就绪探测冒烟（用 readiness.rs 逻辑对 claude/codex/alva 真实 CLI 探测）
4. 跨引擎 memory（cindy 启发）：HubSession memory 注入 handoff，Claude 走 CLAUDE.md 注入、Codex 走 AGENTS.md
5. 跨引擎 skills：把 workspace skills 渲染进目标引擎 handoff 文本
6. engine model 选择器 + provider 状态展示
7. 打包 .app 产物冒烟启动

## 恢复协议（上下文压缩后）

读 `.scratch/hub-loop/STATE.md` → 续迭代队列。每圈只追加 STATE.md 一行：
`iter=N | 完成: X | 验证: Y | 清理: Z | 下一步: W`
