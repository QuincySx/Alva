# Architecture and reference documents
> Current Go architecture, governance and accepted stack decision.

## 地位
Current implementation authority is in the root Go module and its directory maps. Old Rust product/specification documents are retained only in the Git archive.
## 逻辑
ARCHITECTURE describes current dependencies, owners and physical layout. USAGE contains actual run/build/data instructions; current implementation progress is tracked in root MIGRATION.md. BUS-RULES and REGISTRAR-RULES constrain SDK growth; ADR 0012 records the Go stack and cutover. Dated Go migration/layout records are complete in history; old Rust comparisons remain in the Git archive.
## 约束
Do not treat archived Rust paths or competitor examples as active implementations. Runtime/platform acceptance remains in MIGRATION.md. Preserve accepted boundaries when updating docs.
## 业务域清单
| 路径 | 职责 |
|---|---|
| ARCHITECTURE.md | Current Go architecture、owners、Hub页面/服务/存储映射、typed输入草稿与只读计划准入及交互行为验收、UI/问答/权限分工、模型工具名称及独立开发者测试/评估、same-Turn等待和共享存储插口 |
| USAGE.md | Root-run commands、live/demo/HTTP、正式Hub工作台/首发附件/命令Skills与项目引用/只读计划/设置返回/项目选择、外观设置与revision API、SDK/WASM作者插件、compatibility、CLI选项/Other/审批/等待、显式本机CLI配置复用及原生UI诊断、独立test/eval及storage conformance |
| HUB-FLOWS.md | 业务owner与各层职责、逐模块UI/SPEC/服务/存储对齐顺序与关闭条件、用户动作到服务/SQLite事实的可重复验收入口、真实Codex与fixture边界、带稳定编号/关闭条件的已知未闭环账；非完整需求总表，不据测试数计算产品完成率 |
| MYGO-WEBVIEW-VALIDATION.md | MyGo v0.6.0隔离实测、系统WebView/多窗口归属、混合尺寸/滚动交接与测量边界；不改变生产依赖或桌面ADR |
| design/ | SmallPen bundled Skill入口、基础Token/组件与Lucide图标、稳定功能SPEC、包外组合预览和revision绑定Review；保留正式页面与线稿，跨平台规范不改变生产栈或代表业务已验收 |
| history/ | Complete dated Go checkpoints and initial repository layout review |
| BUS-RULES.md | Typed capabilities and explicit bounded notifications |
| REGISTRAR-RULES.md | Assembly contribution boundaries |
| ROADMAP.md | 模块固定与 v1 的阶段顺序、阶段 1 消息流拆分与工具块呈现、长会话验收预算；进度与证据仍在 MIGRATION |
| adr/ | Accepted Go stack decision, ADR 0013 v1 scope/content blocks/long-session/narrowing amendments and historical archive pointer |
