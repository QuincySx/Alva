# Alva Hub 本地服务 + 多前端 —— 调研与方向

> 状态：调研结论，尚未实现。本文记录对 DeepSeek Harness（DSH）的形态调研，
> 以及 Alva 对齐该形态的差距分析与路线。用于后续 "headless hub 服务 +
> 网页/TUI/桌面 多前端" 的决策依据。

## 一、结论摘要

DSH（`deepseek-ai/deepseek-harness`，10 万+ star）的核心形态是：

```
一个本地服务进程（host，持有 LLM 能力 + 会话状态）
   ↑ HTTP POST（上行 unary RPC）+ WebSocket（下行事件流）
   ├─ Web GUI（默认，127.0.0.1:3080）
   ├─ headless（同一内核，不 mount 任何 HTTP/端口）
   └─ TUI / Electron / 其他前端 —— 消费同一套 wire 协议
```

Alva 想达到同样形态，**不需要照搬 DSH 的 TypeScript/Cordis，也不需要重写内核**。
唯一需要补的一片是：**在 `alva-hub-desktop::lib`（已有业务逻辑）之上加一个
前端无关的 hub wire 协议 + HTTP/WS 载体**，让 Web/TUI/Tauri 全部降级为协议患者。

## 二、DSH 的架构（调研来源：本地 195 个 `@deepseek-ai/*` 包 + GitHub 源码）

### 分层（每层只做一件事，依赖方向可审计）

```
apps/web（Vite 薄壳，main.ts 调 dsh-client-web）
apps/cli（dsh bin：dsh web / dsh --profile headless）
   │
packages/host/*        packages/client/*
  apiproxy   协议层（TS+Zod，零 Node 依赖，浏览器可 import）
  webserver  http + upgrade 路由，不认识业务
  runtime    装配层（哪些插件以什么默认挂载，唯一决策点）
  frontend-static  SPA dist + index.html transform
   │ 仅通过 /api 与 /client 两个子路径消费
harness core（cordis 内核）
```

关键纪律：

- `packages/host/*` 只管 host 侧能力；`packages/client/*` 只管 client 侧，**单边**。
- client 包 **永不 import** host 包 runtime，只消费协议层的 `/api` 和 `/client`
  两个浏览器安全的子路径。
- 新增一个应用 = 选一个 fetch 载体（浏览器同源 HTTP / in-process / 未来 Electron
  IPC）+ 写一个 `apps/` 下的装配模块，**装配不成为 package**。

### 四象限 wire 协议（channel-independent）

```
            客户端发起                服务端发起
请求   ① ClientRequest          ③ ServerRequest
       POST /api/<method>       WebSocket/SSE 帧（事件/审批/问答）
响应   ② ServerResponse         ④ ClientResponse
       该 POST 的应答体          POST /api/respond（回填 ③ 的 rpcId）
```

纪律：

1. `rpcId` 谁发起谁铸造，响应永远回显、绝不新造。
2. 业务错误走 RPC 的 error 分支（`RpcResult<T> = {ok, value} | {ok:false, error}`），
   **方法不 throw 业务错误**；HTTP status 只表达载体。
3. 四象限与物理通道解耦——换载体（HTTP→WS→in-process fetch→Electron IPC）
   四象限不变。DSH 刻意保留一个 in-process SSE 编解码去验证同构性。
4. 浏览器下行用**两条独立 WebSocket**（`events.mux` + `events.host`），上行仍 HTTP
   POST。原因：HTTP/1.1 每 origin 约 6 连接，两条永久 SSE 会挤爆连接池。

### 三条值得借鉴的设计

- **安全围栏在载体层做一次、覆盖所有 `/api`**：415 拒绝非 `application/json`
  （杀光 cross-site「simple」请求）+ Host/Origin 校验（防 DNS rebinding）。
  DSH 明确这是 "confused-deputy 防御，不是认证"；远程访问 v1 明确不做、不做认证。
- **Electron 是二等公民**：复用同一份 SPA dist，fetch 走 IPC 桥，不是独立产品。
- **headless 是同一 base 的一个 patch**：不 mount 任何 Host/HTTP/端口，同一内核两个面。

### Cordis 内核机制（与 Alva 内核对比的事实基础）

Cordis 内核 = 运行期依赖注入 + 作用域 + 生命周期运行时：

- `Context` 是 **Proxy**，普通属性读走 service resolver；`extend()`/`isolate()`/
  `intercept()` 造有作用域的子 context，不 mutate 父级。
- `Service` 构造时 `ctx.provide(name, this)`，随 owning **Fiber** 自动卸载。
- `Fiber` 有显式生命周期状态机（PENDING/LOADING/ACTIVE/FAILED/UNLOADING/DISPOSED），
  支持 `dispose()` / `restart()` / `update()`；effect 产生的 disposer 逆序回收。
- `isolate(name, label)` 在子 context 给同名 service 开独立作用域——**同一名字在
  不同作用域/时刻指向不同实现**，这是 DSH 能 "host 一棵树 + browser 平行一棵树 +
  session scope ×N" 的基础。
