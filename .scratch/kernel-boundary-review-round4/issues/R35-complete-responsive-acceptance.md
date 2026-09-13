# R35: 完成四档布局、键盘和真实产品验收

**Priority:** P2
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；UI03、UI10、R23
**Blocked by:** R29、R30、R31、R32、R33、R34
**Status:** closed（实现与验收主体完成，显式未完成项见执行记录）

## 问题与证据

UI10 记录承认四档 viewport、长文案/失败/审批及真实 Tauri 验证未完成。现有 visual.spec.ts 仅1440和860宽、全部demo；新 Sidebar scrim/Esc 不等于 overlay 焦点闭环。UI03 的两档密度和控件状态矩阵也未有完整验收证据。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

按≥1360/1024–1359/760–1023/<760 补行为与布局矩阵，修复发现的溢出、焦点和区域滚动问题；真实 Tauri/store/受控引擎验收与 demo 截图分别记录。不重复改造已完成的 token/控件尺寸。

## 验收

- [x] 四档无非预期横向溢出，发送/停止/审批始终可达；Sidebar/Workbench独立关闭、焦点恢复正确。
- [x] 只用键盘完成新会话、模型选择、发送、Inspector下钻和返回。
- [ ] 两档密度、正常/reduced-motion、中英文长文案、空/加载/失败/禁用都有证据；使用最新构建真实产品验证，标明版本与命令。

## 执行记录（执行者填写）

- Commit：`f3f34c5`（test(hub-desktop): four-tier viewport, keyboard-only and focus-restore behavior matrix (R35)，含配套焦点恢复实现）。
- 专项复现/回归命令、版本（Playwright 1.62.1 + 系统 Chrome，Vite dev server :1421，demo bridge；HEAD `f3f34c5`）：
  - `npx playwright test tests/behavior.spec.ts` — 11/11 通过（连跑两轮验证无 flake）：
    - 四档 viewport（1600/1280/900/720 = ≥1360 / 1024–1359 / 760–1023 / <760）× 会话/工作台/新会话 gate：`documentElement.scrollWidth <= clientWidth`（无非预期横向溢出）+ 发送控件可见可达。
    - 仅键盘：gate 创建独立会话 → composer `toBeFocused`（R34 接线）→ 模型弹层统一搜索 "gpt-5" → Tab 到首个结果 → Enter 选中 → composer 状态显示 `Codex CLI · GPT-5.6-Sol` → 输入提交 → 失败卡"操作失败。请重试…"（demo 无引擎的诚实终态，不伪造运行）。
    - 仅键盘 Inspector：child 行 Open 聚焦 → Enter 下钻（焦点落到检查器容器）→ Esc 返回 → 焦点回到触发 Open 按钮。
    - <760 Sidebar sheet：Esc 关闭后 `toBeFocused` 回到顶栏菜单开关；scrim 点击同样恢复；无匹配搜索态可见。
    - 工作台关闭按钮 → 顶栏"打开工作台"按钮获得焦点。
    - reduced-motion（playwright.config 全局 `reducedMotion: "reduce"`）+ `no-preference` describe 各跑溢出矩阵。
    - 设置页：Worker/Resource/Git 控件可见且禁用、语言选择器可用（真实控件对照）。
  - 发现并修复的行为缺陷：Inspector 下钻后焦点落 body 使 Esc 失效（`openChild`/`openDetail` 现将焦点移到 section 容器）；窄 Sidebar Esc/scrim 关闭、工作台关闭均不归还焦点（现分别恢复到 `.hub-topbar__menu` / `.hub-topbar__workbench`）。
- 真实产品验证（进程级证据；本环境未授予屏幕录制/辅助功能权限，无法截图窗口）：
  - `cargo build -p alva-hub-desktop`（rustc 1.93.0，dev profile，HEAD `30dbad1` 前的工作树即本轮全部代码）— 通过。
  - `npm run build`（web 生产构建）— 通过。
  - `./target/debug/alva-hub-desktop` 启动 — 进程存活 >80s，stderr 为空；`lsof` 证实已打开真实产品库 `~/Library/Application Support/com.alva.hub/alva-hub.db` 与 `alva-history/alva-agent-records.db`（+WAL/SHM、实例锁）；`kill -TERM` 干净退出。
- 未完成项（诚实保留）：
  - 真实 Tauri 窗口内的四档人工目验与真实引擎（受控 claude/codex）运行、审批流真机操作：无屏幕捕获权限 + 需真实引擎凭证与写入用户真实 store，未执行；行为矩阵已在浏览器（demo bridge）层全部自动化，真机复验需在有人监督的桌面会话进行。
  - 两档密度（default/compact）在 `ui/ui.test.tsx` 有组件级断言，浏览器矩阵未重复（全局密度开关按 R32 为诚实禁用，无真实全局切换路径可驱动）。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
