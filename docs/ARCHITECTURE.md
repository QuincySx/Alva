# Alva Architecture

Go 是唯一受维护的实现，module 根目录就是仓库根目录，module path 为 `github.com/QuincySx/Alva`。
本页描述当前职责和依赖方向；实际接口以源码/公开快照为准，能力与平台状态见 [MIGRATION](../MIGRATION.md)。

## 分层

| 层 | 路径 | 职责 |
|---|---|---|
| L0 | sdk/llm/wire、sdk/kernel/bus、sdk/sandbox/abi | 纯数据、类型化能力发现/独立 Topic、沙箱能力 ABI |
| L1 | sdk/kernel/abi | 模型、工具、窄对话日志、上下文与执行契约 |
| L2 | sdk/kernel/core | 可替换循环、中间件协调、模型/工具事实与真实收尾 |
| L2.3 | sdk/agent/abi | Task、Scope、Spawn、Analytics、Provider 等 Agent 语义 |
| L2.5 | sdk/agent/core | Plugin/Registrar、晚期接线、SDK Agent 装配 |
| L3 | sdk/agent/{context,memory,security,analytics,graph} | 可复用能力；Graph 是独立 runtime |
| L4 | extension | 可选 Tool/Plugin/Middleware |
| L4.5 | integration/engine 根契约 | Hub 驱动完整 Agent 的轻量契约 |
| L5 | harness | BaseAgent 默认配方、引用策略、历史与子会话政策 |
| L6/L7 | apps；根 cmd 启动 | direct、Gateway、Hub 与共享 apps/ui 呈现；direct 桌面位于 apps/direct/desktop |

integration/platform 为横向责任组，不代表所有内容都处于 SDK 之下。
LLM codec/provider 分开转换与传输；协议不拥有授权和持久化政策。
platform/host 拥有执行、审批/问题 rendezvous 和资源生命周期，system 提供底层宿主原语，
storage 保留 native records/artifacts 权威，sandbox 实现 host/guest；纯 ABI 留 SDK。

## 两类装配与产品入口

```text
第三方 harness → sdk/agent/core Builder → sdk/kernel/core AgentLoop
我们的 direct CLI/desktop → harness BaseAgent → 同一 SDK 内核
Hub CLI/desktop/server → apps/hub/product + bootstrap
                     → engine 根契约 → Alva 完整 harness / 外部 process adapter
Gateway → provider + codec（不运行 Agent）
```

Plugin 注册同步封闭贡献，Seal 后注册集固定，Finalize 只读接线完整能力。
默认 harness 按 Plugin.Name 跳过同名默认项，不把替换能力做成零散 setter。
Tool、Plugin、Middleware 各有独立职责；AgentLoop、Graph、EngineRuntime 各有实际消费者，
不合成统一 runtime，也不为未来可能性预加 hook。

planning/interaction/utility 根包为普通工具能力，可以只用内存 SDK 与宿主绑定的服务/引用运行。
各自 native/ 子包才依赖具体会话、Guard 和持久证据；Hub 装配明确导入该适配，不把这些政策
变成普通工具的前置条件。原生适配复用同一 schema/业务操作，保留 Plugin.Name、工具名与记录
namespace。工具目录读取调用方的实时 registry；偏好投影由明确的 BeforeModel hook 消费。
只有标准库、SDK 与纯 strictjson helper 可进入这三个根包，传递依赖也受静态闸约束。

## 强制边界

- SDK 不得直接/传递依赖 integration、platform、extension、harness、apps 或具体第三方 Provider/UI。
- kernel 不依赖 agent 语义/装配；窄 ConversationLog 不强迫用户采用 SQLite 或产品会话模型。
- direct 产品不进入 EngineRuntime；Hub core 只依赖 engine 根契约，具体适配器在 bootstrap 装配。
- UI 与 Inspector 观察已提交事实，不负责恢复、自动重放、执行取消或审批旁路。
- 取消须传播并真实 join；物理未完成的资源仍由原 owner 持有，关闭重试不能重放副作用。
- 文件/媒体/大结果/MCP/历史引用优先；直接父授权和 child 自有事实/日志不因共享 Bus 而放宽。

`tests/architecture` 按真实 tagged imports 强制依赖图、公开面和分形文档；
`tests/extensibility` 独立运行 SDK 消费者实验，手工窗口工具位于 `tools/qa`。
Bus 与 Registrar 的治理见 [BUS-RULES](./BUS-RULES.md)、[REGISTRAR-RULES](./REGISTRAR-RULES.md)。

