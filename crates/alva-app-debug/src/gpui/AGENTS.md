# GPUI Inspector
> GPUI 视图树调试检查器，将运行时视图层级构建为可检查的树形结构

## 地位
alva-app-debug crate 的 GPUI 专属子模块。实现 `Inspectable` trait，为调试面板和开发工具提供 GPUI 视图树的运行时快照能力。依赖 crate 内部定义的 `InspectNode` 和 `Inspectable` trait。

## 逻辑
1. **ViewRegistry** — 线程安全的视图注册表（`RwLock<Vec<ViewEntry>>`），视图组件在挂载时调用 `register` 注册自身（包括 id、type_name、parent_id 和快照闭包），卸载时调用 `unregister` 移除。
2. **ViewEntry** — 单条注册记录，持有一个 `snapshot_fn` 闭包，调用时返回该视图当前状态的 `InspectNode`。
3. **build_tree** — 从扁平的 ViewEntry 列表构建父子树：先收集所有快照，按 parent_id 分组，递归挂载子节点，最终返回一棵 `InspectNode` 树。
4. **GpuiInspector** — 持有 `ViewRegistry` 的引用，实现 `Inspectable` trait，调用 `inspect()` 即可获得完整视图树快照。

## 约束
- `snapshot_fn` 闭包必须是 `Send + Sync`，因为 ViewRegistry 可跨线程访问
- ViewEntry 的 `id` 必须全局唯一，`unregister` 依赖 id 精确匹配进行移除
- 当注册表为空时，`build_tree` 返回一个 id 为 "empty" 的占位节点
- 多个根节点时自动包裹一个虚拟 "Root" 节点

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | mod.rs | ViewEntry、ViewRegistry、GpuiInspector 的完整实现及单元测试 |
