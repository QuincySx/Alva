# alva-hub-desktop/web/src
> Hub Desktop ticket-01 React workspace、composer 与产品 timeline。

## 地位

Tauri webview 的纯表现层。只消费稳定 IPC DTO 和 `hub_timeline` 产品事件，
不理解 BaseAgent 或 adapter-native stream。

## 逻辑

1. `bridge` 选择 workspace、提交 prompt、订阅 timeline。
2. `App` 管理用户输入和可见状态。
3. `index.css` 提供两栏 Desktop walking-skeleton 布局。

## 约束

- 不直接渲染 raw adapter event。
- browser-only 模式必须给出清楚错误，不产生未捕获异常。
- ticket 01 只提供 Alva EngineTarget。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| React entry | `main.tsx` | 挂载应用 |
| Product UI | `App.tsx` | workspace、composer、timeline |
| IPC bridge | `bridge.ts` | Tauri invoke/listen/dialog 封装 |
| Styles | `index.css` | Desktop 视觉与响应式布局 |
