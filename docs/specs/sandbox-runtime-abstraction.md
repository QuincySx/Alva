# Sandbox Runtime Abstraction Design

> Status: Draft
> Date: 2026-03-24
> Reference: [chekusu/sandbank](https://github.com/chekusu/sandbank) (TypeScript, learned from)

## 1. Problem

Agent 工具（ExecuteShellTool, CreateFileTool 等）当前直接调用本地系统 API（`tokio::process::Command`, `tokio::fs`）。要支持 Docker / E2B / iOS / WASM 等多种执行环境，需要一层统一抽象。

## 2. Goal

提供 `Sandbox` trait 作为工具的执行后端，隐藏底层差异。工具不关心命令在哪跑、文件在哪存，只通过 trait 操作。

**不包含**：
- Keystore / Secrets 管理（独立外部工具，通过网络 API 按需获取）
- 具体 adapter 实现（本文档只定义 trait 和协议）

## 3. Architecture

```
┌─ Tool Layer ──────────────────────────────────────────┐
│  ExecuteShellTool / CreateFileTool / GrepSearchTool    │
│  调用 ctx.sandbox().exec() / write_file() / read_file()│
└───────────────────┬───────────────────────────────────┘
                    │ &dyn Sandbox
┌───────────────────▼───────────────────────────────────┐
│  alva-sandbox (core crate)                             │
│                                                        │
│  trait Sandbox        — exec / files / env / state     │
│  trait SandboxProvider — create / get / destroy         │
│  trait SandboxAdapter  — 后端实现者实现此 trait          │
│  SandboxCapability    — 可选能力枚举                    │
│  CreateConfig         — 统一创建配置                    │
│  EnvPolicy            — 环境变量隔离策略                │
│  SandboxError         — 错误体系                       │
└───────────────────┬───────────────────────────────────┘
                    │ impl SandboxAdapter
┌───────────────────▼───────────────────────────────────┐
│  alva-sandbox-local    — macOS Seatbelt / 直接执行      │
│  alva-sandbox-docker   — Docker container              │
│  alva-sandbox-e2b      — E2B cloud sandbox             │
│  (future)              — iOS / WASM / Fly.io           │
└───────────────────────────────────────────────────────┘
```

### 三层分离（学 Sandbank）

| 层 | trait | 谁实现 | 职责 |
|---|---|---|---|
| `SandboxAdapter` | 后端作者 | 原始操作：spawn container / exec / read file |
| `SandboxProvider` | SDK（`create_provider()`） | 包装 adapter：补 fallback、注入 env、注入 skills、事件观测 |
| `Sandbox` | SDK 返回给消费者 | 统一接口：exec / write_file / read_file |

消费者（工具层）只看到 `Sandbox`，不知道底层是 Docker 还是本地。

## 4. Core Traits

### 4.1 Sandbox — 实例接口（最小公约数）

```rust
/// 所有后端必须实现的最小接口。
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Sandbox unique ID.
    fn id(&self) -> &str;

    /// Current state (may be stale — use refresh_state() for accuracy).
    fn state(&self) -> SandboxState;

    /// Refresh state from backend (e.g., check if container is still alive).
    async fn refresh_state(&mut self) -> Result<SandboxState, SandboxError>;

    /// Execute a command inside the sandbox.
    async fn exec(&self, command: &str, opts: &ExecOptions) -> Result<ExecResult, SandboxError>;

    /// Write a file inside the sandbox.
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), SandboxError>;

    /// Read a file from the sandbox.
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, SandboxError>;

    /// List directory contents.
    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, SandboxError>;

    /// Check if a file/directory exists.
    async fn exists(&self, path: &str) -> Result<bool, SandboxError>;
}
```

**对比 Sandbank**：
- 增加 `refresh_state()`：解决 state 过时问题
- 增加 `list_dir()` / `exists()`：文件操作不只是读写，grep/list_files 工具需要目录遍历
- 去掉 `uploadArchive` / `downloadArchive`：用 write_file + exec tar 组合实现，不作为最小接口

### 4.2 SandboxProvider — 工厂接口

```rust
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    /// Provider name (e.g., "local", "docker", "e2b").
    fn name(&self) -> &str;

    /// Supported capabilities.
    fn capabilities(&self) -> &[SandboxCapability];

    /// Create a new sandbox.
    async fn create(&self, config: CreateConfig) -> Result<Box<dyn Sandbox>, SandboxError>;

    /// Get an existing sandbox by ID.
    async fn get(&self, id: &str) -> Result<Box<dyn Sandbox>, SandboxError>;

    /// List all sandboxes.
    async fn list(&self) -> Result<Vec<SandboxInfo>, SandboxError>;

    /// Destroy a sandbox (idempotent).
    async fn destroy(&self, id: &str) -> Result<(), SandboxError>;
}
```

### 4.3 SandboxAdapter — 后端实现者的接口

```rust
/// 后端作者实现此 trait。SDK 包装为 SandboxProvider。
#[async_trait]
pub trait SandboxAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Vec<SandboxCapability>;

    async fn create_sandbox(&self, config: &CreateConfig) -> Result<Box<dyn Sandbox>, SandboxError>;
    async fn get_sandbox(&self, id: &str) -> Result<Box<dyn Sandbox>, SandboxError>;
    async fn list_sandboxes(&self) -> Result<Vec<SandboxInfo>, SandboxError>;
    async fn destroy_sandbox(&self, id: &str) -> Result<(), SandboxError>;
}
```

`create_provider(adapter)` 函数包装 adapter 为 provider，自动处理：
- env 注入（按 EnvPolicy）
- skill 文件写入
- 事件观测
- 文件操作 fallback（如果 adapter 的 Sandbox 没实现 write_file，用 exec base64 分块补）

## 5. Types

### 5.1 CreateConfig

```rust
pub struct CreateConfig {
    /// Container image (e.g., "node:22", "ubuntu:24.04").
    /// None for local sandbox.
    pub image: Option<String>,

    /// Environment variables to inject.
    pub env: HashMap<String, String>,

    /// Environment variable isolation policy.
    pub env_policy: EnvPolicy,

    /// Resource limits.
    pub resources: Option<ResourceConfig>,

    /// Working directory inside the sandbox.
    pub working_directory: Option<String>,

    /// Auto-destroy timeout (minutes). 0 = no auto-destroy.
    pub auto_destroy_minutes: u32,

    /// Skills to inject (written as files into sandbox).
    pub skills: Vec<SkillDefinition>,

    /// Creation timeout (seconds).
    pub timeout_secs: u32,
}

impl Default for CreateConfig {
    fn default() -> Self {
        Self {
            image: None,
            env: HashMap::new(),
            env_policy: EnvPolicy::Inherit,
            resources: None,
            working_directory: None,
            auto_destroy_minutes: 0,
            skills: vec![],
            timeout_secs: 60,
        }
    }
}
```

### 5.2 EnvPolicy — 环境变量隔离策略

```rust
/// How to handle environment variables in the sandbox.
pub enum EnvPolicy {
    /// Inherit host environment + merge CreateConfig.env (overlay).
    /// Default for local sandbox.
    Inherit,

    /// Start clean, only use CreateConfig.env.
    /// Default for Docker/E2B.
    Clean,

    /// Inherit only whitelisted host vars + merge CreateConfig.env.
    Whitelist(Vec<String>),
}
```

**解决 Sandbank 的坑**：Sandbank 没有隔离策略，本地模式会泄漏宿主全量 env。我们明确三种模式。

### 5.3 ExecOptions / ExecResult

```rust
pub struct ExecOptions {
    /// Working directory for the command.
    pub cwd: Option<String>,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Additional env vars for this specific command (overlay on sandbox env).
    pub env: HashMap<String, String>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            cwd: None,
            timeout_ms: 120_000,
            env: HashMap::new(),
        }
    }
}

pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

### 5.4 SandboxState

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum SandboxState {
    Creating,
    Running,
    Stopped,
    Error(String),
    Terminated,
}
```

### 5.5 SandboxCapability — 可选能力

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SandboxCapability {
    /// Stream stdout/stderr in real-time.
    ExecStream,
    /// Interactive terminal (ttyd / PTY).
    Terminal,
    /// Hibernate and wake.
    Sleep,
    /// Persistent volumes.
    Volumes,
    /// Snapshot and restore.
    Snapshot,
    /// Expose ports to the internet.
    PortExpose,
    /// Archive upload/download (tar.gz).
    Archive,
}
```

**能力扩展接口**（Rust trait 向下转型）：

```rust
pub trait StreamableSandbox: Sandbox {
    fn exec_stream(
        &self,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Vec<u8>, SandboxError>> + Send>>, SandboxError>;
}

pub trait SleepableSandbox: Sandbox {
    async fn sleep(&self) -> Result<(), SandboxError>;
    async fn wake(&self) -> Result<(), SandboxError>;
}

pub trait SnapshotSandbox: Sandbox {
    async fn create_snapshot(&self, name: Option<&str>) -> Result<String, SandboxError>;
    async fn restore_snapshot(&self, snapshot_id: &str) -> Result<(), SandboxError>;
}
```

**使用方式**（类型安全，不用鸭子类型）：

```rust
// Sandbox trait 提供 as_any()
fn as_any(&self) -> &dyn Any;

// 消费者
if let Some(streamable) = sandbox.as_any().downcast_ref::<dyn StreamableSandbox>() {
    let stream = streamable.exec_stream("npm install", &opts)?;
}
// 或者更简单：provider.capabilities() 先检查
if provider.capabilities().contains(&SandboxCapability::ExecStream) {
    // safe to use
}
```

## 6. Error Types

```rust
#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("Sandbox '{0}' not found")]
    NotFound(String),

    #[error("Sandbox '{id}' is {current:?}, expected {expected:?}")]
    InvalidState {
        id: String,
        current: SandboxState,
        expected: SandboxState,
    },

    #[error("Command timed out after {timeout_ms}ms")]
    ExecTimeout { sandbox_id: String, timeout_ms: u64 },

    #[error("Capability '{0:?}' not supported by provider '{1}'")]
    CapabilityNotSupported(SandboxCapability, String),

    #[error("Provider error: {0}")]
    Provider(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Creation timed out after {timeout_secs}s")]
    CreationTimeout { timeout_secs: u32 },

    /// Partial cleanup failure — contains list of sandbox IDs that failed to destroy.
    #[error("Cleanup failed for {0} sandboxes")]
    PartialCleanup(Vec<(String, Box<dyn std::error::Error + Send + Sync>)>),
}
```

**对比 Sandbank**：增加 `PartialCleanup`，不吃 close() 的错误。

## 7. Tool Layer Integration

### 7.1 ToolContext 扩展

```rust
/// 扩展 ToolContext，提供 sandbox 访问。
pub trait SandboxToolContext: ToolContext {
    /// Get the sandbox instance for this session.
    /// Returns None if running in direct/local mode (backward compat).
    fn sandbox(&self) -> Option<&dyn Sandbox>;
}
```

### 7.2 工具适配策略

工具内部 dual-path：有 sandbox 走 sandbox，没有走本地（向后兼容）。

```rust
// ExecuteShellTool.execute() 伪代码
async fn execute(&self, input: Value, cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
    let params: Input = serde_json::from_value(input)?;

    if let Some(sandbox) = ctx.as_any().downcast_ref::<dyn SandboxToolContext>()
        .and_then(|c| c.sandbox())
    {
        // Sandbox path
        let result = sandbox.exec(&params.command, &ExecOptions {
            cwd: params.cwd,
            timeout_ms: params.timeout_secs.unwrap_or(30) * 1000,
            ..Default::default()
        }).await?;
        Ok(ToolResult { content: result.stdout, is_error: result.exit_code != 0, details: None })
    } else {
        // Local path (current behavior)
        let output = tokio::process::Command::new("sh")
            .arg("-c").arg(&params.command)
            .output().await?;
        Ok(ToolResult { content: String::from_utf8_lossy(&output.stdout).into(), .. })
    }
}
```

## 8. Provider 实现参考

### 8.1 Local（macOS / Linux）

```
exec()       → tokio::process::Command + SandboxConfig(Seatbelt)
write_file() → tokio::fs::write
read_file()  → tokio::fs::read
list_dir()   → tokio::fs::read_dir
env_policy   → Inherit (default) or Whitelist
lifecycle    → 无容器，sandbox ID = session ID
```

当前 `alva-agent-security::SandboxConfig` 直接迁入，wrap_command 生成 Seatbelt profile。

### 8.2 Docker

```
exec()       → docker exec <container_id> sh -c "command"
write_file() → docker cp 或 docker exec base64
read_file()  → docker exec cat / base64
list_dir()   → docker exec ls -la
env_policy   → Clean (default, 容器天然隔离)
lifecycle    → docker create → docker start → docker stop → docker rm
```

### 8.3 E2B

```
exec()       → E2B REST API: POST /sandboxes/{id}/exec
write_file() → E2B REST API: POST /sandboxes/{id}/files
read_file()  → E2B REST API: GET /sandboxes/{id}/files/{path}
list_dir()   → E2B REST API: GET /sandboxes/{id}/files?path=dir
env_policy   → Clean (cloud sandbox 天然隔离)
lifecycle    → E2B REST API: create / destroy
```

## 9. Crate 结构

```
crates/
├── alva-sandbox/              ← core (traits + types + errors + create_provider)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── sandbox.rs         — Sandbox trait
│   │   ├── provider.rs        — SandboxProvider + SandboxAdapter + create_provider()
│   │   ├── config.rs          — CreateConfig, EnvPolicy, ExecOptions, ResourceConfig
│   │   ├── capability.rs      — SandboxCapability + extension traits
│   │   ├── error.rs           — SandboxError
│   │   └── types.rs           — SandboxState, SandboxInfo, ExecResult, DirEntry
│   └── Cargo.toml             — deps: async-trait, thiserror, futures-core
│
├── alva-sandbox-local/        ← 本地执行 adapter
│   └── src/lib.rs             — LocalAdapter impl SandboxAdapter
│
├── alva-sandbox-docker/       ← Docker adapter (future)
│   └── src/lib.rs             — DockerAdapter impl SandboxAdapter
│
└── alva-sandbox-e2b/          ← E2B adapter (future)
    └── src/lib.rs             — E2BAdapter impl SandboxAdapter
```

**依赖方向**：
```
alva-sandbox (zero workspace deps, only async-trait + thiserror)
    ↑
alva-sandbox-local (depends on: alva-sandbox)
alva-sandbox-docker (depends on: alva-sandbox)
alva-sandbox-e2b (depends on: alva-sandbox)
    ↑
alva-agent-tools (depends on: alva-sandbox, optional via feature gate)
alva-app-core (depends on: alva-sandbox + chosen adapter)
```

## 10. Secrets 管理（架构边界说明）

**不在 sandbox crate 范围内。**

Secrets 通过独立 CLI 工具管理：
- 工具以 npm 包 / 纯 JS 文件 / WASM 形式部署到各种沙箱
- 通过网络 API 向密码管理服务请求临时授权
- 获取的 secret 通过 `CreateConfig.env` 或 `ExecOptions.env` 注入
- 每次访问有 audit log（谁、什么时候、拿了什么）
- 授权有时间限制（TTL），沙箱销毁后自动失效

Sandbox 层只负责传递 env，不负责 secret 的获取和管理。

## 11. Migration Path

### Phase 1: alva-sandbox core + local adapter
- 定义 traits 和 types
- 实现 LocalAdapter（迁入现有 Seatbelt + 直接执行逻辑）
- 工具层加 dual-path（sandbox || local）
- 所有现有行为不变（backward compat）

### Phase 2: Docker adapter
- 实现 DockerAdapter
- 集成测试：在 Docker 中跑工具

### Phase 3: E2B / 云沙箱
- 实现 E2BAdapter
- 网络 API 对接

### Phase 4: Session + 多 Agent
- 参考 Sandbank 的 Session 设计
- Relay 通信层
- 共享 Context Store

## 12. Design Decisions

| Decision | Rationale |
|----------|-----------|
| Sandbox trait 不含 archive 操作 | write_file + exec tar 组合即可，不作为最小接口 |
| refresh_state() 是显式方法 | 避免隐式网络请求，调用者控制何时刷新 |
| EnvPolicy 三种模式 | 本地需要 Inherit，容器需要 Clean，Whitelist 覆盖中间场景 |
| create_provider() 包装层 | 自动补 file fallback、注入 env/skills、事件观测 |
| PartialCleanup 错误 | close/destroy 失败不能静默吃掉，调用者需要知道 |
| Keystore 不在 sandbox 内 | 独立工具、独立部署、独立生命周期 |
| 工具层 dual-path | 向后兼容，sandbox 为 None 时走本地路径 |
| alva-sandbox core 零 workspace 依赖 | 可独立发布，不耦合 agent 体系 |

---

## 13. Observer / 事件系统

Sandbox 操作需要可观测性——日志、指标、审计。通过 Observer trait 实现，不侵入 Sandbox 接口。

### 13.1 SandboxObserver trait

```rust
/// Observer 接收所有 sandbox 操作的事件通知。
pub trait SandboxObserver: Send + Sync {
    fn on_event(&self, event: SandboxEvent);
}

pub struct SandboxEvent {
    pub sandbox_id: String,
    pub timestamp_ms: u64,
    pub event_type: SandboxEventType,
}

pub enum SandboxEventType {
    /// 沙箱已创建
    Created { image: Option<String> },
    /// 命令执行
    Exec { command: String, exit_code: i32, duration_ms: u64 },
    /// 文件写入
    FileWrite { path: String, size: usize },
    /// 文件读取
    FileRead { path: String, size: usize },
    /// 状态变更
    StateChanged { from: SandboxState, to: SandboxState },
    /// 沙箱已销毁
    Destroyed,
    /// 操作失败
    Error { operation: String, message: String },
}
```

### 13.2 注入方式

Observer 在 `create_provider()` 时注入，ProviderWrapper 自动在每个操作前后发射事件。

```rust
pub fn create_provider(
    adapter: Box<dyn SandboxAdapter>,
    observer: Option<Arc<dyn SandboxObserver>>,
) -> Box<dyn SandboxProvider> {
    Box::new(ProviderWrapper { adapter, observer })
}
```

**消费者不需要关心**——Sandbox trait 上没有 observer 方法，完全透明。

### 13.3 使用场景

| 场景 | Observer 实现 |
|------|-------------|
| 调试日志 | `TracingObserver` — 转发到 tracing::info! |
| 操作审计 | `AuditObserver` — 写入 SQLite 审计表 |
| 资源计量 | `MetricsObserver` — 累计 exec 次数、文件操作量、总耗时 |
| UI 展示 | `EventChannelObserver` — 通过 mpsc 推送到 GUI |

---

## 14. 文件操作 Fallback（分块传输）

Sandbank 的 base64 exec fallback 有大文件问题（shell ARG_MAX ~2MB）。我们的 fallback 必须分块。

### 14.1 策略

```
write_file() 调用
    ↓
Adapter 提供了原生 write_file?
    ├─ Yes → 直接调用（Docker cp / E2B API / 本地 fs）
    └─ No  → exec-based chunked fallback
              1. 在沙箱内创建临时文件
              2. 分 512KB 块 base64 编码
              3. 逐块 exec("printf ... | base64 -d >> tmpfile")
              4. mv tmpfile → target path
              5. 失败时清理临时文件
```

### 14.2 实现

```rust
/// Fallback file write via exec, with chunking for large files.
const CHUNK_SIZE: usize = 512 * 1024; // 512KB per chunk → ~680KB base64 → safe for ARG_MAX

async fn write_file_via_exec(
    sandbox: &dyn Sandbox,
    path: &str,
    content: &[u8],
) -> Result<(), SandboxError> {
    use base64::Engine;

    let tmp = format!("/tmp/_sb_write_{}", uuid::Uuid::new_v4());

    // Ensure parent directory
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
    sandbox.exec(&format!("mkdir -p '{dir}'"), &ExecOptions::default()).await?;

    // Chunked write
    for (i, chunk) in content.chunks(CHUNK_SIZE).enumerate() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(chunk);
        let op = if i == 0 { ">" } else { ">>" };
        let result = sandbox.exec(
            &format!("printf '%s' '{b64}' | base64 -d {op} '{tmp}'"),
            &ExecOptions::default(),
        ).await?;
        if !result.success() {
            // Cleanup on failure
            let _ = sandbox.exec(&format!("rm -f '{tmp}'"), &ExecOptions::default()).await;
            return Err(SandboxError::Provider(
                format!("write_file chunk {i} failed: {}", result.stderr).into()
            ));
        }
    }

    // Atomic move
    sandbox.exec(&format!("mv '{tmp}' '{path}'"), &ExecOptions::default()).await?;
    Ok(())
}
```

**对比 Sandbank**：Sandbank 不分块，直接把整个 base64 塞进一条命令。我们 512KB 分块，安全上限 ~680KB base64 < 2MB ARG_MAX。

---

## 15. 生命周期管理

### 15.1 Session-bound sandbox

Sandbox 可以绑定到 Agent session。Session 结束时自动销毁。

```rust
/// Sandbox 归属关系
pub enum SandboxOwnership {
    /// 独立生命周期，需要手动 destroy
    Standalone,
    /// 绑定到 session，session 结束时自动销毁
    SessionBound { session_id: String },
    /// 自动销毁（超时后自动清理）
    AutoDestroy { timeout: Duration },
}
```

### 15.2 GC 策略

Provider 层维护一个后台任务，定期清理：

```rust
/// 垃圾回收配置
pub struct GcConfig {
    /// 扫描间隔
    pub scan_interval: Duration,
    /// 最大空闲时间（无 exec 调用）
    pub max_idle: Duration,
    /// 最大存活时间（无论是否活跃）
    pub max_lifetime: Duration,
}
```

**本地 adapter**：无需 GC（无容器残留）。
**Docker adapter**：扫描 `docker ps` 匹配 label，清理超时容器。
**E2B adapter**：调用 E2B API 列出并销毁超时沙箱。

### 15.3 优雅关闭

```rust
impl SandboxProvider {
    /// 销毁所有沙箱，返回失败清单。
    async fn destroy_all(&self) -> Vec<CleanupFailure> {
        let sandboxes = self.list().await.unwrap_or_default();
        let mut failures = Vec::new();

        for info in sandboxes {
            if let Err(e) = self.destroy(&info.id).await {
                failures.push(CleanupFailure {
                    sandbox_id: info.id,
                    error: Box::new(e),
                });
            }
        }

        failures
    }
}
```

---

## 16. 网络策略

### 16.1 NetworkPolicy enum

```rust
pub enum NetworkPolicy {
    /// 允许所有出站连接
    Open,
    /// 阻止所有出站连接
    Closed,
    /// 只允许白名单域名/IP
    AllowList(Vec<String>),
    /// 通过代理转发（审计 + 控制）
    Proxied { proxy_url: String },
}
```

### 16.2 各后端实现

| 后端 | Open | Closed | AllowList | Proxied |
|------|:---:|:---:|:---:|:---:|
| Local (Seatbelt) | ✅ | ✅ `(deny network*)` | ❌ 不支持 | ❌ |
| Docker | ✅ | ✅ `--network none` | ✅ iptables | ✅ `--env HTTP_PROXY` |
| E2B | ✅ | ❌ API 不支持 | ❌ | ❌ |

不支持的策略抛 `CapabilityNotSupported`。

### 16.3 在 CreateConfig 中声明

```rust
pub struct CreateConfig {
    // ... existing fields ...

    /// Network access policy.
    pub network: NetworkPolicy,
}
```

---

## 17. 与 EngineRuntime 的集成

### 17.1 流程

```
APP 创建 session
    ↓
APP 创建 sandbox (via SandboxProvider)
    ↓
APP 构造 ToolContext (包含 sandbox 引用)
    ↓
APP 构造 AlvaAdapterConfig (包含 tools + tool_context)
    ↓
APP 调用 engine.execute(request)
    ↓
Engine 内部: Agent loop → Tool.execute(ctx)
    ↓
Tool 内部: ctx.sandbox().exec("npm install")
    ↓
Sandbox 执行命令（本地 / Docker / E2B）
```

### 17.2 ToolContext 实现

```rust
/// 带 Sandbox 的 ToolContext 实现
pub struct SandboxedToolContext {
    session_id: String,
    sandbox: Arc<dyn Sandbox>,
    config: HashMap<String, String>,
}

impl ToolContext for SandboxedToolContext {
    fn session_id(&self) -> &str { &self.session_id }
    fn get_config(&self, key: &str) -> Option<String> { self.config.get(key).cloned() }
    fn as_any(&self) -> &dyn Any { self }
    fn local(&self) -> Option<&dyn LocalToolContext> { None }
}

impl SandboxToolContext for SandboxedToolContext {
    fn sandbox(&self) -> Option<&dyn Sandbox> { Some(self.sandbox.as_ref()) }
}
```

### 17.3 无 Sandbox 时的兼容

APP 不设置 sandbox 时，工具走 `ctx.local()` → 本地执行。这是当前的默认行为，无需任何改动。

```
ctx.sandbox() == None  → 本地路径（现有行为）
ctx.sandbox() == Some  → Sandbox 路径（新能力）
```

---

## 18. Session 管理（多 Agent 编排）

参考 Sandbank 的 Session 设计，但简化为 Phase 4 的目标。

### 18.1 Session trait

```rust
pub struct Session {
    id: String,
    provider: Arc<dyn SandboxProvider>,
    sandboxes: HashMap<String, Box<dyn Sandbox>>,
    context: ContextStore,
}

impl Session {
    /// 创建一个命名沙箱
    pub async fn spawn(&mut self, name: &str, config: CreateConfig) -> Result<&dyn Sandbox, SandboxError>;

    /// 获取已有沙箱
    pub fn get(&self, name: &str) -> Option<&dyn Sandbox>;

    /// 列出所有沙箱
    pub fn list(&self) -> Vec<&str>;

    /// 关闭 session，销毁所有沙箱
    pub async fn close(&mut self) -> Vec<CleanupFailure>;
}
```

### 18.2 ContextStore（共享上下文）

```rust
/// Agent 间共享的键值存储
pub struct ContextStore {
    store: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl ContextStore {
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T>;
    pub async fn set<T: Serialize>(&self, key: &str, value: &T);
    pub async fn delete(&self, key: &str);
    pub async fn keys(&self) -> Vec<String>;
}
```

### 18.3 多 Agent 通信

Phase 4 考虑。两种方案：

| 方案 | 实现 | 适用场景 |
|------|------|---------|
| **ContextStore 轮询** | Agent 通过 context store 读写共享数据 | 低频协作 |
| **Relay WebSocket** | 类 Sandbank 的 relay 服务端 | 实时消息传递 |

初期用 ContextStore 轮询（简单），后期按需加 Relay。

---

## 19. 各 Adapter 详细设计

### 19.1 alva-sandbox-local

**已实现**（见 alva-sandbox 仓库）。

核心逻辑：
- exec → `tokio::process::Command::new("sh").arg("-c").arg(cmd)`
- write_file → `tokio::fs::write`
- read_file → `tokio::fs::read`
- env 注入 → `cmd.env(k, v)`

待加入：
- Seatbelt 集成：将 `alva-agent-security::SandboxConfig::wrap_command()` 迁入
- NetworkPolicy 支持：Seatbelt profile 的 `(deny network*)` 规则

### 19.2 alva-sandbox-docker

```
crates/alva-sandbox-docker/
├── Cargo.toml          — deps: alva-sandbox, bollard (Docker API)
└── src/
    ├── lib.rs          — DockerAdapter + DockerSandbox
    └── config.rs       — DockerAdapterConfig
```

**依赖**：[bollard](https://crates.io/crates/bollard)（Rust Docker Engine API client）

**关键实现**：

```rust
pub struct DockerAdapterConfig {
    /// Docker daemon URL (default: unix socket)
    pub docker_url: Option<String>,
    /// 默认镜像
    pub default_image: String,
    /// 容器 label 前缀（用于 GC 识别）
    pub label_prefix: String,
    /// 网络模式
    pub default_network: String,
}

struct DockerSandbox {
    id: String,              // container ID
    container_id: String,    // Docker container ID
    state: SandboxState,
    docker: bollard::Docker,
    working_directory: String,
}
```

**操作映射**：

| Sandbox 方法 | Docker 实现 |
|-------------|-------------|
| `exec(cmd)` | `docker.create_exec(container_id, cmd)` → `docker.start_exec(exec_id)` |
| `write_file(path, content)` | 原生：`docker.upload_to_container(container_id, tar_archive)` |
| `read_file(path)` | 原生：`docker.download_from_container(container_id, path)` → 解 tar |
| `list_dir(path)` | `exec("ls -la --time-style=+%s path")` 解析输出 |
| `exists(path)` | `exec("test -e path && echo 1 || echo 0")` |
| `refresh_state()` | `docker.inspect_container(container_id)` → 映射 status |
| `create()` | `docker.create_container(config)` → `docker.start_container(id)` |
| `destroy()` | `docker.stop_container(id)` → `docker.remove_container(id)` |

**环境变量**：Docker `create_container` 的 `Env` 字段直接注入，天然 Clean 隔离。

**网络策略**：
- Open → 默认 bridge 网络
- Closed → `--network none`
- AllowList → 创建自定义 network + iptables 规则

### 19.3 alva-sandbox-e2b

```
crates/alva-sandbox-e2b/
├── Cargo.toml          — deps: alva-sandbox, reqwest, serde_json
└── src/
    ├── lib.rs          — E2BAdapter + E2BSandbox
    ├── client.rs       — E2B REST API 客户端
    └── config.rs       — E2BAdapterConfig
```

**依赖**：[reqwest](https://crates.io/crates/reqwest)（HTTP client）

**E2B API 基础 URL**：`https://api.e2b.dev/v1`

**操作映射**：

| Sandbox 方法 | E2B API |
|-------------|---------|
| `create()` | `POST /sandboxes { template, timeout, envs, metadata }` |
| `exec(cmd)` | `POST /sandboxes/{id}/commands { command, workdir, timeout }` |
| `write_file(path, content)` | `PUT /sandboxes/{id}/files/{path}` body = content |
| `read_file(path)` | `GET /sandboxes/{id}/files/{path}` |
| `list_dir(path)` | `GET /sandboxes/{id}/files?path={dir}` |
| `destroy()` | `DELETE /sandboxes/{id}` |
| `refresh_state()` | `GET /sandboxes/{id}` → status 字段 |

**E2B 特有能力**：
- 持久化沙箱（`keep_alive` 参数）
- 模板（预构建镜像）
- 文件系统原生支持（不需要 exec fallback）

### 19.4 iOS Sandbox（概念）

iOS 场景特殊——不能 spawn 子进程、不能 Docker。

**可行方案**：
- exec → 内嵌 JavaScript 引擎（如 JavaScriptCore / QuickJS）执行命令的模拟
- write_file / read_file → App sandbox 内的 Documents 目录
- 网络 → 通过 URLSession，受 App Transport Security 约束

**或者**：iOS 端不跑本地沙箱，直接用 E2B 云沙箱，命令在云端执行。

### 19.5 WASM Sandbox（概念）

**场景**：浏览器内或轻量嵌入式环境。

**可行方案**：
- exec → WASI 模拟的命令执行（限制很大）
- write_file / read_file → 内存虚拟文件系统（如 memfs）
- 网络 → fetch API

**更实际的方案**：WASM 端作为 thin client，通过 WebSocket 连接到远端沙箱（E2B / Docker），命令在远端执行。

---

## 20. 测试策略

### 20.1 Conformance 测试套件

每个 adapter 必须通过同一套一致性测试，验证 Sandbox trait 的行为契约。

```rust
/// 一致性测试——所有 adapter 必须通过
pub async fn conformance_suite(provider: &dyn SandboxProvider) {
    test_create_and_destroy(provider).await;
    test_exec_echo(provider).await;
    test_exec_exit_code(provider).await;
    test_exec_timeout(provider).await;
    test_exec_env_injection(provider).await;
    test_exec_cwd(provider).await;
    test_write_and_read_file(provider).await;
    test_write_large_file(provider).await;  // 测试分块 fallback
    test_list_dir(provider).await;
    test_exists(provider).await;
    test_destroy_idempotent(provider).await;
    test_state_transitions(provider).await;
}
```

### 20.2 测试分层

| 层 | 测试类型 | 工具 |
|---|---------|------|
| alva-sandbox core | 单元测试（types、config、error） | `cargo test` |
| alva-sandbox-local | 集成测试（依赖本地 OS） | `cargo test` |
| alva-sandbox-docker | 集成测试（依赖 Docker daemon） | `cargo test`，CI 需 Docker |
| alva-sandbox-e2b | 集成测试（依赖 E2B API） | `E2B_API_KEY=... cargo test` |
| Conformance | 跨 adapter 一致性 | 参数化测试，每个 adapter 跑同一套 |

### 20.3 Mock adapter

core crate 提供 `MockSandbox` 用于上层测试，不需要真实后端。

```rust
/// 测试用 mock sandbox
pub struct MockSandbox {
    pub id: String,
    pub exec_responses: Vec<ExecResult>,
    pub files: HashMap<String, Vec<u8>>,
}

impl MockSandbox {
    pub fn new() -> Self { ... }
    pub fn with_exec_response(mut self, result: ExecResult) -> Self { ... }
    pub fn with_file(mut self, path: &str, content: &[u8]) -> Self { ... }
}
```

---

## 21. 实现优先级（更新）

| Phase | 内容 | 依赖 | 预计 |
|:---:|------|------|------|
| **1a** | ✅ alva-sandbox core traits + types | 无 | 已完成 |
| **1b** | ✅ alva-sandbox-local adapter | 1a | 已完成 |
| **1c** | 工具层 dual-path 改造 | 1b | 改 alva-agent-tools 的 9 个工具 |
| **1d** | Seatbelt 集成到 local adapter | 1c | 迁入 alva-agent-security 的 wrap_command |
| **1e** | Observer 事件系统 | 1a | core crate 加 observer trait |
| **1f** | Conformance 测试套件 + MockSandbox | 1a | core crate 加测试模块 |
| **2a** | Docker adapter | 1f | 新 crate，依赖 bollard |
| **2b** | Docker conformance 通过 | 2a | 跑一致性测试 |
| **3a** | E2B adapter | 1f | 新 crate，依赖 reqwest |
| **3b** | E2B conformance 通过 | 3a | 需要 E2B API key |
| **4a** | Session + ContextStore | 1a | core crate 扩展 |
| **4b** | 多 Agent 通信 (Relay) | 4a | 新 crate 或集成到 core |

---

## 22. Open Questions

| # | 问题 | 候选方案 | 决策状态 |
|---|------|---------|---------|
| 1 | Sandbox trait 要不要 `remove_file()` / `move_file()`？ | A: 不加，用 exec rm/mv；B: 加，完整文件 API | 倾向 A（最小接口） |
| 2 | Docker adapter 用 bollard 还是 shell out docker CLI？ | A: bollard（类型安全）；B: CLI（简单但脆弱） | 倾向 A |
| 3 | 大文件传输用分块 exec 还是强制要求原生 API？ | A: 分块 fallback（通用）；B: 强制原生（快但限制 adapter） | 倾向 A |
| 4 | iOS 端走本地还是云沙箱？ | A: 本地 JSCore 模拟；B: 云端 E2B | 倾向 B（iOS 限制太多） |
| 5 | NetworkPolicy 放 CreateConfig 还是单独的 configure 方法？ | A: CreateConfig（创建时确定）；B: 运行时可变 | 倾向 A（安全不可变） |
| 6 | Session 的 ContextStore 要不要持久化？ | A: 纯内存；B: 可选 SQLite 持久化 | 先 A，按需加 B |
