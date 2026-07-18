# Alva 全仓宏观架构与实现细节 Review

- 审查日期：2026-07-18
- 审查提交：`7acab43d9196cfef79410a26ef28b44e0461e8e5`
- 分支：`main`
- 仓库：`/Users/smallraw/Development/QuincyWork/alva-agent`
- 审查方式：全仓静态审查、关键调用链追踪、依赖边界检查、Rust/前端构建与测试验证

## 总体结论

架构方向成立：分层命名清楚，Plugin/Tool/Middleware 扩展模型一致，Rule 17 依赖防火墙有效，WASM sandbox 的能力边界尤其扎实。

但当前版本不建议发布：存在 1 个 P0、多个生产路径 P1，以及若干“当前未接入、启用即危险”的潜伏缺陷。现有测试全部通过，但没有覆盖这些对抗性条件和并发时序。

## 阻断级问题

### 1. [P0] 默认开启的 `read_url` 可以绕过 SSRF 防护

安全层只检查初始 URL，并明确接受 DNS TOCTOU；实际请求自动跟随最多 5 次重定向，且没有重新验证每一跳。因此公开地址可以重定向到 `127.0.0.1`、私网或云 metadata 服务，DNS rebinding 也可绕过预检。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-security/src/url_info.rs:270`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-security/src/url_info.rs:277`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-extension-builtin/src/read_url.rs:161`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-extension-builtin/src/read_url.rs:223`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/components.rs:127`

此外，`resp.text()` 会先将整个响应读入内存，`max_length` 只在下载完成后截断，仍可造成内存 DoS。

建议复用 WASM sandbox 已有的实现：禁用自动重定向、逐跳解析并分类 IP、限制响应体、限制跳数，并考虑 DNS pinning。

### 2. [P1] Workspace 文件边界可被父目录 symlink 绕过

完整路径存在时会 canonicalize；新文件不存在时退回纯词法 normalize。若 `workspace/link -> /outside`，写入 `workspace/link/new_file` 会被判定在 workspace 内，实际写到外部目录。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-security/src/authorized_roots.rs:38`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-security/src/authorized_roots.rs:72`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-extension-builtin/src/local_fs.rs:151`

修复必须 canonicalize 授权根与最近存在的父目录，再以安全相对路径创建叶节点；仅做字符串或组件 normalize 不够。

### 3. [P1] UI 的手动工具 allow-list 完全没有生效

前端明确承诺手动模式只向模型暴露用户选择的工具，并传递 `tool_names`；Rust 后端仅记录一条 `filtering is TODO`，所有工具仍然可用。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/web/src/store/appStore.ts:147`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/web/src/routes/Home.tsx:443`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/src/agent/run.rs:105`

这是安全语义错误。用户以为关闭了 Shell/文件工具，模型实际仍可调用。

### 4. [P1] 多窗口或并发发送可能把 A 的消息运行在 B 的 session 上

Tauri 的 `turn_start_lock` 在 `prompt_text()` 返回后释放，但 `prompt_text()` 只是 spawn task；task 尚未拿到全局 AgentState 锁。此时另一请求可以先 `swap_session()`，然后 A 的后台 task 才获取状态并在 B 的 session 上执行。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/src/agent/run.rs:88`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/src/agent/run.rs:136`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/base_agent/agent.rs:60`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/base_agent/agent.rs:155`

单一 `current_cancel` 也会指向最后排队的任务，而不是实际正在执行的任务。

## 核心运行时 P1

### 5. 错误路径跳过完整生命周期清理

`on_agent_start` 或 `input_committed` 返回错误时，函数直接 `?` 返回，不会执行 `on_agent_end`、context dispose、`run_end` 或 `AgentEnd`。消费者可能只看到事件流突然关闭。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:333`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:384`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:411`

应把启动、输入提交和主循环全部放入统一的 try/finally 式结构，保证任何已开始的 run 都有且只有一个终止事件。

### 6. LLM 流无法及时取消，也没有网络超时

