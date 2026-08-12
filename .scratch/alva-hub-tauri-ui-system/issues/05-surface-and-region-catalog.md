# 05 Surface and Region Catalog

## 目标

建立可单独预览的 L2 区域与 Surface 目录，页面只拼这些积木。

## 工作

- ContentFrame、ScrollRegion、DockRegion、WorkbenchPage；
- PanelSection、Notice、StatusListItem、EmptyState；
- ListRow、CardGrid、SettingsGroup、SettingsRow；
- 每个组件提供 default/compact、宽度和状态示例；
- 每个区域说明 width、scroll、dock 和 responsive 所有权。

## 验收

- 组件目录覆盖七个页面总层架中的重复结构；
- 页面不复制基础边框、圆角和 section header；
- 每个区域只存在一个滚动拥有者；
- Region 与 Surface 不依赖业务层。
