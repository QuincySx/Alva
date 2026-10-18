# L6/L7 · 可执行程序入口
> CLI、桌面、HTTP 服务、MCP 调试桥与 WASIp1 Worker 的启动代码。

## 地位

进程资源所有者；direct harness 与 Hub 多 engine 入口互相独立，不是可复用 SDK。

## 逻辑

cmd 表示可执行程序入口集合，CLI 是其中的终端交互形态。每个子目录是独立 main 包，按显式参数装配对应产品并拥有启动/退出流程；Agent 产品创建 runner/controller，Gateway 只处理模型 API，Worker 在 WASIp1 guest 中运行。退出按所属资源 owner 执行 cleanup。

## 约束

默认无外部模型请求、不发现配置或凭据；不依赖旧二进制、Tauri IPC 或 React。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| CLI | `alva/` | headless prompt、显式 live 与完整WASIp1/QuickJS冷启动集成验证 |
| WASIp1 Worker | `alva-worker-wasm/` | 有界host ABI注入环境/模型、同SDK实际循环与显式guest目录能力 |
| Desktop | `alva-desktop/` | `desktop` build tag 的 MyGo 自绘入口 |
| Hub Desktop | `alva-hub-desktop/` | 独立多 engine native 产品入口 |
| Hub CLI | `alva-hub/` | 独立共用bootstrap/product的one-shot/REPL，冷读sessions/history/handoff、耐久resume/手动切换，实际成功后才输出最终答复 |
| Hub Server | `alva-hub-server/` | 同源 product/native 的认证 loopback HTTP/SSE |
| Model Gateway | `alva-gateway/` | 独立显式route/stdio key、真实Magpie跨协议HTTP/SSE，非Hub或Agent loop |
| Devtools MCP | `alva-devtools-mcp/` | 显式numeric-loopback Hub/UI+独立token fd或私有文件，默认只读stdio；UI controls另需operator双端授权 |
