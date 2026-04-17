# alva-app-tauri

> Tauri 2.x 桌面壳 + React/Vite/TS 前端。GPUI 版已删除,本 crate 是唯一桌面 GUI。

## 分层定位

L6 应用层。

- **Rust 侧** (`src/main.rs` + `src/agent.rs`):Tauri 运行时 + `alva_app_core::BaseAgent` 的薄桥接。
- **前端** (`web/`):Vite + React + TS + Tailwind。`src/agent-bridge.ts` 封装 `invoke` / `listen`,`src/App.tsx` 是 MVP Chat UI。

## 架构决策(为什么是这套)

1. **Tauri(不是 Electron,也不是 GPUI)**:体积(3–10MB vs 100MB+)+ 纯 Rust 后端 + AI 训练数据密度高。GPUI 因为语料稀薄、API 还在动,让 AI 独立维护成本过高。
2. **IPC `emit`/`listen`(不是内嵌 HTTP/SSE)**:最直接,少一层端口和路由。
3. **前端栈 React + Vite + TS + Tailwind + Zustand(预留 shadcn/ui)**:AI 训练数据密度最高的组合;不用 Next.js(webview 里不需要 SSR)。
4. **只移除 `alva-app-debug` / `alva-app-devtools-mcp` 依赖**:Tauri 下 Chrome DevTools 原生可用,不需要自造视图树检查器。

## 如何跑

```bash
# 1. 安装前端依赖(首次)
cd crates/alva-app-tauri/web && npm install

# 2. 启动 Tauri dev(会自动通过 beforeDevCommand 启动 Vite)
cd crates/alva-app-tauri/web && npm run tauri dev
# 或直接 cargo:
cd crates/alva-app-tauri/web && npm run dev  # 另开终端跑 vite
cargo run -p alva-app-tauri                   # Tauri Rust 侧
```

需要 `ANTHROPIC_API_KEY` 或 `OPENAI_API_KEY` 环境变量(或在 UI 里粘贴 key)。

## 下一步(按序排)

- [ ] MessageUpdate 流式渲染(当前只在 MessageEnd 整段追加)
- [ ] 会话侧边栏 + 多会话切换(迁 `alva-app/src/models/workspace_model.rs` 的语义)
- [ ] Settings 面板 + provider 配置持久化
- [x] Session inspector(已内嵌为 Inspector 组件 + 独立窗口,session_projection port 自原 eval)
- [x] ~~删除旧 `alva-app`(GPUI)~~ 已完成

## 重要:分形文档协议

按 `FRACTAL-DOCS.md` 三层规范,`src/` 下每个模块都要有自己的顶部注释 `INPUT / OUTPUT / POS`,`agent.rs` 已经按这个格式写了。