## 技术栈与历史

桌面为固定 MyGo 自绘，无 React/WebView/npm；普通 Go 构建嵌入固定 QuickJS-NG WASM，
只有维护者显式重建该资产需要独立 WASI C 工具链。协议转换吸收固定 MIT Magpie 切片，
不吸收其完整应用、凭据发现或工具执行；SDK 不依赖其 DTO。

[ADR 0012](./adr/0012-go-native-stack.md) 记录技术栈和边界决策。
Rust 及旧 UI/构建工具已存于 `archive/rust-final-2026-10-18`；旧架构/产品规范可用
`git show archive/rust-final-2026-10-18:docs/ARCHITECTURE.md` 等命令查阅。
旧声明不自动成为当前实现或验收证据。

## 当前目录与入口

```text
cmd/                         八个独立 main：CLI、桌面、HTTP、MCP 桥、WASIp1 Worker
sdk/                         L0–L3；原公开包路径与契约保留
extension/                   L4；按能力组织的可选工具/插件/中间件
  files/                     模型参数、引用读取、备份与编辑政策
  security/                  actual Tool 身份/effect/HITL 装配
  diagnostics/               显式诊断命令与可替换 Manager
  planning/                  普通 Service 驱动的任务工具；native/ 为原生会话适配
  interaction/               普通 Handler 问答/引用；native/ 为原生 journal/期限适配
  utility/                   普通偏好/实时目录、timer/；native/ 为原生 CAS/授权适配
harness/                     L5 默认配方
integration/                 llm / protocol / engine 外部边界
platform/
  host/                      Controller、command/browser 资源 owner
    workspace/               独立 pinned 文件系统叶子，不依赖 host 父包
  system/                    底层宿主原语，严格 JSON 校验在 strictjson
  storage/                   native facts/artifacts、memory 与 SQLite 原语
  sandbox/                   实际 host/guest 与固定 QuickJS 资产
apps/
  direct/                    直接产品会话/执行 owner
    desktop/                 direct 窗口/诊断/订阅/耐久页面
  ui/chat/                   共享聊天呈现与原生控件
  ui/modules/                可信版本化消息 renderer
  debug/                     默认关闭的观察诊断
  gateway/                   独立模型 HTTP/SSE 产品
  hub/                       根调度契约及 product/bootstrap/sqlite/server/desktop 等子包
tests/architecture/          静态依赖、布局、分形、公开快照与转换来源闸
tests/extensibility/         独立 SDK 消费者实验；其他单元测试仍就近
tools/qa/                    显式手工窗口验收，不由普通测试启动
docs/                        当前架构、使用说明、治理、ADR 和 history
```

cmd 是启动程序的源码集合，CLI 是其中的终端交互形态；各入口有独立生命周期，使用对应产品
时才启动。L 编号是架构职责标记；运行阶段主要由 L2 协调，不按 L0→L7 顺序逐层调用。

共享 UI 不依赖 direct/Hub/harness/host/engine，产品通过回调接入实际 owner。direct 的私有
诊断状态随产品 wrapper 保留；共享模型仅提供明确的行数保留构造、只读序号和已存在运行的
观察重绑，不导出可变 ListState/内部序号，也不产生新执行。

workspace 只提供 Guard-bound 有界读/列目录/搜索、串行 Update、快照和根生命周期，不导出
os.Root。Update 在同一锁内读取当前字节、执行调用方变换/备份回调并原子替换；回调失败不写入，
回调不得重入同一 Workspace。引用/backend 和模型可恢复错误仍归 extension/files；未知工具
审批与实际 Tool 信任归 extension/security。宿主路径 Guard 不代替模型执行的 effect/HITL。

SDK、Graph/AgentLoop、默认 harness、EngineRuntime 的职责未合并；单 module 身份未改变。
所有实际消费者迁到新外围包路径，旧入口/UI/builtin/tooling/httpjson 路径不留 alias。
诊断 Plugin 的既有 Name("tooling")、配置键、工具名和记录 namespace 保留；包目录改名不重写事实。
后端合规套件、可选扩展、harness、协议/provider 和存储继续保留外部复用入口，未整体 internal 化。

2026-10-18 的初始覆盖表和整理依据完整保存在
[结构审查存档](./history/2026-10-18-structure-review.md)，其中数量/旧路径描述审查基线。
当前执行和剩余验收见根 [MIGRATION](../MIGRATION.md)，运行与构建见 [USAGE](./USAGE.md)。
