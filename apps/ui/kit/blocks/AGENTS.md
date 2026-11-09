# Kit blocks
> Alva 业务组合块：只含呈现与交互槽位，入参是本包自己的视图模型，不接任何产品类型。

## 地位
Alva Kit 的 K3（ADR 0014）。位于 `apps/ui/kit/blocks`，依赖 kit 与 MyGo 与自身；`boundaries_test` 禁止它导入 `apps/hub`、direct、engine、SDK 等一切产品或执行层。产品侧到这些视图模型的映射属于 K4 的产品与桌面层。
## 逻辑
每个块 = 视图模型（枚举表达类型与状态，字符串表达内容）+ 动作回调（可选，门控）+ 可选槽位（`Toolbar`、`Queue`、`fields`）。`ToolKind` 是命令、读文件、改文件、搜索、子 Agent、待办、其他，不出现任何工具名；Approval 与 Question 的 `AskStatus` 是等待、处理中、已允许、已拒绝、已回答、已过期、出错。Composer 外壳有 13 个阶段（`ComposerPhase`）加强制焦点，覆盖设计的 14 个样本；排队与插话只提供 `Queued` 计数与 `Queue` 槽位，行为由 M2 实现。所有块复用 kit 的 `InlineAction`、`InlineFocus`、`Loader`、`PanelSurface`、`Slot`、`ListRow`、`Menu`、`Segmented`、`Card` 与强制态机制。
## 约束
不含业务流程：一次性同意、迟到点击拒绝、分支归属、脱敏由产品层负责；块只保证不重复触发（Busy 时回调不触发）、暴露 busy、按钮可聚焦且有可访问名称。不含颜色与尺寸字面量、工具名或引擎名（kit 的架构测试覆盖本包）。
## 业务域清单
| 文件 | 职责 |
|---|---|
| doc.go / util_desktop.go | 包说明；数字格式化 |
| blocks_desktop.go | RunStatus、TaskRow、ConnectionRow、ModelRow、MessageHeader、Citation、Attachment、CodeBlock 与它们的视图模型 |
| tool_desktop.go | ToolActivity 与 ToolKind/ToolStatus |
| ask_desktop.go | Approval、Question、QuestionOption 与 AskStatus |
| composer_desktop.go | Composer 外壳、VoiceInput、ComposerMenu、PermissionMenu |
| forms_desktop.go | ProviderForm、ModelForm 外壳与 ProviderModels 目录面板 |
| pages_desktop.go | 画廊页：每个设计组件一页，格子等于设计变体，带真实演示 |
| *_desktop_test.go | 交互、几何与像素、变异验证覆盖的关键路径测试 |