取消仅在迭代开始检查；正在 `stream.next().await` 时不会 select cancellation token。四个 provider 都使用无超时的 `Client::new()`，连接或 SSE 永不返回时，Cancel 也无法结束任务。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:213`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:463`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-llm-provider/src/provider/anthropic.rs:68`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-llm-provider/src/provider/openai_chat.rs:53`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-llm-provider/src/provider/openai_responses.rs:50`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-llm-provider/src/provider/gemini.rs:55`

### 7. 长中文或 Emoji system prompt 可直接 panic

代码用字节长度计算 `len - 200`，再对 UTF-8 `String` 做字节切片；切点落在多字节字符中间就会 panic。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:515`

### 8. SDK `Agent::run()` 宣称 streaming，实际等运行结束才返回 receiver

它先 await 完整 `run_agent()`，期间所有事件堆积在 unbounded channel。调用方既无法实时消费，也可能产生大量内存占用。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-core/src/agent.rs:78`

### 9. Tool middleware 出错会留下不完整的 tool call 序列

ToolUse 已提交后，`before/wrap/after` 的普通错误会直接返回，没有为当前及剩余 tool call 补 ToolResult。下一次 provider 请求会看到悬空 tool call，部分 API 会直接拒绝该历史。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/tool_batch.rs:51`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/tool_batch.rs:73`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/tool_batch.rs:123`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/tool_batch.rs:133`

### 10. 默认 Compaction 会删除整个 append-only 事件日志

压缩调用 `session.clear()`，不仅删除旧消息，也删除 `run_start`、LLM、tool、因果父节点等全部事件，然后重建无 parent 的消息。这会破坏 Inspector、评估、持久化审计和 session 因果图，而且该中间件默认开启。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-context/src/middleware.rs:303`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/components.rs:109`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/components.rs:439`

### 11. `AgentSession` 合约与运行时实现不一致

Trait 文档保证运行时会在 agent end、每 10 iteration/30 秒和进程关闭时调用 `flush()`；kernel 实际一次也没有调用。SDK 用户使用持久化 backend 时，durability 错误不能按承诺暴露。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-abi/src/agent_session.rs:254`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-abi/src/agent_session.rs:281`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs:324`

其他合约问题：

- 文档要求 close 后其他操作返回错误，但大量读取/写入方法本身没有 `Result`，类型层面无法兑现。
- `ListenableInMemorySession` 在分配 seq 后释放锁再异步广播，并发 append 可能先广播 seq 2、后广播 seq 1。
- 广播持有 listener read lock，并串行 await 任意 listener，一个慢 listener 会阻塞 append 和订阅。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/agent_session.rs:386`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/agent_session.rs:425`

## 当前未接入生产、但启用即危险

### 12. [P1] Environment installer 存在目录删除逃逸和 ZIP Slip

当前只发现 `alva-app-core` re-export，未发现 CLI/Tauri 调用，所以不是现行产品攻击面；但公开 API 默认启用 download feature。

风险包括：

- Manifest component 名直接 `base_dir.join(name)`，随后 `remove_dir_all`；`../../victim` 可删除 base_dir 外目录。
- ZIP entry 原始名称直接 join 到目标，未使用 `enclosed_name()`；`../payload` 可写出安装目录。
- archive 文件名同样未经校验。
- 下载无 checksum/signature、无尺寸限制，并一次性缓冲全部响应。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-environment/src/environment/config.rs:50`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-environment/src/environment/installer.rs:50`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-environment/src/environment/installer.rs:91`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-environment/src/environment/installer.rs:143`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-environment/src/environment/installer.rs:200`

### 13. [P1] Skill repository 的路径遍历检查无效

`root.join("../secret")` 在词法上仍然 `starts_with(root)`，随后文件系统会解析 `..`；symlink 同样可以逃逸。

安装时还使用 SKILL.md frontmatter 的 `meta.name` 构造目标，并在目标存在时 `remove_dir_all`。恶意 `name: ../../victim` 可删除用户目录外的数据。当前默认 Skill Tool 只使用正文加载，未发现产品 UI 调用 install，因此属于潜伏风险。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-skill/src/fs.rs:330`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-skill/src/fs.rs:405`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-skill/src/fs.rs:448`

## 协议、Graph 与能力库问题

