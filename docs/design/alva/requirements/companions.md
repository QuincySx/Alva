# companions

> 来源：旧 JSON 包的 `requirements/companions.json`（该包已于 2026-11-09 删除，见 git 提交 690c4525）。本文件是这些需求的正本；页面位置按新包路径和元素 `data-name` 链给出。

## req_alva_companion_profile — 伙伴资料与本窗草稿

状态：2026-10-27 已确认的伙伴/群产品契约；现有27–30静态页面不执行保存、授权或模型调用。
触发新建或设置后，仅编辑本窗资料：名字、职责、实际可用模型 / Agent、私聊工作区、Ask / Auto 意图及明确固定来源的 Skill 引用。Skill 的源会话、分支与 SHA 固定；“允许在群使用”是独立选择。资料、Skill 或角色正文不授予工具、文件、MCP、账号或 Full 权限。已有私聊的工作区固定。
明确保存才按原 ID / revision 提交；成功以真实已提交回执为准，失败保留表单。取消既有编辑还原已保存资料，新建取消不创建伙伴、会话或执行。返回工作台 / 设置再返回可保留本窗未存表单；改选另一身份后不继承原草稿。迟到保存回执仍归原对象，不覆盖后来选择。
控件：复用既有Foundation Button/SelectOption/Checkbox与原生共享控件，表单正文独立滚动，保存/取消区固定。
owner与来源：共享CompanionService/SQLite拥有资料、群、scope与回合事实；原始成员记录仍归各实际会话，UI为观察者。apps/hub/product/companions.go、apps/hub/sqlite/companions.go、apps/hub/desktop/workspace_companions_desktop.go及伙伴/群回归。
验收：失败保留全部输入；取消编辑还原保存值，新建取消零创建；迟到保存不替换后来选择。本轮范围为本地SQLite、deterministic engine/local HTTP与无窗口MyGo控件；真实OS/IME/设备/外部账户和新增原生画面验收分别记录，不由静态设计或测试通过推定。

绑定位置：
- `pages/伙伴/28 伙伴资料.html` → `Dialog`

标注：
- [behavior-contract] req_alva_companion_profile — 伙伴资料、明确保存及本窗身份绑定。 — `pages/伙伴/28 伙伴资料.html` → `Dialog`

## req_alva_companion_private — 私聊、名册与身份生命周期

状态：2026-10-27 已确认的伙伴/群产品契约；现有27–30静态页面不执行保存、授权或模型调用。
打开名册、搜索、刷新和读取已有私聊均为冷观察。首次明确“打开私聊”可建立此伙伴独立的稳定会话，但不运行模型；模型执行仅由随后明确 Send 触发，并要求实际账号 / 计费路由确认。已审核的 Profile revision 发生变化时拒绝旧授权，要求重新检查。资料更新作用于之后明确的新轮，保留既有历史与执行 owner。
启停只改变后续准入，不停止正在执行的 owner。删除经确认后成为 tombstone，原会话 / 原件 / 群历史保留；已删除入口仅可浏览，不恢复身份或发送。已有物理执行仍由原 Stop 按真实 owner 取消并等待收尾。私聊读取或打开迟到时可保留实际会话回执，但不能把用户从设置或后来的会话导航拉走。
owner与来源：共享CompanionService/SQLite拥有资料、群、scope与回合事实；原始成员记录仍归各实际会话，UI为观察者。apps/hub/product/companions.go、apps/hub/product/companions_browse.go、apps/hub/desktop/workspace_companions_desktop.go及伙伴/群回归。
验收：搜索/刷新零执行，首次打开私聊只建稳定会话；停用/删除不Stop原owner，历史可读且旧导航不拉回当前页。本轮范围为本地SQLite、deterministic engine/local HTTP与无窗口MyGo控件；真实OS/IME/设备/外部账户和新增原生画面验收分别记录，不由静态设计或测试通过推定。

绑定位置：
- `pages/伙伴/27 伙伴私聊.html` → `Main`

标注：
- [behavior-contract] req_alva_companion_private — 冷名册与私聊；明确Send才执行，删除保留只读历史。 — `pages/伙伴/27 伙伴私聊.html` → `Main`

## req_alva_companion_group_settings — 群设置与隔离 scope

