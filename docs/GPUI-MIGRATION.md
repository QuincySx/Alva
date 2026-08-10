# Hub → GPUI 迁移评估

> 2026-08-10。目标：整个项目（alva-hub-desktop + alva-app-tauri）1:1 迁移到
> Rust 原生 UI（GPUI + longbridge/gpui-component ⭐12.5k，60+ 组件，Longbridge Pro 生产验证）。

## 现状（要迁移的）

| 层 | 现状 | 迁移后 |
|---|---|---|
| 壳 | Tauri 2（wry webview = 系统 WebKit） | **GPUI 原生窗口**（Zed 同源框架） |
| 前端 | React 18 + Vite + TS + Tailwind 3（32 测试） | **Rust GPUI**（Render trait，无 DOM） |
| 后端 | Rust（store/run/main/alva.rs，139 lib 测试） | **完全复用**（GPUI 是进程内，桥接层简化） |
| 通信 | bridge.ts invoke → tauri command | **Rust 直接调用**（无 IPC 层） |

**关键优势**：后端全是 Rust——store/run/failure/claims/worktree 一行不改。
Tauri command（main.rs 的 `#[tauri::command]`）换成 GPUI 内的直接调用，桥接层消失。

## 组件映射（App.tsx 现状 → gpui-component）

| Hub 现有 | GPUI 组件 |
|---|---|
| app-shell/sidebar/topbar/main | dock / sidebar / title_bar / status_bar / resizable |
| workspace/session/readiness/fallback/team 面板 | collapsible / accordion / group_box |
| session-item 列表 | virtual_list / searchable_list |
| readiness 卡片 / timeline 卡片 | card（styled）/ badge / tag |
| 状态徽章（healthy/failed/fallback） | tag / badge（主题色） |
| composer（textarea/input/select/checkbox） | textarea / input / select / checkbox |
| 折叠面板（fallback/team） | collapsible |
| details drawer | sheet / dialog |
| approval 组卡片 | dialog / notification |
| handoff 信封（markdown/列表） | markdown / text / description_list |
| 深色主题 + accent | theme（多主题 + 变量） |
| 搜索过滤 | searchable_list |
| 状态 pill（running/completed/fallback） | status_bar / tag |

## 影响（不可复用部分）

1. **web 测试（32 个 vitest）**：React 测试不适用 → GPUI 测试（gpui 测试上下文，Rust 侧）
2. **审查工具链**（wireframe/vision/a11y，CDP 驱动 webview）：
   - wireframe（DOM 遍历）→ 不适用 GPUI（无 DOM）。替代：GPUI 视图树反射（gpui 有访问树？或截图对比）
   - vision 审查（截图）→ **保留**（GPUI 窗口可截图）
   - a11y audit（DOM）→ 需要 GPUI 的 a11y（macos_accessibility.rs 在组件里！）
3. **?demo=1**：改为 GPUI 的 feature 开关（本地模式注入 demo 数据）
4. **打包**：GPUI 产物（macOS .app bundle，Zed 的打包方式）

## 阶段计划

1. **P0 骨架**：新 crate 接入 gpui + gpui-component，最小窗口 + 主题 + dock 布局 + 一个真实面板（session 列表）
2. **P1 主壳**：sidebar/topbar/timeline/composer 布局 + 引擎选择 + 折叠
3. **P2 数据打通**：Rust 后端直连（复用 HubStore），替换 tauri command
4. **P3 完整功能**：fallback 编辑/team run/details drawer/approval/handoff 信封
5. **P4 测试**：GPUI 测试 + 截图审查 + 打包
6. **P5 alva-app-tauri**：同路径迁移（BaseAgent 直连）

## 风险

- GPUI 生态年轻（API 变化快，需锁版本 git dep）
- 迁移是**数周级**工程（P0-P1 可先做，验证可行后继续）
- 审查工具链需适配（无 DOM）

## 决策（用户拍板，2026-08-10）

- **不要共存**：迁移完成后**整体删除**旧 Tauri 版（alva-hub-desktop 的 webview+React 层、
  alva-app-tauri 壳）。alva-hub-gpui 是唯一入口。
- **Web 需求走 GPUI wasm**：gpui-component 有 `story-web` 先例（wasm32 构建），未来要
  Web 端直接编译 GPUI 到 wasm，不保留任何 web 前端。
- 开发期旧版保留在仓库作功能参考（不维护新功能）；P5 完成、功能对齐后删除：
  - 删 `crates/alva-hub-desktop`（web/ + tauri 壳 + tauri.conf.json）
  - 删 `crates/alva-app-tauri`（若一并迁移；Rule 18 检查相应移除）
  - 删 `web/` 测试与审查工具链的 DOM 路径（wireframe/a11y audit 的 CDP 部分废弃，
    vision 截图保留——GPUI 窗口可截图）
- GPUI 依赖：git 锁定（Zed main + gpui-component main），crates.io 版太旧
