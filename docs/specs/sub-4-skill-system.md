# Sub-4: Skill 系统 + MCP 集成技术规格

> Crate: `srow-skills` | 依赖: Sub-2 (`srow-engine`) | 被依赖: Sub-5, Sub-6

---

## 1. 目标与范围

Sub-4 实现 Srow Agent 的能力层，1:1 复刻 Wukong（com.dingtalk.real）的 Skill 系统和 MCP 集成，核心差异是：**Skill 和 MCP 按 Agent 定义加载，不是全局的。**

**包含：**
- Skill 包规范解析（SKILL.md YAML frontmatter + Markdown body）
- 三级渐进加载机制（元数据 → 指令层 → 资源层）
- Skill 类型支持：Bundled Skills + MBB Skills（域名路由）+ 用户自装
- Skill 管理：安装、启用/禁用、删除
- Skill 注入策略：explicit / auto / strict
- MCP Server 生命周期管理（启动/停止/连接/断开）
- MCP 工具调用（list_servers / list_tools / call_tool）
- 按 Agent 模板定义 Skill 集和 MCP Server 集
- AgentTemplate 配置模型（全局基线 + Agent 级叠加）
- 与 Sub-2 AgentEngine 的集成接口（system prompt 注入 + Tool 注册）

**不包含：**
- 浏览器自动化本身（Sub-6，但 MBB Skill 的域名路由接口在此定义）
- 沙箱隔离（Sub-7，`execute_shell` 在此直接调用，沙箱由 Sub-7 加固）
- Agent 编排层（Sub-5）

---

## 2. 模块架构

参考 Sub-2 的 DDD 分层，`srow-skills` 作为单独 crate，通过 Rust 模块边界组织。

```
srow-skills/
├── Cargo.toml
└── src/
    ├── lib.rs                              # pub use 统一导出
    │
    ├── domain/                             # 领域模型：纯类型，零 I/O 依赖
    │   ├── mod.rs
    │   ├── skill.rs                        # SkillMeta, SkillBody, SkillResource
    │   ├── skill_config.rs                 # SkillInjectionPolicy, SkillRef
    │   ├── mcp.rs                          # McpServerConfig, McpServerState
    │   └── agent_template.rs              # AgentTemplate, SkillSet, McpSet
    │
    ├── application/                        # 应用服务：业务逻辑
    │   ├── mod.rs
    │   ├── skill_loader.rs                 # 三级渐进加载
    │   ├── skill_store.rs                  # Skill 安装/查询/删除
    │   ├── skill_injector.rs               # system prompt 注入
    │   ├── mcp_manager.rs                  # MCP Server 生命周期
    │   └── agent_template_service.rs       # AgentTemplate 解析与实例化
    │
    ├── ports/                              # 对外抽象 trait
    │   ├── mod.rs
    │   ├── skill_repository.rs             # SkillRepository trait
    │   └── mcp_transport.rs                # McpTransport trait
    │
    ├── adapters/                           # 具体实现
    │   ├── mod.rs
    │   ├── skill_fs.rs                     # 文件系统 SkillRepository
    │   ├── mcp_stdio.rs                    # stdio 传输层（rmcp）
    │   ├── mcp_sse.rs                      # SSE 传输层（rmcp）
    │   └── mcp_tool_adapter.rs             # MCP Tool → srow_engine::Tool
    │
    └── error.rs                            # SkillError 统一错误类型
```

### 分层依赖规则

```
domain ← ports ← application ← adapters
```

`srow-skills` 对 `srow-engine` 的依赖方向：
- `adapters::mcp_tool_adapter` 实现 `srow_engine::ports::tool::Tool`
- `application::skill_injector` 输出 `String`（system prompt 片段），由上层注入 `AgentConfig::system_prompt`

---

## 3. 领域模型（Domain Types）

### 3.1 Skill 模型

```rust
// src/domain/skill.rs

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;

/// Skill 元数据（Level 1 — 始终驻留上下文）
/// 对应 SKILL.md YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    /// kebab-case，[a-z0-9-]，最长 64 字符
    pub name: String,
    /// 触发描述，最长 1024 字符，禁止尖括号
    /// 作为 Agent 判断是否激活此 Skill 的唯一依据
    pub description: String,
    pub license: Option<String>,
    /// Skill 允许使用的工具白名单（None = 不限制）
    pub allowed_tools: Option<Vec<String>>,
    /// 扩展元数据（版本、作者、兼容性等）
    pub metadata: Option<HashMap<String, serde_yaml::Value>>,
}

/// Skill 指令层（Level 2 — 触发后加载）
/// 对应 SKILL.md 的 Markdown body（frontmatter 之后的全部内容）
#[derive(Debug, Clone)]
pub struct SkillBody {
    /// SKILL.md body 的原始 Markdown 文本
    pub markdown: String,
    /// 估算 token 数（用于上下文管理）
    pub estimated_tokens: u32,
}

/// 单个资源文件（Level 3 — 按需加载）
#[derive(Debug, Clone)]
pub struct SkillResource {
    /// 相对于 skill 根目录的路径（如 "references/api.md"）
    pub relative_path: String,
    pub content: Vec<u8>,
    pub content_type: ResourceContentType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceContentType {
    Markdown,
    Python,
    JavaScript,
    TypeScript,
    Shell,
    Binary,
    Other(String),
}

/// Skill 的完整表示（内存中）
#[derive(Debug, Clone)]
pub struct Skill {
    pub meta: SkillMeta,
    /// Skill 类型
    pub kind: SkillKind,
    /// Skill 根目录路径（已解压）
    pub root_path: PathBuf,
    /// 是否已启用
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    /// 随 App 内置分发的 Skill（bundled-skills/）
    Bundled,
    /// 浏览器增强 Skill，绑定域名（mbb-skills/）
    Mbb {
        /// 绑定的域名列表，如 ["12306.cn"]
        domains: Vec<String>,
    },
    /// 用户自装 Skill（用户 Skill 目录）
    UserInstalled,
}
```

### 3.2 Skill 注入配置

```rust
// src/domain/skill_config.rs

use serde::{Deserialize, Serialize};

/// Skill 引用：在 AgentTemplate 中声明使用某个 Skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRef {
    /// 对应 SkillMeta::name
    pub name: String,
    /// 覆盖注入策略（None = 使用全局默认）
    pub injection: Option<InjectionPolicy>,
}

/// Skill 注入到 system prompt 的策略
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionPolicy {
    /// 显式注入：将 SKILL.md body 完整注入 system prompt
    /// 适用于核心技能，确保 Agent 始终感知该技能
    Explicit,
    /// 自动注入：仅注入 description（元数据层），
    /// Agent 通过 use_skill 工具按需拉取完整内容
    Auto,
    /// 严格注入：与 explicit 相同，但同时限制 Agent
    /// 只能使用该 Skill 的 allowed_tools
    Strict,
}

impl Default for InjectionPolicy {
    fn default() -> Self {
        InjectionPolicy::Auto
    }
}
```

### 3.3 MCP Server 配置

```rust
// src/domain/mcp.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP Server 的传输协议类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum McpTransportConfig {
    /// stdio 传输（子进程 stdin/stdout）
    Stdio {
        /// 可执行文件路径
        command: String,
        /// 命令行参数
        args: Vec<String>,
        /// 环境变量注入
        env: HashMap<String, String>,
    },
    /// SSE 传输（HTTP Server-Sent Events）
    Sse {
        /// SSE 端点 URL，如 "http://127.0.0.1:3000/sse"
        url: String,
        /// 额外 HTTP 请求头（如 Authorization）
        headers: HashMap<String, String>,
    },
}

/// 单个 MCP Server 的完整配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 唯一标识符，kebab-case，如 "builtin-browser"
    pub id: String,
    /// 人类可读名称，用于 UI 显示
    pub display_name: String,
    pub transport: McpTransportConfig,
    /// 是否在 Agent 启动时自动连接
    pub auto_connect: bool,
    /// 连接超时（秒）
    pub connect_timeout_secs: u32,
}

/// MCP Server 运行时状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    /// 未连接（初始状态）
    Disconnected,
    /// 正在连接
    Connecting,
    /// 已连接，tools 已列举完毕
    Connected {
        /// 此 Server 暴露的工具数量
        tool_count: usize,
    },
    /// 连接失败
    Failed { reason: String },
}

/// MCP Tool 描述（从服务端列举）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_id: String,
    pub tool_name: String,
    pub description: String,
    /// JSON Schema 格式的参数定义
    pub input_schema: serde_json::Value,
}
```

