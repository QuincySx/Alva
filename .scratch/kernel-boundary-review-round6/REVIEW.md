# Round 6 复审

本轮仍不能整体关卡。按上轮 R38–R61 的 24 张卡逐项核对了 `5c64af1640ab00b6a64aa3474112b62fe4106036...5290e033585bd44c6c019c2a47661043e05009b7` 的全部 16 个提交、43 个变更文件。历史 106 张卡的背景见 round5；本轮没有冒称重新执行全部历史验收。

## Standards

独立报告：[STANDARDS](STANDARDS.md)。4 项：公开 API 属性漏检、签名漏检（P2，合为 R70），分形导航遗漏、spacing token 残留（P3，合为 R75）。六组 before/after 契约变异均生成相同快照，闸门不能提供所声明的保护。没有根据抽象代码气味另拆泛化重构卡。

## Spec

独立报告：[CORE](CORE.md)、[UI](UI.md)；集成面：[SERVER](SERVER.md)。13 项：中文审批吞掉工具效果、广播跨会话阻断、旧取消误伤新轮、旧登记覆盖新句柄、HTTP Stop 过早终态、启动失败 sink 泄漏、Inspector 定位失败、只读执行误称变更、费用缺失覆盖不全，以及 swap/工具锁/压缩三项验收缺口、引擎合规矩阵未完成。

广播、两项代际竞态和 HTTP Stop 四项为 P1。实现修复和测试修复分卡，不因已有测试全绿就认定需求完成。R45、R52 的执行记录本来就承认部分未完成，不能把它们描述成隐瞒；R48 的部分验收声明则超出了实际断言。

## 24 张原卡去向

| 原卡 | 结论 | 后续/证据边界 |
|---|---|---|
| R38 | 返工 | R62；跨会话广播回归 |
| R39 | 局部有效 | 退出关闭/排空修复存在；广播整体仍受 R62 影响 |
| R40 | 实现有效 | role-scoped merge；真实事件浏览器验收留 R45 |
| R41 | 字符修复有效，阅读验收不足 | jsdom 负 scrollTop 不是浏览器几何证明；留 R45 |
| R42 | 返工 | R67；默认折叠定位失败，概览未完整迁入 |
| R43 | 文案修复有效 | ALVA 范围已说清；完整资格矩阵未验 |
| R44 | 有效 | 资料假保存改为诚实禁用 |
| R45 | 继续 open | 实现者已承认；真实权限/密度/滚动/Tab 等验收未完成 |
| R46 | 部分完成 | R75；新增测试导航遗漏 |
| R47 | 部分完成 | R75；残留 spacing literal |
| R48 | 部分完成 | R71/R72/R73；生产 Inspector 重开递归有效，勿重做 |
| R49 | 实现有效，验收未全覆盖 | 补 ToolEnd 路径正确；生产 side_effect SQLite 重开组合未新增 |
| R50 | 普通路径有效，竞态失败 | R63 |
| R51 | 返工 | R65/R66；终止判断、取消时序与失败清理 |
| R52 | 继续 open | R74；实现者承认 Full/quarantine，另漏 Alva KilledMidRun |
| R53 | 有效 | 浏览器/终端/文本功能诚实禁用 |
| R54 | 返工 | R68；只读工具仍称变更 |
| R55 | 返工 | R69；整个 usage 缺失未算 partial |
| R56 | 返工 | R70；六类公开契约差异漏检 |
| R57 | DTO 有效，中文 UI 返工 | R76；未知英文效果被替换通用提示 |
| R58 | 返工 | R63/R64；旧 cancel 与旧登记两条竞态 |
| R59 | 有限索引有效 | 真实会话+三导航动作，无结果状态；键盘端到端未验 |
| R60 | 有效 | helper await 与真实错误断言修复 |
| R61 | 有效 | 正式 build_engine_for_target 公共入口被测试直接调用 |

## 验证与交付

- 前端现有测试：7 文件、99 项通过；`evidence/web-tests.txt`。
- 浏览器现有测试：23 项通过（11 行为、12 视觉）；`evidence/browser-tests.txt`。截图基线通过不替代 R45 行为验收。
- 独立 UI 探针：定位/只读措辞/费用 3 项失败；审批中英对照中文失败、英文通过。见 `evidence/ui-probes.txt`、`evidence/ui-r57-probes.txt`。
- 独立核心探针：跨会话无 Tokio、异步 listener、旧 cancel、旧登记四个确定性反例；最终结果及依赖版本见 CORE 和 evidence/core。
- 扫描器变异：6 类破坏性改动全部漏检；`evidence/scanner-results.txt`。
- Rust 常规测试：agent-core 68、security 193、engine-runtime 53、hub-core 40，共 354 项通过；`evidence/core/core-tests.log`。
- server seam 探针未执行：首次 Cargo package/build 锁等待超过 11 分钟，核心套件退出后重试仍等待 package cache，已中止本次探针并归档。R65/R66 为静态确认，未计入动态复现；没有声称真实 HTTP 集成通过。

[下一波任务](TASKS.md)：15 张 R62–R76，另保留原 R45 open。证据、复现入口、有限验收和执行记录模板已写入各卡。未修改生产实现、未提交或推送。

Standards：4 项，最严重为 P2 公开 API 闸门漏检。Spec：13 项，最严重为 P1 运行/取消与广播竞态。