### 14. AEP 子进程插件可能永久挂起

JSON-RPC pending request 没有内部 timeout；请求 future 被取消或 writer send 失败会遗留 pending sender，initialize 也无超时，一个失控插件可阻塞 Agent build。stdout 的单行 frame 也没有大小限制。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-extension-loader/src/dispatcher.rs:146`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-extension-loader/src/proxy.rs:238`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-extension-loader/src/subprocess.rs:140`

### 15. ACP Delegate 存在完成事件丢失竞态和进程回收缺口

Delegate 先发送 prompt，后订阅 broadcast；快速子进程可以在订阅前完成，调用方随后等待 900 秒。该 timeout 每收到其他进程事件还会重新计时，并非绝对 deadline。

Process shutdown 只发送消息，不等待、不 kill；manager 的 restart/heartbeat 配置也未实现。stdout reader 和 child waiter 还会竞争覆盖进程状态。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-acp/src/delegate.rs:103`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-acp/src/delegate.rs:122`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-acp/src/connection/process.rs:125`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-acp/src/connection/process.rs:158`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-acp/src/connection/process.rs:192`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-protocol-acp/src/connection/manager.rs:19`

### 16. Graph 会静默丢失嵌套 fan-out，并存在非确定性合并

并行节点返回 `NodeResult::Sends` 时只 warn 然后丢弃。没有 merge_fn 时，最终状态取 JoinSet 最后完成的 update，结果取决于调度顺序。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-graph/src/pregel/parallel.rs:76`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-graph/src/pregel/engine.rs:134`

### 17. 默认 Memory 的 hybrid ranking 会制造假相关结果

默认 embedder 返回空向量；vector search 仍返回所有 chunk 的 0 分，normalize 又把所有相同分数转换成 1，最终任意内存获得默认 vector 权重，可能压过真实 FTS 结果。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-memory/src/embedding.rs:26`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-memory/src/in_memory.rs:118`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-memory/src/service.rs:122`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-memory/src/service.rs:224`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-extension-builtin/src/wrappers/memory.rs:39`

其他 Memory 问题：

- 持久化扫描使用 `.follow_links(true)`，没有 canonical workspace 边界检查，可索引 workspace 外的 `MEMORY.md`。
- 删除旧 chunk、upsert file、插入新 chunk 不是事务；中途失败可能留下“hash 已更新、chunk 不完整”的状态，下一轮同步误判 unchanged。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-extension-memory/src/sync.rs:62`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-extension-memory/src/sync.rs:105`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-memory/src/service.rs:94`

### 18. MCP 产品组件当前没有真实 transport

应用层插件始终使用 `StubTransport`。即使配置真实 stdio/SSE 服务器，也发现不到任何工具；代码本身已记录 `no working transport implementation`。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/extension/mcp/extension.rs:19`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/extension/mcp/extension.rs:131`

### 19. Embedded gateway 会报告假成功

Gateway 在后台 task 内才 bind，却立即向 UI 返回成功；端口占用时仍报告 started。重启同一端口还会先让新 bind 失败，再 abort 旧实例，最后一个都不剩。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/src/agent/gateway.rs:40`

### 20. API key 存储缺少操作系统级保护