### 3.4 Agent 模板

```rust
// src/domain/agent_template.rs

use serde::{Deserialize, Serialize};
use crate::domain::skill_config::{SkillRef, InjectionPolicy};
use crate::domain::mcp::McpServerConfig;

/// Agent 模板：定义一类 Agent 的能力集和行为规范
/// 对应 Wukong 中的"Agent 模板库"概念
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    /// 模板 ID，kebab-case，如 "browser-agent"
    pub id: String,
    /// 人类可读名称
    pub name: String,
    /// 模板描述（决策 Agent 选人时参考）
    pub description: String,

    /// 基础 system prompt（Skill 注入内容会追加到此之后）
    pub system_prompt_base: String,

    /// 此模板使用的 Skill 集合
    pub skills: SkillSet,

    /// 此模板使用的 MCP Server 集合
    pub mcp_servers: McpSet,

    /// 允许使用的工具名列表（None = 使用全部注册工具）
    /// 注意：MCP 工具名格式为 "mcp:<server_id>:<tool_name>"
    pub allowed_tools: Option<Vec<String>>,

    /// 最大循环轮次（覆盖引擎默认值）
    pub max_iterations: Option<u32>,
}

/// Agent 模板的 Skill 声明集合
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillSet {
    /// 继承全局 Skill 配置（全局 skills.toml 中的 enabled skills）
    pub inherit_global: bool,
    /// 在全局基础上额外添加的 Skill
    pub include: Vec<SkillRef>,
    /// 从全局集合中排除的 Skill（按 name）
    pub exclude: Vec<String>,
    /// 默认注入策略（单个 SkillRef 可覆盖此值）
    pub default_injection: InjectionPolicy,
}

/// Agent 模板的 MCP Server 声明集合
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpSet {
    /// 继承全局 MCP Server 配置
    pub inherit_global: bool,
    /// 额外添加的 MCP Server（完整配置）
    pub include: Vec<McpServerConfig>,
    /// 从全局集合中排除的 Server（按 id）
    pub exclude: Vec<String>,
}

/// 全局 Skill 和 MCP 基线配置文件格式
/// 对应 ~/.srow/skills.toml 或 workspace/.srow/skills.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSkillConfig {
    /// 全局启用的 Skill（按 name）
    pub enabled_skills: Vec<SkillRef>,
    /// 全局 MCP Server 配置
    pub mcp_servers: Vec<McpServerConfig>,
}
```

---

## 4. 端口定义（Ports）

### 4.1 SkillRepository

```rust
// src/ports/skill_repository.rs

use crate::domain::skill::{Skill, SkillMeta, SkillBody, SkillResource};
use crate::error::SkillError;
use async_trait::async_trait;

/// Skill 仓储接口：屏蔽底层存储细节（文件系统 / 压缩包 / 网络）
#[async_trait]
pub trait SkillRepository: Send + Sync {
    /// 列出所有已知 Skill（仅元数据，Level 1）
    async fn list_skills(&self) -> Result<Vec<Skill>, SkillError>;

    /// 按 name 查找 Skill（仅元数据）
    async fn find_skill(&self, name: &str) -> Result<Option<Skill>, SkillError>;

    /// 加载 Skill 指令层（Level 2 — SKILL.md body）
    async fn load_body(&self, name: &str) -> Result<SkillBody, SkillError>;

    /// 加载 Skill 资源文件（Level 3 — 按需）
    async fn load_resource(
        &self,
        name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError>;

    /// 列出某 Skill 的所有资源路径（不加载内容）
    async fn list_resources(&self, name: &str) -> Result<Vec<String>, SkillError>;

    /// 安装 Skill（从本地路径或 .zip 文件）
    async fn install(&self, source: SkillInstallSource) -> Result<SkillMeta, SkillError>;

    /// 删除 Skill（仅 UserInstalled 类型可删除）
    async fn remove(&self, name: &str) -> Result<(), SkillError>;

    /// 设置 Skill 启用/禁用状态
    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError>;
}

/// Skill 安装来源
pub enum SkillInstallSource {
    /// 本地目录路径（包含 SKILL.md 的目录）
    LocalDir(std::path::PathBuf),
    /// 本地 .zip 文件路径
    LocalZip(std::path::PathBuf),
    /// 远程 URL（.zip 文件，HTTPS）
    RemoteUrl(String),
}
```

### 4.2 McpTransport

```rust
// src/ports/mcp_transport.rs

use crate::domain::mcp::McpToolInfo;
use crate::error::SkillError;
use async_trait::async_trait;
use serde_json::Value;

/// MCP 传输层抽象：屏蔽 stdio / SSE 差异
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// 建立连接并完成 MCP 握手（initialize）
    async fn connect(&mut self) -> Result<(), SkillError>;

    /// 断开连接
    async fn disconnect(&mut self) -> Result<(), SkillError>;

    /// 是否已连接
    fn is_connected(&self) -> bool;

    /// 列举此 Server 暴露的所有工具
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, SkillError>;

    /// 调用工具
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, SkillError>;
}
```

---

## 5. 核心应用服务

### 5.1 SkillLoader — 三级渐进加载

```rust
// src/application/skill_loader.rs

use std::sync::Arc;
use crate::{
    domain::{
        skill::{Skill, SkillBody, SkillResource},
        skill_config::InjectionPolicy,
    },
    ports::skill_repository::SkillRepository,
    error::SkillError,
};

/// 三级渐进式 Skill 加载器
///
/// Level 1（元数据）: 始终从内存 / 索引读取，不触及磁盘
/// Level 2（指令层）: 用户 prompt 触发 Skill 后按需读取 SKILL.md body
/// Level 3（资源层）: Agent 通过 use_skill(level="full") 或直接读取 references/ 文件
pub struct SkillLoader {
    repo: Arc<dyn SkillRepository>,
}

impl SkillLoader {
    pub fn new(repo: Arc<dyn SkillRepository>) -> Self {
        Self { repo }
    }

    /// 构建 Level 1 上下文片段（元数据摘要表）
    /// 格式参考 Wukong 的 skill 列表注入：name + description 的紧凑列表
    /// 此片段始终注入 system prompt，约 50-150 tokens
    pub async fn build_meta_summary(
        &self,
        skills: &[Skill],
    ) -> Result<String, SkillError> {
        if skills.is_empty() {
            return Ok(String::new());
        }

        let mut lines = vec![
            "## Available Skills".to_string(),
            String::new(),
            "The following skills are available. Use `use_skill` to load full instructions."
                .to_string(),
            String::new(),
        ];

        for skill in skills.iter().filter(|s| s.enabled) {
            lines.push(format!(
                "- **{}**: {}",
                skill.meta.name, skill.meta.description
            ));
        }

        Ok(lines.join("\n"))
    }

    /// 构建 Level 2 上下文片段（单个 Skill 的完整 SKILL.md body）
    /// 在用户 prompt 触发该 Skill 时调用
    pub async fn load_skill_body(&self, name: &str) -> Result<SkillBody, SkillError> {
        self.repo.load_body(name).await
    }

    /// 构建 Level 2 内联注入（Explicit/Strict 模式下预先注入）
    /// 直接在 system prompt 中展开 SKILL.md body
    pub async fn build_explicit_injection(
        &self,
        skill: &Skill,
    ) -> Result<String, SkillError> {
        let body = self.repo.load_body(&skill.meta.name).await?;
        Ok(format!(
            "## Skill: {}\n\n{}\n",
            skill.meta.name, body.markdown
        ))
    }

    /// 加载资源文件（Level 3）
    /// 由 use_skill(level="full") 工具或 Agent 直接的 read_file 调用触发
    pub async fn load_resource(
        &self,
        skill_name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError> {
        self.repo.load_resource(skill_name, relative_path).await
    }

    /// 列出 Skill 的所有资源路径（供 Agent 选择性加载）
    pub async fn list_resources(
        &self,
        skill_name: &str,
    ) -> Result<Vec<String>, SkillError> {
        self.repo.list_resources(skill_name).await
    }
}
```

### 5.2 SkillInjector — System Prompt 注入

