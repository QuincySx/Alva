# Hub session presentation
> 产品时间线投影与有界渲染，不拥有引擎执行策略。

## 地位
位于 Hub web 表现层，输入稳定产品 DTO。

## 逻辑
`timeline-model` 统一实时与历史条目身份和合并；`bounded-list` 对消息、工具、审批和状态共享窗口，动态测量已挂载行，保持阅读锚点或底部跟随。

## 约束
- 虚拟化只卸载 DOM，不删除权威数据；全文保存能力由后端负责。
- 行必须注册最外层尺寸；单一外部 scroll owner，不新增嵌套主滚动区。
- ResizeObserver、scroll listener 与 animation frame 必须随卸载清理。
- 会话切换由父级 key 隔离缓存；窗口测试必须统计全部行，不能只数消息。
- 工具按 attempt 隔离身份，使用持久化 first_seen_at/sequence 排序；旧记录使用 attempt 起点与 evidence 数组次序，不伪造缺失的时间。
- BoundedList 是测量补偿与 follow 意图的唯一拥有者，布局尺寸变化引起的 scroll clamp 不退出跟随；会话切换重置跟随。
- 相同文本 delta 去重仍需独立审查，不能因工具排序通过宣称完整事件协议验收。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| Projection | timeline-model.ts | 产品条目身份、排序、合并及历史映射接缝 |
| Projection tests | timeline-model.test.ts | 身份、会话隔离、合并与工具参数保留 |
| Window | bounded-list.tsx | 动态高度测量、窗口计算与锚点补偿 |
| Browser evidence | ../../tests/windowing.spec.ts | 全部行 DOM 上界与流式阅读锚点回归 |