- 类型手段：`unique symbol` + declaration merging + 跨 realm 的 `global symbol`
  brand check（`Context.is()` 跨多个 cordis 副本有效）。

Alva 内核 = 编译期装配 + 洋葱中间件 + agent loop：

- `Plugin` trait 只有 `register()`（装配期）+ `finalize()`（晚期接线），**无运行期生命周期**。
- `Registrar` 单一装配句柄，能力收进 `PluginHost`。
- `provide::<T>` 是**单值语义**（bus 按类型存一个值，后覆盖前）；要"多个同类型贡献者"
  须让 `T` 自身是注册表。
- 无作用域隔离、无 proxy 解析、无 fiber 卸载；装配一次性（build 定型），
  运行期靠 Middleware 洋葱 + Tool 执行。

## 三、Alva 现状与差距

### 已具备的地基（对应 DSH 各层）

| DSH 层 | Alva 现有 | 差距 |
|---|---|---|
| 协议层 apiproxy | **无** | **唯一真正缺口** |
| 装配层 runtime | `alva-hub-desktop/src/lib.rs`（run/readiness/providers/store/permissions/team 纯函数） | 已具备，可被非 Tauri 复用 |
| 载体层 webserver | `alva-app-gateway`（axum，仅 LLM 代理，不承载 hub 会话） | 有 HTTP 能力，缺 hub 路由 |
| client 插件 | `alva-hub-desktop/web/src/components/*`（16+ 组件已拆） | 已模块化 |
| 应用装配 | `main.rs`（1546 行，几乎全 `#[tauri::command]`） | 业务已抽到 lib.rs，main.rs 剩 IPC 胶水 |

### 两个关键发现

1. **`bridge.ts` 已预留「非 Tauri」裂隙**：`hasTauri` 常量 + `DEMO` 模式，
   每个函数 `if (DEMO) / if (!hasTauri) / invoke(...)` 三段式。把 `invoke()` 换成
   `fetch()` 是机械的点对点替换，React 组件几乎不动。

2. **业务已与 IPC 分家**：`main.rs` 的 `#[tauri::command]` 多为薄壳，真逻辑在
   `lib.rs` 导出的 `run_once/run_persisted/HubStore/build_engine/readiness` 等
   不依赖 Tauri 的纯函数。headless 服务可直接消费 `lib.rs`。

### 需要补的唯一一块

在 `lib.rs` 之上加一层薄 HTTP，把 `bridge.ts` 里约 50 个 `invoke` 命令变成 HTTP
端点，把 `listen("hub_timeline")` 变成 WebSocket/SSE 推送。工作量是「协议化 + 换
carrier」，不是「重写业务」。

## 四、内核对比的诚实结论

1. **哲学层同构**：都是"稳定瘦内核 + 一切皆插件注入"。成立。
2. **机制层互补，不可比"谁更严谨"**：
   - Cordis 强在**运行期组合表达力**（作用域隔离、fiber 生命周期、热插拔），
     Alva 的 `provide::<T>` 单值语义恰好暴露其在"多实例作用域"上的薄弱。
   - Alva 强在**编译期可控性**（trait 契约、`BusWriter` vs `BusHandle` 读写分离、
     `AgentAssemblySnapshot` 装配可观测），这是 Rust 所有权模型给的、Cordis（JS）给不了的。
3. 早先"Alva 内核更严谨（类型安全 trait）"的说法**不成立，已撤回**。
4. 对"本地服务 + 多前端"的启示：DSH 多前端共享协议的能力**靠 Cordis 的作用域与
   生命周期撑起来**，Alva 的扁平装配模型在"一个进程承载多个并发 session/前端"时
   会遇到 Cordis 已解决的作用域问题，须在实现时留意。

## 五、路线（按保守→激进）

- **路线 A：复用 `alva-app-gateway`，加 hub 服务 bin（最小，推荐起始）**
  在 gateway 加 hub 路由组（axum），handler 调 `alva-hub-desktop::lib` 纯函数；
  `bridge.ts` 加 `HttpBridge` 实现。改动面：协议层 + bridge transport，lib/组件/store
  几乎不动。收益：立刻得到浏览器可开的 Web 面，同时保留 Tauri。

- **路线 B：Tauri 降级为薄壳（DSH 的 Electron 模式）**
  `main.rs` 改成「启动本地 hub 服务 + wry 加载同一份 dist」。改动面中等。

- **路线 C：TUI 也吃掉协议（三形态统一）**
  `alva-hub-cli` 从直连引擎改成消费 hub 服务的客户端。改动面中大。

建议节奏：先 A 打通 Web，稳定后再评估 B/C。

## 六、待决策

唯一需要产品拍板的点：**hub 服务暴露给谁、是否鉴权**。

- DSH 选择：`127.0.0.1` 本地 + 载体层信任围栏，远程访问 v1 不做、不做认证。
- 若目标「本地跑个服务自己玩」，照抄 DSH 的 loopback 姿态最省事。
- 若目标「能给他人远程用」，协议层须从一开始带鉴权（token/mTLS），设计完全不同。
