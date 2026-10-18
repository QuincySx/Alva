# L6/L7 · Applications
> 面向用户的产品与进程入口，不是通用 SDK 或默认 Agent 配方。

## 地位

容纳 L6 独立产品入口与 L7 Hub 多引擎产品。目录分类不合并各产品 runtime、存储或授权边界；Go module 仍只有根目录的一份 go.mod。

## 逻辑

`cmd` 仅解析显式参数并拥有进程退出；direct CLI/desktop 直接消费 harness，Hub desktop/server 消费同一 `apps/hub/bootstrap` 装配，Gateway 则只处理显式模型协议请求，不启动 Agent。

基础 `desktop` 拥有共享自绘控件与 direct 界面，Hub 界面位于 `apps/hub/desktop`；后者可消费基础控件，前者不反向依赖 Hub/engine。Hub 核心产品事实只依赖 engine contract，具体存储、服务、装配与界面继续分包。

这些是源码职责目录。运行direct桌面时，`cmd/alva-desktop`负责启动、`desktop`负责界面、`direct`负责会话和执行资源；运行多引擎桌面则从`cmd/alva-hub-desktop`进入`hub`。`debug`是默认关闭的观察能力，`gateway`是独立模型API入口，均有独立生命周期。

## 约束

不把用户产品政策下沉到 SDK；不引入旧路径 alias、React/WebView、历史兼容或跨产品默认授权。目录已归类不代表功能全迁完，状态以根 MIGRATION.md 为准。运行命令从 Go module 根执行，入口为 `./apps/cmd/...`。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 进程入口 | `cmd/` | direct CLI/desktop、WASIp1 worker、Hub desktop/server 与 Gateway 的显式启动/关闭 |
| Direct 自绘界面 | `desktop/` | 共享控件、有限呈现状态与直接 harness 界面，不拥有 Agent 执行 |
| Direct 耐久应用 | `direct/` | 独立native facts/workspace/route与Controller owner、冷历史/文件引用/逐调用审批，不依赖Hub/engine |
| 原生 UI diagnostics | `debug/` | 独立默认关闭的认证日志/level、实际视图状态/语义树/窗口PNG与声明action，不拥有Agent/Hub事实 |
| 模型 Gateway | `gateway/` | 独立认证 HTTP/SSE 模型 API，真实协议转换，不是会话或 Agent runtime |
| Hub 产品 | `hub/` | 多引擎会话/Team/自动化/worktree 与共享产品服务、bootstrap、UI、HTTP、SQLite |