```rust
// src/application/skill_injector.rs

use std::sync::Arc;
use crate::{
    domain::{
        skill::Skill,
        skill_config::{SkillRef, InjectionPolicy},
    },
    application::skill_loader::SkillLoader,
    error::SkillError,
};

/// 将 Skill 内容注入 Agent system prompt
///
/// 注入结果作为 system prompt 的一个段落追加，
/// AgentEngine 在构建 LLMRequest 时与 AgentConfig::system_prompt_base 拼接
pub struct SkillInjector {
    loader: SkillLoader,
}

impl SkillInjector {
    pub fn new(loader: SkillLoader) -> Self {
        Self { loader }
    }

    /// 为一组 SkillRef 构建完整的 system prompt 注入块
    ///
    /// 注入策略：
    /// - Auto:    仅注入 Level 1 元数据摘要表（描述 + 触发条件）
    /// - Explicit: 注入 Level 1 元数据 + 完整 SKILL.md body
    /// - Strict:  与 Explicit 相同，额外在 prompt 中声明工具限制
    pub async fn build_injection(
        &self,
        skill_refs: &[SkillRef],
        available_skills: &[Skill],
    ) -> Result<String, SkillError> {
        let mut auto_skills: Vec<&Skill> = vec![];
        let mut explicit_skills: Vec<(&Skill, &SkillRef)> = vec![];

        for skill_ref in skill_refs {
            let Some(skill) = available_skills.iter().find(|s| s.meta.name == skill_ref.name) else {
                continue; // 未找到则跳过，不报错（宽容策略）
            };
            if !skill.enabled {
                continue;
            }

            let policy = skill_ref
                .injection
                .as_ref()
                .unwrap_or(&InjectionPolicy::Auto);

            match policy {
                InjectionPolicy::Auto => auto_skills.push(skill),
                InjectionPolicy::Explicit | InjectionPolicy::Strict => {
                    explicit_skills.push((skill, skill_ref));
                }
            }
        }

        let mut parts: Vec<String> = vec![];

        // 1. Auto 模式：汇总成元数据摘要表
        if !auto_skills.is_empty() {
            let meta_summary = self.loader.build_meta_summary(&auto_skills).await?;
            if !meta_summary.is_empty() {
                parts.push(meta_summary);
            }
        }

        // 2. Explicit/Strict 模式：直接内联展开每个 Skill 的完整内容
        for (skill, skill_ref) in &explicit_skills {
            let injected = self.loader.build_explicit_injection(skill).await?;
            parts.push(injected);

            // Strict 模式：在 prompt 中声明工具约束
            let policy = skill_ref.injection.as_ref().unwrap_or(&InjectionPolicy::Auto);
            if *policy == InjectionPolicy::Strict {
                if let Some(allowed_tools) = &skill.meta.allowed_tools {
                    parts.push(format!(
                        "> [Skill: {}] Strict mode: only use tools: {}\n",
                        skill.meta.name,
                        allowed_tools.join(", ")
                    ));
                }
            }
        }

        Ok(parts.join("\n\n"))
    }
}
```

### 5.3 SkillStore — Skill 管理

```rust
// src/application/skill_store.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{
    domain::skill::{Skill, SkillMeta, SkillKind},
    ports::skill_repository::{SkillRepository, SkillInstallSource},
    error::SkillError,
};

/// Skill 仓库：索引缓存 + 统一访问入口
/// 在 App 启动时扫描所有 Skill 目录，维护内存索引
pub struct SkillStore {
    repo: Arc<dyn SkillRepository>,
    /// 内存索引（name → Skill），在首次 scan 后填充
    cache: Arc<RwLock<Vec<Skill>>>,
}

impl SkillStore {
    pub fn new(repo: Arc<dyn SkillRepository>) -> Self {
        Self {
            repo,
            cache: Arc::new(RwLock::new(vec![])),
        }
    }

    /// 扫描所有 Skill 目录，填充内存索引
    /// App 启动时调用一次
    pub async fn scan(&self) -> Result<(), SkillError> {
        let skills = self.repo.list_skills().await?;
        let mut cache = self.cache.write().await;
        *cache = skills;
        Ok(())
    }

    /// 查询所有 Skill（含元数据和启用状态）
    pub async fn list(&self) -> Vec<Skill> {
        self.cache.read().await.clone()
    }

    /// 按 name 查找（仅已启用的）
    pub async fn find_enabled(&self, name: &str) -> Option<Skill> {
        self.cache
            .read()
            .await
            .iter()
            .find(|s| s.meta.name == name && s.enabled)
            .cloned()
    }

    /// 按域名查找 MBB Skill
    pub async fn find_mbb_by_domain(&self, domain: &str) -> Vec<Skill> {
        self.cache
            .read()
            .await
            .iter()
            .filter(|s| {
                s.enabled
                    && matches!(&s.kind, SkillKind::Mbb { domains } if
                        domains.iter().any(|d| domain.ends_with(d.as_str())))
            })
            .cloned()
            .collect()
    }

    /// 安装新 Skill
    pub async fn install(&self, source: SkillInstallSource) -> Result<SkillMeta, SkillError> {
        let meta = self.repo.install(source).await?;
        // 重新扫描更新索引
        self.scan().await?;
        Ok(meta)
    }

    /// 删除 Skill（仅 UserInstalled）
    pub async fn remove(&self, name: &str) -> Result<(), SkillError> {
        // 校验：Bundled Skill 不可删除
        {
            let cache = self.cache.read().await;
            if let Some(skill) = cache.iter().find(|s| s.meta.name == name) {
                if matches!(skill.kind, SkillKind::Bundled) {
                    return Err(SkillError::CannotRemoveBundledSkill(name.to_string()));
                }
            }
        }
        self.repo.remove(name).await?;
        self.scan().await?;
        Ok(())
    }

    /// 启用/禁用 Skill
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError> {
        self.repo.set_enabled(name, enabled).await?;
        let mut cache = self.cache.write().await;
        if let Some(skill) = cache.iter_mut().find(|s| s.meta.name == name) {
            skill.enabled = enabled;
        }
        Ok(())
    }
}
```

### 5.4 McpManager — MCP Server 生命周期

```rust
// src/application/mcp_manager.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use crate::{
    domain::mcp::{McpServerConfig, McpServerState, McpToolInfo},
    ports::mcp_transport::McpTransport,
    error::SkillError,
};

/// MCP Server 运行时实例（内存中）
struct McpServerInstance {
    config: McpServerConfig,
    state: McpServerState,
    /// 连接建立后的传输层实例
    transport: Option<Box<dyn McpTransport>>,
    /// 此 Server 的工具列表（connected 后填充）
    tools: Vec<McpToolInfo>,
}

/// 工厂 trait：根据 McpTransportConfig 创建对应传输层
pub trait McpTransportFactory: Send + Sync {
    fn create(&self, config: &McpServerConfig) -> Box<dyn McpTransport>;
}

/// MCP Server 生命周期管理器
/// 管理所有 MCP Server 的连接状态、工具列表
pub struct McpManager {
    factory: Arc<dyn McpTransportFactory>,
    /// server_id → 实例
    servers: Arc<RwLock<HashMap<String, McpServerInstance>>>,
}

impl McpManager {
    pub fn new(factory: Arc<dyn McpTransportFactory>) -> Self {
        Self {
            factory,
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册 MCP Server 配置（不立即连接）
    pub async fn register(&self, config: McpServerConfig) {
        let mut servers = self.servers.write().await;
        servers.insert(
            config.id.clone(),
            McpServerInstance {
                config,
                state: McpServerState::Disconnected,
                transport: None,
                tools: vec![],
            },
        );
    }

    /// 连接指定 Server（建立传输、握手、列举工具）
    pub async fn connect(&self, server_id: &str) -> Result<(), SkillError> {
        let mut servers = self.servers.write().await;
        let instance = servers
            .get_mut(server_id)
            .ok_or_else(|| SkillError::McpServerNotFound(server_id.to_string()))?;

        if matches!(instance.state, McpServerState::Connected { .. }) {
            return Ok(()); // 幂等
        }

        instance.state = McpServerState::Connecting;

        let mut transport = self.factory.create(&instance.config);
        let connect_result = tokio::time::timeout(
            std::time::Duration::from_secs(instance.config.connect_timeout_secs as u64),
            transport.connect(),
        )
        .await
        .map_err(|_| SkillError::McpConnectTimeout(server_id.to_string()))
        .and_then(|r| r);

        match connect_result {
            Ok(()) => {
                let tools = transport.list_tools().await?;
                let tool_count = tools.len();
                instance.tools = tools;
                instance.state = McpServerState::Connected { tool_count };
                instance.transport = Some(transport);
                Ok(())
            }
            Err(e) => {
                instance.state = McpServerState::Failed {
                    reason: e.to_string(),
                };
                Err(e)
            }
        }
    }

    /// 断开指定 Server
    pub async fn disconnect(&self, server_id: &str) -> Result<(), SkillError> {
        let mut servers = self.servers.write().await;
        if let Some(instance) = servers.get_mut(server_id) {
            if let Some(transport) = instance.transport.as_mut() {
                let _ = transport.disconnect().await;
            }
            instance.transport = None;
            instance.state = McpServerState::Disconnected;
            instance.tools.clear();
        }
        Ok(())
    }

    /// 列举所有已连接 Server 的所有工具
    pub async fn list_all_tools(&self) -> Vec<McpToolInfo> {
        self.servers
            .read()
            .await
            .values()
            .flat_map(|inst| inst.tools.clone())
            .collect()
    }

    /// 调用 MCP 工具
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, SkillError> {
        let mut servers = self.servers.write().await;
        let instance = servers
            .get_mut(server_id)
            .ok_or_else(|| SkillError::McpServerNotFound(server_id.to_string()))?;

        let transport = instance
            .transport
            .as_ref()
            .ok_or_else(|| SkillError::McpNotConnected(server_id.to_string()))?;

        transport.call_tool(tool_name, arguments).await
    }

    /// 获取所有 Server 的状态快照
    pub async fn server_states(&self) -> HashMap<String, McpServerState> {
        self.servers
            .read()
            .await
            .iter()
            .map(|(id, inst)| (id.clone(), inst.state.clone()))
            .collect()
    }

    /// 连接所有设置了 auto_connect = true 的 Server
    pub async fn connect_auto(&self) -> Vec<(String, SkillError)> {
        let server_ids: Vec<String> = {
            self.servers
                .read()
                .await
                .values()
                .filter(|inst| inst.config.auto_connect)
                .map(|inst| inst.config.id.clone())
                .collect()
        };

        let mut errors = vec![];
        for id in server_ids {
            if let Err(e) = self.connect(&id).await {
                tracing::warn!("MCP Server '{}' auto-connect failed: {}", id, e);
                errors.push((id, e));
            }
        }
        errors
    }
}
```