Provider config 直接保存包含 API key 的完整 JSON，未设置 Unix 0600、未原子替换；前端也把完整 provider config/API key 持久化进 localStorage，同时 Tauri CSP 为 null。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-llm-provider/src/config.rs:151`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/web/src/store/appStore.ts:410`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/tauri.conf.json:22`

## 宏观架构评价

### L0 Wire / Sandbox ABI

边界最干净，零 workspace 依赖、WASM 编译链完整。WASM sandbox 的能力显式授予、逐跳 URL 验证、资源限额、路径翻译和 audit 边界是当前仓库最可靠的安全实现，值得作为 native 路径的标准。

### Kernel ABI / Core

Trait/value 分层思路正确，但 ABI 已包含运行时实现逻辑：

- `crates/alva-kernel-abi/src/tool/scheduler.rs`：987 行
- `crates/alva-kernel-abi/src/scope/context/apply.rs`：775 行

稳定契约层开始被 scheduler、async apply 等执行逻辑侵蚀，应迁往 kernel-core 或具体能力 crate。

### Agent Core / Plugin

`Plugin + Registrar + finalize`、同名默认替换契约清楚，是仓库最成功的架构决策之一。应继续坚持 kernel 和 agent-core 不接受功能扩展。

### L3/L4 能力与扩展

依赖方向总体正确，但 security、memory、context 等默认能力的行为契约缺少对抗性测试。当前主要风险不是分层，而是默认实现的安全和一致性。

### App Core

作为 composition root 合理，但已经承担大量组件目录、默认策略和协议装配，应避免继续接收业务实现，防止变成不可替代的 God assembler。

### Engine Runtime 集群

确认零生产消费者，和根文档一致。应坚持 2026-12 的退役检查点，在出现真实消费者前不要继续扩展。

### CLI/Tauri

共用 component catalog 减少了装配漂移，但“单例 Agent + 可交换 session”的模型不适合多窗口并发。应该让一个运行中的 turn 捕获不可变的 session、tool snapshot 和 ModelConfig。

## 架构治理漂移

### Bus 规则与实现冲突

`alva-kernel-bus/src` 实际约 970 行，并公开 Bus、Caps、StateCell、BusEvent、EventBus、BusHandle、BusPlugin、PluginRegistrar、BusWriter；规则文档仍强制“不超过 800 行、只暴露 4 个类型”。`BusPlugin` 还与 agent-core `Plugin` 形成第二套插件抽象。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/docs/BUS-RULES.md:247`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-bus/src/lib.rs:21`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-bus/src/plugin.rs:108`

### 分形文档协议大面积失守

协议要求每个 Rust 文件前三行严格为 INPUT/OUTPUT/POS、每个源码目录有 AGENTS.md。实际结果：

- Rust 文件：540
- 三行头不合规：170，约 31.5%
- 含 Rust 源码但缺少 AGENTS.md 的目录：56

部分现存文档仍引用已迁走或不存在的 SQLite、`apply.rs`、双层 loop 等结构。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/FRACTAL-DOCS.md:19`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-memory/AGENTS.md:3`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-context/AGENTS.md:12`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/AGENTS.md:8`

## CI 与测试覆盖

本次执行且通过：

- `scripts/ci-check-deps.sh`
- `cargo test --workspace --exclude alva-app-tauri`
- `cargo check -p alva-app-tauri`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets`，退出码 0，但仍有较多 warning
- Tauri 前端 `tsc -b && vite build`

现有 CI 盲区：

- Rust test/clippy 明确排除 Tauri。
- 没有前端 build/test job。
- 前端 `package.json` 没有 test 脚本。
- coverage 没有阈值。
- clippy 为 report-only，不阻断合并。

证据：

- `/Users/smallraw/Development/QuincyWork/alva-agent/.github/workflows/ci.yml:14`
- `/Users/smallraw/Development/QuincyWork/alva-agent/.github/workflows/ci.yml:94`
- `/Users/smallraw/Development/QuincyWork/alva-agent/.github/workflows/ci.yml:97`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-tauri/web/package.json`

## 建议修复顺序

1. 立即处理 SSRF、native 文件 symlink 逃逸、工具 allow-list。
2. 修复 per-turn session 所有权、取消链和统一生命周期终止。
3. 修复 tool call/result 原子性、compaction 对事件日志的破坏、session flush 合约。
4. 封住 Environment/Skill 所有路径输入，并给归档下载加入校验、限额和原子安装。
5. 修复 ACP/AEP deadline、进程回收和 Graph 静默丢分支。
6. 收敛 ABI 运行逻辑、MCP/Engine 未完成表面、Bus 双插件系统和分形文档债务。
7. 将 Tauri Rust、前端构建和最小前端测试加入 CI gate。

## 复核提示

现有测试通过不代表上述问题不存在：大部分缺陷依赖恶意重定向、symlink、新文件路径、UTF-8 字节边界、并发调度顺序、快速子进程响应、永不结束的 SSE 或错误中间件等条件，当前测试没有覆盖这些情形。
