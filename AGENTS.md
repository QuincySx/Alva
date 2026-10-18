@/Users/smallraw/.codex/RTK.md

# Alva
> Go AI Agent SDK、可选扩展与独立产品。

## 地位

Go 是唯一受维护的实现，项目名为 Alva。go.mod 位于仓库根目录，module path 为 `github.com/QuincySx/Alva`；没有外层 go/ 容器。
进入代码前先读 [README.md 的可点击分层地图](./README.md#先看地图l0--l7)与程序入口表，再进入对应目录的 AGENTS.md。
Rust 实现、旧前端及配套工具已按用户明确要求提交归档并退出工作树；历史标签为
`archive/rust-final-2026-10-18`。不得为了参考历史重新引入运行桥或旧路径 alias。

## 逻辑

保留细分职责与依赖方向。稳定 SDK 内核只拥有通用机制；可选行为通过 Tool、Plugin、
Middleware 注入。确有缺陷可审计修复；抽象合并须先说明独立职责、实际消费者、收益与损失，
未有明确结论则保留。AgentLoop、Graph、EngineRuntime 是不同执行模型，不合成统一 runtime。

- Tool 是模型可调用的能力；Plugin 提供工具、中间件、服务、提示词与命令贡献；Middleware 拦截执行流程。
- SDK 入口在 `sdk/agent/core`，默认 harness 在 `harness`；同名 Plugin 替换对应默认项。
- Bus 仅保留类型化能力注册与显式独立 Topic；组件内已知依赖优先构造传参，服务生命周期由 owner 负责。
- direct CLI/desktop 直连 harness/host；Hub core 仅看 engine 根契约，具体 adapter 在 bootstrap 装配。
- 原始 native facts 与 Hub 产品事实分别持久化；UI/Inspector 是观察者，退订不等于 Stop。

## 约束

- SDK 不得直接或传递依赖 integration/platform/extension/harness/apps、MyGo 或具体 Provider；kernel 不反向依赖 agent。
- 不将产品默认、权限、凭据、存储后端或可选能力塞入内核；不增加 ad-hoc 默认能力 setter。
- 取消须传播并等待真实收尾；未完成的进程/回调清理仍由原 owner 持有，不能伪报完成或自动重放副作用。
- 文件、媒体、大工具结果、MCP 和历史引用优先；原件保存、派生、选择投递与真实回执分别记录。
- 不隐式发现旧配置、凭据或会话，不添加历史格式/数据兼容桥，不删除用户数据或外部共享缓存。
- 桌面采用固定 MyGo 自绘；无需 Rust/React/WebView/npm。QuickJS-NG 的固定 WASM 资产及 C 重建桥属于当前 Go 宿主能力，必须保留来源、哈希与许可。
- 遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md)：源码三行头、逐级 AGENTS.md 及级联地图更新。
- 公开面变化须显式更新 `tests/architecture/testdata/public-surface`；依赖和分形闸在 `architecture`。
- 测试、构建、真实平台/UI/外部账户验收分别报告；Go 转正不等于所有平台或产品验收完成，状态见 [MIGRATION](./MIGRATION.md)。
- 小步提交，每个逻辑改动独立 commit；使用 feat:/fix:/refactor:/chore:，非显然原因写入 body。
- 维护 AGPL-3.0-only 与第三方许可/归属；见根 LICENSE、NOTICE。

在仓库根目录执行主要检查：

```sh
go test -mod=readonly -tags desktop -race -p 1 -timeout 6m ./...
go vet -mod=readonly -tags desktop ./...
go mod verify
```

## 业务域清单

| 路径 | 层级/职责 |
|---|---|
| `cmd/` | 八个可执行程序入口：CLI、桌面、HTTP、MCP 调试桥与 WASIp1 Worker |
| `sdk/` | L0 wire/bus/沙箱 ABI；L1 kernel 契约；L2 通用循环；L2.3 Agent 语义；L2.5 装配；L3 能力库 |
| `extension/` | L4 可选 Tool/Plugin/Middleware |
| `harness/` | L5 默认配方、同名替换、引用/历史/子会话政策 |
| `apps/` | L6 direct/Gateway 与 L7 Hub；CLI/桌面/HTTP/MCP/Worker 程序入口在 cmd，Hub desktop/server 共用 bootstrap/product |
| `integration/` | 模型/协议/引擎适配；仅 engine 根契约为 L4.5，具体 adapter 是产品桥接 |
| `platform/` | host 执行/审批 owner、独立 workspace 文件能力、system 底层宿主原语、storage、sandbox 实现 |
| `tests/` | architecture 静态闸/公开快照/分形检查与独立 extensibility 消费者实验 |
| `tools/` | 显式维护和手工验收工具，不进入生产执行或普通测试路径 |
| `docs/` | 当前 Go 架构、治理、ADR 与明确标注的结构审查提案；实现状态以 MIGRATION.md 为准 |
| `.github/` | 唯一维护的 Go CI，无 Rust 构建/测试工作流 |
| `CONTEXT.md` | 领域词汇，不替代实际接口或验收记录 |