### 5.5 AgentTemplateService — 模板实例化

```rust
// src/application/agent_template_service.rs

use std::sync::Arc;
use crate::{
    domain::{
        agent_template::{AgentTemplate, GlobalSkillConfig},
        skill::Skill,
        skill_config::SkillRef,
        mcp::McpServerConfig,
    },
    application::{
        skill_store::SkillStore,
        skill_injector::SkillInjector,
        mcp_manager::McpManager,
    },
    error::SkillError,
};

/// 从 AgentTemplate 实例化出运行时所需的所有配置：
/// 1. 合并 GlobalSkillConfig + AgentTemplate::skills
/// 2. 构建 system prompt 注入块
/// 3. 收集需要连接的 MCP Server 列表
/// 4. 导出 MCP Tool 名称列表（供 EngineBuilder::allowed_tools 过滤）
pub struct AgentTemplateService {
    skill_store: Arc<SkillStore>,
    injector: Arc<SkillInjector>,
    mcp_manager: Arc<McpManager>,
    global_config: GlobalSkillConfig,
}

/// 模板实例化结果
pub struct AgentTemplateInstance {
    /// 完整 system prompt（base + skill 注入）
    pub system_prompt: String,
    /// 本次实例需要激活的 MCP Server id 列表
    pub mcp_server_ids: Vec<String>,
    /// 本次实例可用的工具名（包含 MCP 工具），供 AgentConfig::allowed_tools
    pub allowed_tools: Option<Vec<String>>,
}

impl AgentTemplateService {
    pub fn new(
        skill_store: Arc<SkillStore>,
        injector: Arc<SkillInjector>,
        mcp_manager: Arc<McpManager>,
        global_config: GlobalSkillConfig,
    ) -> Self {
        Self { skill_store, injector, mcp_manager, global_config }
    }

    /// 实例化 AgentTemplate，返回运行时配置
    pub async fn instantiate(
        &self,
        template: &AgentTemplate,
    ) -> Result<AgentTemplateInstance, SkillError> {
        // 1. 合并 Skill 引用列表（全局基线 + 模板级叠加 - 模板级排除）
        let skill_refs = self.merge_skill_refs(template);

        // 2. 从 SkillStore 中解析 Skill 实例
        let all_skills = self.skill_store.list().await;

        // 3. 构建 system prompt
        let skill_injection = self
            .injector
            .build_injection(&skill_refs, &all_skills)
            .await?;

        let system_prompt = if skill_injection.is_empty() {
            template.system_prompt_base.clone()
        } else {
            format!("{}\n\n{}", template.system_prompt_base, skill_injection)
        };

        // 4. 合并 MCP Server 列表
        let mcp_server_ids = self.merge_mcp_servers(template).await;

        // 5. 构建工具白名单
        let allowed_tools = self
            .build_allowed_tools(template, &mcp_server_ids)
            .await;

        Ok(AgentTemplateInstance {
            system_prompt,
            mcp_server_ids,
            allowed_tools,
        })
    }

    /// 合并 Skill 引用：全局基线 + include - exclude
    fn merge_skill_refs(&self, template: &AgentTemplate) -> Vec<SkillRef> {
        let mut refs: Vec<SkillRef> = vec![];

        if template.skills.inherit_global {
            refs.extend(self.global_config.enabled_skills.clone());
        }

        // 追加模板级 include
        refs.extend(template.skills.include.clone());

        // 应用模板级 exclude
        let exclude_set: std::collections::HashSet<&str> =
            template.skills.exclude.iter().map(|s| s.as_str()).collect();
        refs.retain(|r| !exclude_set.contains(r.name.as_str()));

        // 应用模板级默认注入策略（单个 SkillRef 没有显式设置时）
        for r in &mut refs {
            if r.injection.is_none() {
                r.injection = Some(template.skills.default_injection.clone());
            }
        }

        refs
    }

    /// 合并 MCP Server 配置并注册到 McpManager
    async fn merge_mcp_servers(&self, template: &AgentTemplate) -> Vec<String> {
        let mut configs: Vec<McpServerConfig> = vec![];

        if template.mcp_servers.inherit_global {
            configs.extend(self.global_config.mcp_servers.clone());
        }

        configs.extend(template.mcp_servers.include.clone());

        let exclude_set: std::collections::HashSet<&str> =
            template.mcp_servers.exclude.iter().map(|s| s.as_str()).collect();
        configs.retain(|c| !exclude_set.contains(c.id.as_str()));

        let ids: Vec<String> = configs.iter().map(|c| c.id.clone()).collect();

        // 注册到 McpManager（幂等操作）
        for config in configs {
            self.mcp_manager.register(config).await;
        }

        ids
    }

    /// 构建 allowed_tools 白名单
    /// MCP 工具命名格式：`mcp:<server_id>:<tool_name>`
    async fn build_allowed_tools(
        &self,
        template: &AgentTemplate,
        mcp_server_ids: &[String],
    ) -> Option<Vec<String>> {
        if let Some(explicit_tools) = &template.allowed_tools {
            return Some(explicit_tools.clone());
        }

        // 如果有 MCP Server，则需要动态构建包含 MCP 工具的列表
        // 否则返回 None（不限制，使用全部注册工具）
        if mcp_server_ids.is_empty() {
            return None;
        }

        // 收集当前已连接的 MCP 工具名
        let mcp_tools: Vec<String> = self
            .mcp_manager
            .list_all_tools()
            .await
            .iter()
            .filter(|t| mcp_server_ids.contains(&t.server_id))
            .map(|t| format!("mcp:{}:{}", t.server_id, t.tool_name))
            .collect();

        if mcp_tools.is_empty() {
            None
        } else {
            Some(mcp_tools)
        }
    }
}
```

---

## 6. 适配器实现

### 6.1 文件系统 SkillRepository

