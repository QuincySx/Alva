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
