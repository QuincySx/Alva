# R33: 完成 Inspector 选择联动与逐层返回体验

**Priority:** P2
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；UI05、UI06
**Blocked by:** R30
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

components/workbench-tools.tsx:183–274 仍串联原生 Inspector、Product Attempts、handoff、branches、memory/live events；仅默认折叠产品尝试，没有选择驱动 native。engine-inspector.tsx:363 仍用截断 ID 面包屑；UI05/06 执行记录承认滚动恢复、跨层选择未完成。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

按既定 Domain/Surface 分工统一入口，Product Attempt 选择驱动原生定位或给明确不可用状态；子 Agent/LLM/Tool 面包屑用友好身份，保存每层滚动和焦点。避免再创建一套重复详情。

## 验收

- [x] 根→child→LLM/Tool→多次Back 正确恢复选择、位置和焦点。
- [x] 长聊天打开详情立即可见；聊天与每层详情各自唯一 scroll owner。
- [x] 小用量/未知用量、失败、长 hooks、子 Agent 路径矩阵通过；Middleware 继续默认折叠。

## 执行记录（执行者填写）

- Commit：`0ac6ccf`（feat(hub-desktop): attempt selection locates the Inspector; levels keep scroll and focus (R33)）。
- 专项复现/回归命令、版本（vitest 2.1.9，jsdom 环境，HEAD `0ac6ccf`）：
  - `npm test -- src/components/components.test.tsx`（web/ 下）— 28/28 通过，新增 3 个 R33 专项：
    - `restores scroll position and focus through repeated Back steps`：root→child→LLM 三层下钻，断言详情打开即在顶部（scrollTop=0）、Back#1 恢复 child 层 scrollTop=300 且焦点回到 `llm:0` 行控件、Back#2 恢复 root 层 scrollTop=120 且焦点回到 child 行控件（jsdom scrollTop 读写已验证可用）。
    - `product attempt selection drives the Inspector and unsupported engines say so`：claude 尝试定位显示明确不可用 Notice 且不发起 `loadInspection(product, native-claude-1)`；alva 尝试定位把路径截断回根层并清除提示。
    - `renders failures, unknown usage and long hook chains with middleware collapsed`：0/0 token 显示"—"、失败徽标、9 条 hooks 默认折叠、展开后全链可达、子 Agent 路径可导航。
  - `npm test`（全量）— 6 files / 89 tests 通过；`tsc -b` 通过。
  - 原有 child-drill 测试断言同步更新为友好面包屑（engine + 角色，短 id 为次要提示）。
- 实现要点：focus origin 以 key 栈记录（详情页会卸载列表 DOM，Back 后按 `data-inspector-origin` 重查询并 focus）；focus 目标已在路径中时截断路径到该层（真"定位"）；加载失败也消费 focus 请求，避免 live 轮询每秒重试失败加载；`.hub-inspector-tool .hub-engine-inspector` 限高、`__body` 为唯一 scroll owner。
- 未完成项：无（R33 验收 3 项均由上述测试覆盖；真实 Tauri 端到端属 R35 真实产品验收范围）。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