```rust
// src/adapters/skill_fs.rs

use std::path::{Path, PathBuf};
use async_trait::async_trait;
use serde_yaml;
use crate::{
    domain::skill::{Skill, SkillMeta, SkillBody, SkillResource, SkillKind, ResourceContentType},
    ports::skill_repository::{SkillRepository, SkillInstallSource},
    error::SkillError,
};

/// 文件系统 Skill 仓储
///
/// 目录布局（对应 Wukong 的 resources/ 结构）：
///   <bundled_dir>/           — App bundle 内置，只读
///     skill-name/
///       SKILL.md
///       scripts/
///       references/
///   <mbb_dir>/               — MBB Skills，含 manifest.json
///     manifest.json
///     skill-name/
///       SKILL.md
///       ...
///   <user_dir>/              — 用户自装，读写
///     skill-name/
///       SKILL.md
///       ...
pub struct FsSkillRepository {
    bundled_dir: PathBuf,
    mbb_dir: PathBuf,
    user_dir: PathBuf,
    /// 技能启用状态持久化文件（TOML 格式）
    state_file: PathBuf,
}

impl FsSkillRepository {
    pub fn new(
        bundled_dir: impl Into<PathBuf>,
        mbb_dir: impl Into<PathBuf>,
        user_dir: impl Into<PathBuf>,
        state_file: impl Into<PathBuf>,
    ) -> Self {
        Self {
            bundled_dir: bundled_dir.into(),
            mbb_dir: mbb_dir.into(),
            user_dir: user_dir.into(),
            state_file: state_file.into(),
        }
    }

    /// 解析单个 Skill 目录
    async fn parse_skill_dir(
        &self,
        dir: &Path,
        kind: SkillKind,
        enabled_names: &std::collections::HashSet<String>,
    ) -> Result<Option<Skill>, SkillError> {
        let skill_md_path = dir.join("SKILL.md");
        if !skill_md_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))?;

        let meta = Self::parse_frontmatter(&content)?;
        let enabled = enabled_names.contains(&meta.name);

        Ok(Some(Skill {
            meta,
            kind,
            root_path: dir.to_path_buf(),
            enabled,
        }))
    }

    /// 解析 YAML frontmatter（--- 分隔符之间的内容）
    fn parse_frontmatter(content: &str) -> Result<SkillMeta, SkillError> {
        // 查找第一个 "---" 和第二个 "---"
        let after_first = content
            .strip_prefix("---")
            .and_then(|s| s.strip_prefix('\n').or_else(|| s.strip_prefix("\r\n")))
            .ok_or(SkillError::InvalidSkillMd("missing opening '---'".to_string()))?;

        let end_idx = after_first
            .find("\n---")
            .ok_or(SkillError::InvalidSkillMd("missing closing '---'".to_string()))?;

        let yaml_str = &after_first[..end_idx];

        serde_yaml::from_str::<SkillMeta>(yaml_str)
            .map_err(|e| SkillError::InvalidFrontmatter(e.to_string()))
    }

    /// 解析 SKILL.md body（frontmatter 之后）
    fn parse_body(content: &str) -> SkillBody {
        // 跳过 frontmatter
        let body_start = content
            .find("\n---\n")
            .map(|i| i + 5) // 跳过 \n---\n
            .unwrap_or(0);
        let markdown = content[body_start..].trim().to_string();
        let estimated_tokens = (markdown.len() / 4) as u32;
        SkillBody { markdown, estimated_tokens }
    }

    /// 读取 MBB manifest.json 中的域名绑定
    async fn read_mbb_manifest(&self) -> HashMap<String, Vec<String>> {
        // manifest.json 结构：
        // { "skills": [{ "id": "12306-train-query", "domains": ["12306.cn"], ... }] }
        let manifest_path = self.mbb_dir.join("manifest.json");
        let Ok(content) = tokio::fs::read_to_string(&manifest_path).await else {
            return HashMap::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        if let Some(skills) = value["skills"].as_array() {
            for skill in skills {
                let id = skill["id"].as_str().unwrap_or_default().to_string();
                let domains = skill["domains"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                map.insert(id, domains);
            }
        }
        map
    }
}

#[async_trait]
impl SkillRepository for FsSkillRepository {
    async fn list_skills(&self) -> Result<Vec<Skill>, SkillError> {
        let enabled_names = self.load_enabled_names().await;
        let mbb_domains = self.read_mbb_manifest().await;
        let mut skills = vec![];

        // 扫描 Bundled Skills
        if let Ok(mut entries) = tokio::fs::read_dir(&self.bundled_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                    if let Ok(Some(skill)) = self
                        .parse_skill_dir(&entry.path(), SkillKind::Bundled, &enabled_names)
                        .await
                    {
                        skills.push(skill);
                    }
                }
            }
        }

        // 扫描 MBB Skills
        if let Ok(mut entries) = tokio::fs::read_dir(&self.mbb_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                    let dir_name = entry.file_name().to_string_lossy().to_string();
                    let domains = mbb_domains.get(&dir_name).cloned().unwrap_or_default();
                    let kind = SkillKind::Mbb { domains };
                    if let Ok(Some(skill)) = self
                        .parse_skill_dir(&entry.path(), kind, &enabled_names)
                        .await
                    {
                        skills.push(skill);
                    }
                }
            }
        }

        // 扫描用户自装 Skills
        if let Ok(mut entries) = tokio::fs::read_dir(&self.user_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                    if let Ok(Some(skill)) = self
                        .parse_skill_dir(&entry.path(), SkillKind::UserInstalled, &enabled_names)
                        .await
                    {
                        skills.push(skill);
                    }
                }
            }
        }

        Ok(skills)
    }

    async fn load_body(&self, name: &str) -> Result<SkillBody, SkillError> {
        let skill = self
            .find_skill(name)
            .await?
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        let skill_md_path = skill.root_path.join("SKILL.md");
        let content = tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))?;

        Ok(Self::parse_body(&content))
    }

    async fn load_resource(
        &self,
        name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError> {
        let skill = self
            .find_skill(name)
            .await?
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        // 安全检查：防止路径穿越
        let resource_path = skill.root_path.join(relative_path);
        if !resource_path.starts_with(&skill.root_path) {
            return Err(SkillError::PathTraversal(relative_path.to_string()));
        }

        let content = tokio::fs::read(&resource_path)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))?;

        let content_type = match resource_path.extension().and_then(|e| e.to_str()) {
            Some("md") | Some("markdown") => ResourceContentType::Markdown,
            Some("py") => ResourceContentType::Python,
            Some("js") => ResourceContentType::JavaScript,
            Some("ts") => ResourceContentType::TypeScript,
            Some("sh") | Some("bash") => ResourceContentType::Shell,
            Some(ext) => ResourceContentType::Other(ext.to_string()),
            None => ResourceContentType::Binary,
        };

        Ok(SkillResource {
            relative_path: relative_path.to_string(),
            content,
            content_type,
        })
    }

    async fn install(&self, source: SkillInstallSource) -> Result<SkillMeta, SkillError> {
        match source {
            SkillInstallSource::LocalZip(zip_path) => {
                // 解压到 user_dir/<skill_name>/
                self.extract_zip(&zip_path).await
            }
            SkillInstallSource::LocalDir(dir_path) => {
                // 验证 SKILL.md 存在，复制到 user_dir/
                self.copy_dir(&dir_path).await
            }
            SkillInstallSource::RemoteUrl(url) => {
                // 下载到临时文件，再按 LocalZip 处理
                self.download_and_install(&url).await
            }
        }
    }

    // ... (其余方法实现略，逻辑直接)
}
```

### 6.2 MCP Stdio 传输层（基于 rmcp）