状态：2026-10-27 已确认的伙伴/群产品契约；现有27–30静态页面不执行保存、授权或模型调用。
新建 / 设置共用本窗草稿：群名、1..16 个伙伴、既有 mentions / all 回复模式、有限回复数与共享工作区。保存前不改变群执行事实。取消丢弃草稿。失败保留所有字段；成功按原群 ID / revision 回执更新。
发言顺序按已保存名册，mentions 只选择本轮明确勾选成员，all 使用已保存成员；参与人数不得超过保存的回复上限。已停用且原已选成员允许移出，移出后仍不能重新加入，直到伙伴启用。改变成员 / 共享工作区后开启新的隔离 scope；旧 scope 仍可只读访问。同 scope 的名称 / 回复策略更新不删除既有历史或未发消息。
控件：复用既有Foundation Button/SelectOption/Checkbox与原生共享控件，表单正文独立滚动，保存/取消区固定。
owner与来源：共享CompanionService/SQLite拥有资料、群、scope与回合事实；原始成员记录仍归各实际会话，UI为观察者。apps/hub/product/companions.go、apps/hub/sqlite/companions.go、apps/hub/desktop/workspace_companions_desktop.go及伙伴/群回归。
验收：取消后按已提交回复策略发送；保存失败保稿；停用已选成员可移出；scope变更立即隔离旧读取与恢复批准。本轮范围为本地SQLite、deterministic engine/local HTTP与无窗口MyGo控件；真实OS/IME/设备/外部账户和新增原生画面验收分别记录，不由静态设计或测试通过推定。

绑定位置：
- `pages/伙伴/30 新建群聊.html` → `Dialog`

标注：
- [behavior-contract] req_alva_companion_group_settings — 已提交群配置与独立草稿；成员/工作区变更隔离scope。 — `pages/伙伴/30 新建群聊.html` → `Dialog`

## req_alva_companion_group_round — 群回合、等待与同 Turn 恢复

状态：2026-10-27 已确认的伙伴/群产品契约；现有27–30静态页面不执行保存、授权或模型调用。
仅用户明确 Send 开启有限回合，并确认参与成员的实际账号 / 计费路由；固定本轮成员、Profile revision 和顺序。每个成员使用群 × scope × 伙伴的独立会话，不带入私聊正文或私人记忆；群只使用本群上下文及明确允许且重新核验的 Skill 来源。成员回复不自动开启下一轮；已有未完成回合阻止新回合和群配置变更。
Stop 只针对捕获的原 round / operation，传播取消并等待真实成员 owner 退出，不启动后续成员。关闭 / 退订观察者不等于 Stop。等待时保留 round、原 Turn 与完成游标；保存问题 / 审批结果不执行。用户重新确认固定伙伴和路由后，明确继续同一回合 / 原 Turn，已完成成员和已发布回复不重放。终止需先审阅并放弃原检查点，保留已发生或未知副作用说明；未知结果不自动重放。
群消息 / 回合支持按 owner、群、scope 的真实有界 keyset 页读取和显式加载更早页；成员原始记录有独立查看入口。已删除群仍保留只读发现和分页，不恢复执行权。切换 scope 立即隔离旧消息、回合、恢复批准和迟到读取回执。
owner与来源：共享CompanionService/SQLite拥有资料、群、scope与回合事实；原始成员记录仍归各实际会话，UI为观察者。apps/hub/product/companions_rounds.go、apps/hub/product/companions_browse.go、apps/hub/desktop/workspace_companions_desktop.go及伙伴/群回归。
验收：等待/重开仍是原Round/Turn且不执行；明确继续不重放完成成员；Stop真实join，删除后仍可有界分页查看历史。本轮范围为本地SQLite、deterministic engine/local HTTP与无窗口MyGo控件；真实OS/IME/设备/外部账户和新增原生画面验收分别记录，不由静态设计或测试通过推定。

绑定位置：
- `pages/伙伴/29 伙伴群聊.html` → `Main`

标注：
- [behavior-contract] req_alva_companion_group_round — 明确有限回合；原owner Stop与显式原Turn恢复，不重放。 — `pages/伙伴/29 伙伴群聊.html` → `Main`
