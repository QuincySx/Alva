# 2026-09-11 Review

固定范围：`134b8b3...f9d0e57`；新增Hub实测与用户UI需求另列。未修生产代码。

## Standards

1. **P2，架构规范违例**：kernel tool_batch 按具体StoreSessionCell查Extensions，execution context及run_child也识别该store cell，重新让内核承担agent_spawn持久化接线。违反CONTEXT的Kernel/Session Store归属。R18。
2. **P2，验证缺口/命名启发**：新增“cancelled_append_task...”测试没有阻塞监听者、没有abort，仅正常task.await；名字和注释不对应测试行为。R16。
3. **P3，分形文档违例**：新增cell公开表面/Agent返回类型/中间件职责没有级联到文件头及目录说明。R19。

Standards 3项，最高P2。public-surface快照通过，只证明公开表面与快照一致，不代表边界设计已合格。

## Spec

1. **P1，R09**：动态ToolExecutionContext缺少store_session_cell覆盖；child Extensions插Arc<T>却按T取；继续转发父cell也会关联错直接父节点。R14。
2. **P1，R10**：state.session更新后释放state锁，才更新cell，留出半切换窗口及并发swap不一致。R15。
3. **P2，R12**：首个追加仍await慢回调；abort追加任务后mem::take出的本地批次丢失，已有实时订阅者漏投递。R16。
4. **P2，R12**：clear重置seq却未同步high-water/订阅游标，新订阅也可丢掉新低序号事件。R17。

Spec 4项，最高P1。R09首层能力、R10顺序切换引用、R11workspace注入和原历史重复回归已有实质修正；不重复开旧问题卡。

## Hub 实际检查

- 自动化创建后显示1个/已启用，切页返回变0；实现只在React局部state，无scheduler保存。R20。
- Settings选择Plan后新会话仍workspace_write；安全/执行控件没有实际策略接线。R21。
- 插件生产页硬编码安装状态，注册按钮只清输入关对话框。R22。
- 浏览器12项中7过5失败，初步截图差异集中顶栏完成文案/位置，归为1项基线问题，不宣称5个产品bug。R23。

## 用户“你好”后的 UI 审查

主聊天默认执行图、消息大卡片与原样Markdown共同影响阅读；技术session UUID在多处重复；空composer过高。Inspector以Middleware巨列表开头，最新轮次再展开hooks，同时堆叠原生检查器、产品Attempt、Branches、事件摘要；重点/失败/当前上下文不清楚，部分用量显示冲突。

据此新增UI01–UI10：信息架构、语义tokens、基础控件密度、主聊天/Markdown、Inspector重点/下钻、工作台详情导航、composer/新会话、Sidebar/顶栏、次级页面、响应式/可访问状态。用户的时间线归属与Middleware默认隐藏要求优先于旧目录描述。

## 验证与界限

核心735项与前端70项通过；专项复现4项全部失败，确认动态store cell缺失、clear后漏事件、首个append阻塞及取消后漏投递。实际测试结果统一见[evidence/README.md](./evidence/README.md)。真实Tauri UI已运行并产生用户“你好”的回答；当前前端行为有截图。该次原生二进制为已有构建，最新Rust桌面构建未完成；不将其声称为f9d0e57原生全链路通过，也不宣称全工作区/全部引擎/E2E已验收。