```rust
// src/adapters/mcp_stdio.rs

use async_trait::async_trait;
use rmcp::{ServiceExt, transport::TokioChildProcess};
use tokio::process::Command;
use crate::{
    domain::mcp::{McpServerConfig, McpToolInfo, McpTransportConfig},
    ports::mcp_transport::McpTransport,
    error::SkillError,
};

/// 基于 rmcp crate 的 stdio MCP 传输层
/// 通过子进程 stdin/stdout 与 MCP Server 通信
pub struct McpStdioTransport {
    config: McpServerConfig,
    /// rmcp 服务端 peer（连接建立后持有）
    peer: Option<rmcp::RoleClient>,
}

impl McpStdioTransport {
    pub fn new(config: McpServerConfig) -> Self {
        Self { config, peer: None }
    }
}

#[async_trait]
impl McpTransport for McpStdioTransport {
    async fn connect(&mut self) -> Result<(), SkillError> {
        let McpTransportConfig::Stdio { command, args, env } = &self.config.transport else {
            return Err(SkillError::TransportMismatch);
        };

        let mut cmd = Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }

        let transport = TokioChildProcess::new(&mut cmd)
            .map_err(|e| SkillError::McpTransport(e.to_string()))?;

        // rmcp 握手：initialize + list_tools
        let peer = ()
            .serve(transport)
            .await
            .map_err(|e| SkillError::McpTransport(e.to_string()))?;

        self.peer = Some(peer);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), SkillError> {
        // rmcp peer 的 Drop 会关闭连接
        self.peer = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.peer.is_some()
    }

    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, SkillError> {
        let peer = self
            .peer
            .as_ref()
            .ok_or_else(|| SkillError::McpNotConnected(self.config.id.clone()))?;

        // 调用 MCP tools/list
        let tools_result = peer
            .list_tools(Default::default())
            .await
            .map_err(|e| SkillError::McpTransport(e.to_string()))?;

        Ok(tools_result
            .tools
            .into_iter()
            .map(|t| McpToolInfo {
                server_id: self.config.id.clone(),
                tool_name: t.name.to_string(),
                description: t.description.as_deref().unwrap_or("").to_string(),
                input_schema: serde_json::to_value(&t.input_schema).unwrap_or_default(),
            })
            .collect())
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, SkillError> {
        let peer = self
            .peer
            .as_ref()
            .ok_or_else(|| SkillError::McpNotConnected(self.config.id.clone()))?;

        let params = rmcp::model::CallToolParams {
            name: tool_name.into(),
            arguments: arguments.as_object().cloned().map(Into::into),
        };

        let result = peer
            .call_tool(params)
            .await
            .map_err(|e| SkillError::McpToolCall(e.to_string()))?;

        // 将 MCP 结果序列化为 serde_json::Value
        serde_json::to_value(&result.content)
            .map_err(|e| SkillError::Serialization(e.to_string()))
    }
}
```

### 6.3 MCP Tool Adapter — 桥接 srow_engine::Tool

MCP Tool 必须实现 `srow_engine::ports::tool::Tool` trait，这样它才能被注入 `ToolRegistry`，由 `AgentEngine` 在工具调用时直接执行。

```rust
// src/adapters/mcp_tool_adapter.rs

use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;
use srow_engine::ports::tool::{Tool, ToolContext, ToolDefinition};
use srow_engine::domain::tool::ToolResult;
use srow_engine::error::EngineError;
use crate::application::mcp_manager::McpManager;
use crate::domain::mcp::McpToolInfo;

/// 将单个 MCP Tool 包装为 srow_engine Tool trait 实现
///
/// 工具名格式：`mcp:<server_id>:<tool_name>`
/// 这样 AgentEngine 的 ToolRegistry 中 MCP 工具与内置工具共存，
/// Agent 通过标准 tool_call 机制调用，无需感知 MCP 细节
pub struct McpToolAdapter {
    info: McpToolInfo,
    manager: Arc<McpManager>,
}

impl McpToolAdapter {
    pub fn new(info: McpToolInfo, manager: Arc<McpManager>) -> Self {
        Self { info, manager }
    }

    /// 生成 AgentEngine 内部使用的工具名
    pub fn tool_name(server_id: &str, tool_name: &str) -> String {
        format!("mcp:{}:{}", server_id, tool_name)
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        // 注意：此处返回的是 "mcp:<server_id>:<tool_name>" 格式
        // 需要存储为 String，通过 lazy_static 或 Arc<String> 持有
        // 简化实现：由 ToolRegistry 在注册时用 name() 的返回值作 key
        // 这里返回静态格式，实际需 Box 持有拼接后的字符串
        &self.info.tool_name // 实际实现需持有拼接后的全名
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::tool_name(&self.info.server_id, &self.info.tool_name),
            description: format!(
                "[MCP:{}] {}",
                self.info.server_id, self.info.description
            ),
            parameters: self.info.input_schema.clone(),
        }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();

        let result = self
            .manager
            .call_tool(&self.info.server_id, &self.info.tool_name, input)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let output = serde_json::to_string_pretty(&result)
            .unwrap_or_else(|_| result.to_string());

        Ok(ToolResult {
            tool_call_id: String::new(), // 由引擎层填充
            tool_name: Self::tool_name(&self.info.server_id, &self.info.tool_name),
            output,
            is_error: false,
            duration_ms: start.elapsed().as_millis() as u64,
            created_at: chrono::Utc::now(),
        })
    }
}

/// 将 McpManager 中所有已连接工具转换为 Tool 列表
pub fn build_mcp_tools(
    manager: Arc<McpManager>,
    tools_info: Vec<McpToolInfo>,
) -> Vec<Box<dyn Tool>> {
    tools_info
        .into_iter()
        .map(|info| -> Box<dyn Tool> {
            Box::new(McpToolAdapter::new(info, manager.clone()))
        })
        .collect()
}
```

---

## 7. 内置元工具（Skill 发现与加载工具）

Sub-4 向 AgentEngine 注入两个专用工具，使 Agent 具备运行时 Skill 发现和按需加载能力。这两个工具都实现 `srow_engine::ports::tool::Tool`。

### 7.1 search_skills 工具

```rust
/// 工具名: search_skills
/// 功能: Agent 通过关键词搜索可用 Skill，获取元数据摘要
/// 对应 Wukong 的 Level 1 → Level 2 触发机制

// 输入 schema
#[derive(Debug, Deserialize)]
pub struct SearchSkillsInput {
    /// 搜索关键词（可选，空则列出所有启用的 Skill）
    pub query: Option<String>,
}

// 输出（JSON）
// [{ "name": "docx", "description": "...", "kind": "bundled" }, ...]
```

JSON Schema:
```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "Search query to filter skills by name or description. Empty returns all enabled skills."
    }
  }
}
```

### 7.2 use_skill 工具

```rust
/// 工具名: use_skill
/// 功能: 加载指定 Skill 的指令层（SKILL.md body）或完整资源列表
/// 对应 Wukong 的 use_skill(level="preview"|"full") 机制

// 输入 schema
#[derive(Debug, Deserialize)]
pub struct UseSkillInput {
    /// Skill name（kebab-case）
    pub name: String,
    /// "preview": 返回 SKILL.md body（Level 2）
    /// "full": 返回 SKILL.md body + 资源路径列表（Level 3 入口）
    pub level: UseSkillLevel,
    /// 指定要加载的资源路径（level="full" 时可选填，直接返回文件内容）
    pub resource_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UseSkillLevel {
    Preview,
    Full,
}
```

JSON Schema:
```json
{
  "type": "object",
  "required": ["name", "level"],
  "properties": {
    "name": {
      "type": "string",
      "description": "Skill name in kebab-case, e.g. 'docx'"
    },
    "level": {
      "type": "string",
      "enum": ["preview", "full"],
      "description": "preview: load SKILL.md body. full: load body + resource index"
    },
    "resource_path": {
      "type": "string",
      "description": "Optional: relative path within skill (e.g. 'references/api.md') to load specific resource content"
    }
  }
}
```

---

## 8. 配置文件格式

### 8.1 AgentTemplate 配置文件（agent-template.toml）

存储在 `~/.srow/templates/<template-id>.toml` 或 workspace `.srow/templates/` 下。

```toml
# ~/.srow/templates/browser-agent.toml

id = "browser-agent"
name = "浏览器 Agent"
description = "具备完整浏览器自动化能力的通用 Agent，自动根据访问域名激活对应 MBB Skill"

system_prompt_base = """
You are a browser automation agent with access to a full web browser.
You can navigate websites, fill forms, extract data, and automate web workflows.
"""

max_iterations = 30

[skills]
inherit_global = true
default_injection = "auto"

  [[skills.include]]
  name = "12306-train-query"
  injection = "auto"

  [[skills.include]]
  name = "ctrip-flight-search"
  injection = "auto"

  [[skills.include]]
  name = "dianping-info-query"
  injection = "auto"

exclude = []

[mcp_servers]
inherit_global = true

  [[mcp_servers.include]]
  id = "builtin-browser"
  display_name = "Browser"
  auto_connect = true
  connect_timeout_secs = 10

  [mcp_servers.include.transport]
  type = "stdio"
  command = "/path/to/browser-runtime"
  args = ["--mcp"]
  env = {}

exclude = []
```

### 8.2 全局 Skill 配置文件（skills.toml）

```toml
# ~/.srow/skills.toml

# 全局启用的 Skill（所有 Agent 基线）
[[enabled_skills]]
name = "docx"
injection = "auto"

[[enabled_skills]]
name = "pdf"
injection = "auto"

[[enabled_skills]]
name = "xlsx"
injection = "auto"

# 全局 MCP Servers（所有 Agent 共享）
[[mcp_servers]]
id = "builtin-system-permissions"
display_name = "System Capabilities"
auto_connect = true
connect_timeout_secs = 5

  [mcp_servers.transport]
  type = "stdio"
  command = "/path/to/system-mcp-server"
  args = []
  env = {}
```

