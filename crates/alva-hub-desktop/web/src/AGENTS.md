# alva-hub-desktop/web/src
> Hub Desktop React session list、workspace、composer 与产品 timeline。

## 地位

Tauri webview 的纯表现层。只消费稳定 IPC DTO 和 `hub_timeline` 产品事件，
不理解 BaseAgent 或 adapter-native stream。

## 逻辑

1. `bridge` 选择 workspace、提交/恢复 prompt、订阅 timeline，并读取持久化 session。
2. `App` 管理 session list、历史 Turn/Attempt、interrupted 恢复提示、用户输入和可见状态。
3. `index.css` 提供两栏 Desktop walking-skeleton 布局。

## 约束

- 不直接渲染 raw adapter event。
- browser-only 模式必须给出清楚错误，不产生未捕获异常。
- 当前只提供 Alva EngineTarget；session list 不泄露 engine-native 细节。
- interrupted 会话保持可选择、可继续，不能伪装成 running 或普通 completed。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| React entry | `main.tsx` | 挂载应用 |
| Product UI | `App.tsx` | workspace、composer、timeline |
| IPC bridge | `bridge.ts` | Tauri invoke/listen/dialog 封装 |
| Styles | `index.css` | Desktop 视觉与响应式布局 |