### 8.3 MCP Server 配置文件（mcpServerConfig.json，兼容 Wukong 格式）

为与 Wukong 的 `mcpServerConfig.json` 格式兼容，提供 JSON 版本：

```json
{
  "servers": [
    {
      "id": "builtin-browser",
      "display_name": "Browser",
      "transport": {
        "type": "stdio",
        "command": "/usr/local/bin/browser-mcp",
        "args": ["--port", "0"],
        "env": {
          "BROWSER_HEADLESS": "true"
        }
      },
      "auto_connect": true,
      "connect_timeout_secs": 10
    },
    {
      "id": "filesystem-mcp",
      "display_name": "Filesystem",
      "transport": {
        "type": "sse",
        "url": "http://127.0.0.1:3001/sse",
        "headers": {}
      },
      "auto_connect": false,
      "connect_timeout_secs": 5
    }
  ]
}
```

---

## 9. Skill 目录布局

对应 Wukong 的 `resources/` 目录结构，Srow Agent 的 Skill 存储规范：

```
~/.srow/
├── skills.toml                      # 全局 Skill 启用状态和 MCP Server 配置
├── templates/                       # AgentTemplate 配置文件
│   ├── browser-agent.toml
│   ├── coding-agent.toml
│   └── ...
├── skills/                          # 用户自装 Skill 目录
│   └── my-custom-skill/
│       ├── SKILL.md
│       ├── scripts/
│       └── references/
└── skill-states.json                # Skill 启用/禁用状态持久化

<app-bundle>/Resources/
├── bundled-skills/                  # 随 App 分发的内置 Skill（只读，已解压）
│   ├── docx/
│   │   ├── SKILL.md
│   │   ├── scripts/
│   │   └── references/
│   ├── pdf/
│   └── ...
└── mbb-skills/                      # 浏览器增强 Skill（只读）
    ├── manifest.json                # 域名路由表
    ├── 12306-train-query/
    │   ├── SKILL.md
    │   └── references/
    └── ...
```

---

## 10. 与 Sub-2 的集成接口

### 10.1 EngineBuilder 扩展

`srow-skills` 提供 `SkillEngineExt` trait，为 Sub-2 的 `EngineBuilder` 添加 Skill/MCP 注入能力：

```rust
// src/lib.rs (srow-skills 公开接口)

use srow_engine::{EngineBuilder, AgentConfig};
use crate::{
    application::{
        agent_template_service::{AgentTemplateService, AgentTemplateInstance},
        mcp_manager::McpManager,
    },
    adapters::mcp_tool_adapter::build_mcp_tools,
    error::SkillError,
};
use std::sync::Arc;

/// 为 EngineBuilder 扩展 Skill/MCP 注入能力
pub trait SkillEngineExt: Sized {
    /// 从 AgentTemplate 实例化结果注入 Skill/MCP
    /// 1. 将 system_prompt 追加到 AgentConfig::system_prompt
    /// 2. 将 MCP Tools 通过 with_tool() 注册到 ToolRegistry
    /// 3. 将 use_skill / search_skills 元工具注册到 ToolRegistry
    fn with_skill_instance(
        self,
        instance: AgentTemplateInstance,
        mcp_manager: Arc<McpManager>,
    ) -> Self;
}

impl SkillEngineExt for EngineBuilder {
    fn with_skill_instance(
        mut self,
        instance: AgentTemplateInstance,
        mcp_manager: Arc<McpManager>,
    ) -> Self {
        // 1. 合并 system prompt
        // EngineBuilder 需要提供 map_config() 方法（Sub-2 预留扩展点）
        self = self.map_config(|mut config| {
            if !instance.system_prompt.is_empty() {
                config.system_prompt = format!(
                    "{}\n\n{}",
                    config.system_prompt, instance.system_prompt
                );
            }
            if let Some(tools) = instance.allowed_tools {
                config.allowed_tools = Some(tools);
            }
            config
        });

        // 2. 注册 MCP Tools（异步上下文中已连接，此处同步注入）
        let mcp_tools = build_mcp_tools(
            mcp_manager.clone(),
            futures::executor::block_on(mcp_manager.list_all_tools()),
        );
        for tool in mcp_tools {
            self = self.with_tool(tool);
        }

        // 3. 注册元工具（search_skills + use_skill）
        // 这两个工具需要 SkillStore 引用，由调用者提前构造并传入
        self
    }
}
```

### 10.2 完整集成示例

```rust
// 示意：Sub-5 编排层创建 browser-agent 实例的典型代码

use srow_engine::{EngineBuilder, AgentConfig, LLMConfig};
use srow_skills::{
    SkillEngineExt,
    application::agent_template_service::AgentTemplateService,
};

async fn create_browser_agent(
    template_service: Arc<AgentTemplateService>,
    mcp_manager: Arc<McpManager>,
) -> Result<AgentEngine, Box<dyn std::error::Error>> {
    // 1. 加载 AgentTemplate
    let template = load_template("browser-agent").await?;

    // 2. 实例化模板（合并 Skill/MCP，构建 system prompt）
    let instance = template_service.instantiate(&template).await?;

    // 3. 连接所需 MCP Servers
    for server_id in &instance.mcp_server_ids {
        mcp_manager.connect(server_id).await?;
    }

    // 4. 构建 AgentConfig
    let config = AgentConfig {
        name: "browser-agent".to_string(),
        system_prompt: String::new(), // 由 with_skill_instance 填充
        llm: LLMConfig { /* ... */ },
        ..Default::default()
    };

    // 5. 构建引擎，注入 Skill/MCP
    let (event_tx, event_rx) = mpsc::channel(256);
    let (cancel_tx, cancel_rx) = watch::channel(false);

    let engine = EngineBuilder::new(config)
        .with_llm(AnthropicProvider::new(&api_key, "claude-opus-4-5"))
        .with_default_sqlite_storage().await?
        .with_skill_instance(instance, mcp_manager)  // Sub-4 扩展点
        .build(event_tx, cancel_rx)?;

    Ok(engine)
}
```

---

## 11. 错误类型

```rust
// src/error.rs

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("Skill '{0}' not found")]
    SkillNotFound(String),

    #[error("Invalid SKILL.md: {0}")]
    InvalidSkillMd(String),

    #[error("Invalid SKILL.md frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("Cannot remove bundled skill '{0}'")]
    CannotRemoveBundledSkill(String),

    #[error("Path traversal attempt: '{0}'")]
    PathTraversal(String),

    #[error("MCP server '{0}' not found")]
    McpServerNotFound(String),

    #[error("MCP server '{0}' not connected")]
    McpNotConnected(String),

    #[error("MCP server '{0}' connect timed out")]
    McpConnectTimeout(String),

    #[error("MCP transport error: {0}")]
    McpTransport(String),

    #[error("MCP tool call error: {0}")]
    McpToolCall(String),

    #[error("Transport type mismatch for server config")]
    TransportMismatch,

    #[error("Skill zip extraction error: {0}")]
    ZipExtraction(String),

    #[error("Skill download error: {0}")]
    Download(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(String),
}
```

---

## 12. Cargo.toml

```toml
[package]
name = "srow-skills"
version = "0.1.0"
edition = "2021"
description = "Srow Agent skill system and MCP integration"

[lib]
name = "srow_skills"
path = "src/lib.rs"

[dependencies]
# 内部依赖
srow-engine = { path = "../srow-engine" }

# 异步运行时
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
toml = "0.8"

# MCP 协议
rmcp = { version = "0.1", features = ["client", "transport-child-process", "transport-sse-client"] }

# ZIP 解压（Skill 安装）
zip = "2"

# HTTP（远程 Skill 安装）
reqwest = { version = "0.12", features = ["stream", "rustls-tls"], default-features = false }

# 文件系统
walkdir = "2"

# ID / 时间
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }

# 错误处理
thiserror = "1"
anyhow = "1"

# 日志
tracing = "0.1"

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
mockall = "0.12"
wiremock = "0.6"   # 用于 SSE MCP Server mock 测试
```

---

## 13. 公开 API（lib.rs 导出）

```rust
// src/lib.rs

pub mod domain {
    pub mod skill;
    pub mod skill_config;
    pub mod mcp;
    pub mod agent_template;
}

pub mod ports {
    pub mod skill_repository;
    pub mod mcp_transport;
}

pub mod application {
    pub mod skill_loader;
    pub mod skill_store;
    pub mod skill_injector;
    pub mod mcp_manager;
    pub mod agent_template_service;
}

pub mod adapters {
    pub mod skill_fs;
    pub mod mcp_stdio;
    pub mod mcp_sse;
    pub mod mcp_tool_adapter;
}

pub mod error;

// 便捷 re-export
pub use domain::skill::{Skill, SkillMeta, SkillBody, SkillKind};
pub use domain::skill_config::{SkillRef, InjectionPolicy};
pub use domain::mcp::{McpServerConfig, McpServerState, McpToolInfo, McpTransportConfig};
pub use domain::agent_template::{AgentTemplate, AgentTemplateInstance, SkillSet, McpSet, GlobalSkillConfig};
pub use application::skill_store::SkillStore;
pub use application::mcp_manager::McpManager;
pub use application::agent_template_service::AgentTemplateService;
pub use adapters::skill_fs::FsSkillRepository;
pub use adapters::mcp_tool_adapter::build_mcp_tools;
pub use error::SkillError;
pub use ports::skill_repository::{SkillRepository, SkillInstallSource};
pub use ports::mcp_transport::McpTransport;
```

---

## 14. 测试策略

### 14.1 单元测试

```
domain/ 层：
  - SkillMeta 序列化 / 反序列化（各字段边界值）
  - InjectionPolicy merge 逻辑
  - AgentTemplate Skill/MCP 合并规则（include/exclude 组合）

application/skill_loader.rs：
  - build_meta_summary：空列表、含禁用 Skill、混合类型
  - build_explicit_injection：SKILL.md body 注入格式正确
  - parse_frontmatter：标准格式、缺字段、超长 name/description 边界

application/skill_injector.rs：
  - Auto + Explicit + Strict 三种策略的注入结果
  - 多 Skill 混合注入的顺序与格式

application/skill_store.rs：
  - find_mbb_by_domain：精确匹配、suffix 匹配、无匹配
  - set_enabled：内存索引同步更新
```

### 14.2 集成测试

```rust
// tests/skill_fs_test.rs

#[tokio::test]
async fn test_scan_bundled_skills() {
    // 使用 tempfile 创建完整 Skill 目录结构
    // 验证 list_skills 返回正确的 SkillMeta 和 SkillKind
}

#[tokio::test]
async fn test_install_from_zip() {
    // 创建合法 .zip（含 SKILL.md）
    // 验证 install() 解压到 user_dir 并更新索引
}

#[tokio::test]
async fn test_cannot_install_invalid_zip() {
    // 创建缺少 SKILL.md 的 .zip
    // 验证返回 InvalidSkillMd 错误
}

#[tokio::test]
async fn test_mbb_domain_routing() {
    // 创建含 manifest.json 的 mbb-skills 目录
    // 验证 find_mbb_by_domain("www.12306.cn") 返回正确 Skill
}
```

```rust
// tests/mcp_manager_test.rs

#[tokio::test]
async fn test_mcp_stdio_connect_and_list_tools() {
    // 启动一个简单的 MCP echo 服务器（测试用）
    // 验证 McpManager::connect + list_all_tools
}

#[tokio::test]
async fn test_mcp_call_tool() {
    // 通过 McpManager 调用工具，验证参数传递和结果解析
}

#[tokio::test]
async fn test_mcp_connect_timeout() {
    // 连接一个不存在的端口，验证超时错误
}

#[tokio::test]
async fn test_mcp_tool_adapter_as_engine_tool() {
    // 验证 McpToolAdapter 实现 Tool trait：
    // definition() 格式正确，name() 为 "mcp:<server>:<tool>" 格式
}
```

```rust
// tests/agent_template_test.rs

#[tokio::test]
async fn test_template_instantiate_merges_skills() {
    // AgentTemplate { skills: { inherit_global: true, include: [...], exclude: [...] } }
    // 验证合并后的 SkillRef 列表正确（含 exclude 生效）
}

#[tokio::test]
async fn test_template_system_prompt_injection() {
    // 验证 system_prompt_base + skill 注入块的拼接格式
    // Explicit Skill 必须包含 SKILL.md body
    // Auto Skill 只包含 description 摘要
}

#[tokio::test]
async fn test_engine_builder_skill_extension() {
    // 端到端：AgentTemplate → AgentTemplateService → EngineBuilder::with_skill_instance
    // 验证最终 AgentEngine 的 system_prompt 和工具列表正确
}
```

### 14.3 mockall 接口

```rust
use mockall::automock;

#[automock]
#[async_trait]
pub trait SkillRepository: Send + Sync {
    // ... (在 ports/skill_repository.rs 加 #[automock])
}

#[automock]
#[async_trait]
pub trait McpTransport: Send + Sync {
    // ... (在 ports/mcp_transport.rs 加 #[automock])
}
```

---

## 15. 数据流总览

```
                          ┌──────────────────────┐
                          │   AgentTemplate       │
                          │   (TOML 配置文件)     │
                          └──────────┬───────────┘
                                     │
                          AgentTemplateService::instantiate()
                                     │
               ┌─────────────────────┼─────────────────────┐
               │                     │                       │
               ▼                     ▼                       ▼
    ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐
    │   SkillSet 合并   │  │   McpSet 合并    │  │ allowed_tools 计算│
    │  (global+include  │  │ (global+include  │  │ (MCP工具名枚举)  │
    │   -exclude)       │  │  -exclude)       │  └──────────────────┘
    └────────┬─────────┘  └────────┬─────────┘
             │                     │
             ▼                     ▼
    ┌──────────────────┐  ┌──────────────────┐
    │   SkillInjector  │  │   McpManager     │
    │   build_injection│  │   register()     │
    │                  │  │   connect_auto() │
    │ Auto → meta摘要  │  └────────┬─────────┘
    │ Explicit → body  │           │
    └────────┬─────────┘           │ list_all_tools()
             │                     │
             ▼                     ▼
    ┌─────────────────────────────────────────┐
    │         system_prompt (合并后)            │
    │   = system_prompt_base                   │
    │   + [Auto skill 元数据摘要表]             │
    │   + [Explicit skill SKILL.md body]       │
    └──────────────────────┬──────────────────┘
                           │
                           ▼
    ┌─────────────────────────────────────────┐
    │         EngineBuilder::with_skill_instance│
    │                                          │
    │   config.system_prompt = merged          │
    │   registry += McpToolAdapter × N        │
    │   registry += search_skills             │
    │   registry += use_skill                 │
    └──────────────────────┬──────────────────┘
                           │
                           ▼
    ┌─────────────────────────────────────────┐
    │              AgentEngine                 │
    │    LLM 调用时携带完整 system prompt       │
    │    工具调用时可调用 MCP Tool              │
    │    Agent 可通过 use_skill 按需加载 Skill  │
    └─────────────────────────────────────────┘
```

---

## 16. 实现优先级与里程碑

| 里程碑 | 内容 | 验收标准 |
|--------|------|---------|
| M1 | 领域类型 + SKILL.md 解析 | 正确解析所有 10 个 Wukong Skill 包的 frontmatter |
| M2 | FsSkillRepository 实现 | 扫描 bundled-skills + mbb-skills + user 目录，list_skills 返回完整列表 |
| M3 | SkillStore + SkillLoader | scan()、load_body()、load_resource() 正确工作 |
| M4 | SkillInjector + AgentTemplate 合并 | Auto/Explicit/Strict 三种策略注入格式正确 |
| M5 | MCP stdio 传输层（rmcp） | 连接真实 MCP Server，list_tools + call_tool 验证 |
| M6 | McpManager 生命周期 | connect_auto / disconnect / reconnect 状态机正确 |
| M7 | McpToolAdapter 注入引擎 | MCP 工具通过 AgentEngine 工具调用机制正常执行 |
| M8 | AgentTemplateService 端到端 | 从 TOML 配置文件到 AgentEngine 实例的完整流程 |
| M9 | search_skills + use_skill 元工具 | Agent 可在对话中动态发现并加载 Skill |
| M10 | MBB 域名路由接口 | find_mbb_by_domain 正确，为 Sub-6 提供路由查询接口 |
| M11 | Skill 安装（本地 zip + URL） | install / remove / set_enabled 完整功能 |
| M12 | MCP SSE 传输层 | 连接 SSE 型 MCP Server，行为与 stdio 一致 |
