# 内置 Checks 清单

## TL;DR —— Amp **没有内置任何 check**

反编译确认：binary 的 strings 里找不到任何预置的 check 文件。搜索 `security-check` / `general-check` / `checks/security` / `built.in check` 等模式：

```
$ grep -nE "security-check|general-check|checks/security|checks/general" strings.txt
(no matches)
```

所有 checks 都是**用户侧资产**：

- 放在项目 `.agents/checks/*.md`
- 或全局 `~/.config/amp/checks/*.md` / `~/.config/agents/checks/*.md`
- 或通过 skill 包从 GitHub 装的（`amp skill add @user/no-panic` → 如果包里有 checks 目录，一起装过来）

---

## 为什么 Amp 不预置 checks

这是**刻意的产品决策**：

1. **Review 标准因项目而异** —— 电商、嵌入式、ML、infra 对"什么叫 bug"的阈值完全不一样。预置通用规则会吵。
2. **通用规则已经在主 reviewer 的 prompt 里** —— `iuT` 里写了"bugs, hackiness, unnecessary code, too much shared mutable state, over/under-abstraction"。checks 的定位是**只补充通用规则之外的项目专属规则**。
3. **Skills 市场模式** —— Amp 鼓励社区通过 GitHub share skills，checks 搭顺风车。预置会和社区包冲突。
4. **Discovery 简单** —— `.agents/checks/` 目录空着就不跑 checks，没有"如何关闭内置 check"的 UX 问题。

---

## 用户常用的 check 模式（从 skill 市场和 docs 推断）

虽然 Amp 没预置，但社区里常见这些类别。列出来给 Alva 抄作业：

### 1. 安全类（`security/*.md`）

- **no-hardcoded-secret** — 检查新增代码里的 API key / 私钥字面量
- **no-sql-injection** — 检查字符串拼接 SQL
- **no-eval** — 检查 `eval` / `Function(...)` / `exec` 在不该出现的地方
- **no-shell-injection** — 检查 child_process 里未转义的用户输入

典型 severity-default: `critical` 或 `high`

### 2. 可靠性类（`reliability/*.md`）

- **no-panic** — Rust 的 `panic!` / `.unwrap()` / `.expect()` 在生产代码里
- **no-unwrap-outside-test** — 更严格的变种
- **no-unsafe** — Rust 的 `unsafe` 块必须有 safety 注释
- **no-unhandled-promise** — JS 里未 await 的 Promise
- **no-swallow-error** — `catch {}` 或 `.catch(() => {})` 丢错误
- **error-context** — Rust `?` 前必须有 `.context(...)`（anyhow/eyre 风格）

### 3. 风格 / 架构类（`style/*.md`）

- **no-dead-code** — 新增的 unused function/struct/module
- **max-file-length** — 单文件超 N 行
- **layer-boundary** — 不允许某些 crate/module 跨层直接引用
- **no-print-in-prod** — `println!` / `eprintln!` / `console.log` 在生产代码
- **todo-must-have-tracker** — `TODO` / `FIXME` 必须带 issue 号

### 4. 团队文化类（`team/*.md`）

- **no-inline-html** — 必须走模板系统
- **all-handlers-return-result** — REST endpoint 必须返回 Result
- **migration-has-down** — DB migration 必须有 up + down
- **changelog-required** — 改了 public API 必须更新 CHANGELOG

---

## Check 模板库（推荐初始套装）

Alva 可以在 `docs/checks/examples/` 放一套可直接拷贝的 checks，给新用户参考：

```
.alva/checks/
├── no-panic.md                    # severity-default: high
├── no-unsafe-without-comment.md   # severity-default: high
├── no-hardcoded-secret.md         # severity-default: critical
├── error-context.md               # severity-default: medium
├── max-file-length.md             # severity-default: low
└── README.md                      # 介绍如何自定义
```

每个文件 20-40 行，像下面这样：

```markdown
---
name: no-panic
description: Disallow panic! / unwrap / expect in non-test Rust code
severity-default: high
---

Look for any newly added code that would cause a process crash on bad input:

- Direct calls to `panic!(...)`, `unreachable!(...)`, `todo!()`, `unimplemented!()`
- `.unwrap()` or `.expect("...")` on `Result` or `Option`
- Explicit `process::exit(...)` outside of main / binaries / error paths

Exceptions:
- `#[cfg(test)]` modules and code in `tests/`, `benches/`, `examples/`
- `build.rs` scripts
- Doctests

For each violation report:
- Exact function / method name
- Why it's risky (what input / state would trigger crash)
- A suggested replacement (usually "return Result<_, YourError> instead")
```

---

## 关于 Amp skill 包携带 checks

Amp CLI 有 `amp skill add <source>` 命令（见反编译 line 65534 附近）：

```
amp skill add @user/skill-name        # GitHub @handle 形式
amp skill add owner/repo              # repo 形式
amp skill add https://git...          # URL
amp skill add ./local/path            # 本地
```

装 skill 时会把 `SKILL.md` 和**整个目录**都拷进 `.agents/skills/<name>/`。如果这个 skill 包在自己目录里有 `checks/` 子目录，因为 check discovery 是扫 `.agents/checks/` 而不是 `.agents/skills/**/checks/`，**这些 checks 默认不会被发现**。用户需要手动把 checks 文件 mv 到 `.agents/checks/` 下。

这是 Amp 当前设计的一个小瑕疵：skill 和 check 共享格式但不共享安装通道。Alva 可以做得更好 —— 见下文"对 Alva 的启发"。

---

## 对 Alva 的启发

**重点：如何实施可插拔 code review 框架** —— 这是本 checks skill 最有价值的输出。

### 决定论：先想清楚 Alva 里 check 是什么地位

三个可能方案，选一个：

| 方案 | 代价 | 收益 |
|---|---|---|
| A. check 是独立资源类型（crate `alva-checks`） | 一套独立 loader/discovery | 职责清晰、以后可以加 check-specific 逻辑 |
| B. check 是 skill 的变体（复用 `alva-extension-core`） | frontmatter 加 `kind: check` 字段 | 零新 crate；但 discovery 要加路径分支 |
| C. check 是 extension 的元数据，附带到任何 extension 上 | 每个 extension 可以发一个 check | 最灵活，但概念过载 |

**推荐 B**，理由：
- Amp 验证过 check 和 skill 的 frontmatter 99% 一样
- Alva 的 skill loader 可以直接复用
- 新增一个 `SkillKind` enum 分支代价很小

### 方案 B 的 Rust 骨架

```rust
// crates/alva-extension-core/src/skill.rs
// （已有代码里加一个 kind 字段）

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub kind: SkillKind,
    pub trigger_words: Option<Vec<String>>,
    #[serde(rename = "severity-default")]
    pub severity_default: Option<Severity>,
    pub tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    #[default]
    Regular,
    Check,
}

// loader 里分派到不同路径
impl SkillRegistry {
    pub async fn load_all(&self, workspace_root: &Path, cwd: &Path) -> SkillBundle {
        let mut bundle = SkillBundle::default();

        // 原有 regular skill 逻辑不变
        bundle.regular_skills = self.scan_path(
            workspace_root.join(".alva/skills"),
            SkillKind::Regular,
        ).await;

        // 新增 check 发现逻辑：向上遍历
        bundle.checks = self.discover_checks(cwd, workspace_root).await;

        bundle
    }

    async fn discover_checks(&self, cwd: &Path, ws_root: &Path) -> Vec<SkillManifest> {
        let mut seen: indexmap::IndexMap<String, SkillManifest> = Default::default();

        let mut dir = cwd;
        loop {
            self.scan_into_checks(&dir.join(".alva/checks"), &mut seen).await;
            if dir == ws_root { break }
            dir = match dir.parent() { Some(p) => p, None => break };
        }

        for global_dir in &self.global_check_dirs {
            self.scan_into_checks(global_dir, &mut seen).await;
        }

        seen.into_values().collect()
    }
}
```

### `code_review` tool 实施清单

```rust
// crates/alva-extension-codereview/src/lib.rs

use alva_agent_core::tool::{Tool, ToolCall, ToolSpec};
use alva_extension_core::Extension;

pub struct CodeReviewExtension {
    orchestrator: Arc<CodeReviewOrchestrator>,
}

impl Extension for CodeReviewExtension {
    fn name(&self) -> &str { "codereview" }

    fn tools(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "code_review".into(),
            description: include_str!("../prompts/description.md").into(),
            input_schema: code_review_schema(),
            disable_timeout: true,
        }]
    }

    // ... subagent spec registration
}

fn code_review_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "diff_description": { "type": "string", "description": "..." },
            "files":            { "type": "array", "items": { "type": "string" } },
            "instructions":     { "type": "string" },
            "check_scope":      { "type": "string" },
            "check_filter":     { "type": "array", "items": { "type": "string" } },
            "checks_only":      { "type": "boolean" },
            "thinking":         { "type": "string", "enum": ["low", "high"] }
        },
        "required": ["diff_description"]
    })
}
```

### 两个独立 subagent spec

```rust
// alva-agent-core 已有 SubagentSpec 系统，注册两个：

SubagentSpec {
    key: "code-review".into(),
    display_name: "Code Review".into(),
    model: ModelId::GeminiProOrAnthropicOpus,   // 高推理模型
    include_tools: vec!["Read", "Grep", "glob", "web_search", "read_web_page", "Bash"]
        .into_iter().map(Into::into).collect(),
    allow_mcp: false,
    allow_toolbox: false,
}

SubagentSpec {
    key: "codereview-check".into(),
    display_name: "Codereview Check".into(),
    model: ModelId::Haiku,                      // 便宜小模型
    include_tools: vec!["Read", "Grep", "glob", "Bash"]
        .into_iter().map(Into::into).collect(),
    allow_mcp: false,
    allow_toolbox: false,
}
```

### 抄 Amp 的 5 个细节

1. **向上遍历目录 + 全局 fallback + 按 name 去重**（`Y2R` 逻辑） —— monorepo 友好
2. **每个 check 跑在 cheap model 上**（Haiku 级别） —— 10 个 checks × cheap >> 1 次 big model
3. **失败重试 1 次即放弃**（不 infinite retry） —— checks 不应该阻断 review
4. **主 reviewer 和 checks 并发，结果合并**（RxJS `U3` / Rust `tokio::join!`） —— 总时长 ~= max(main, max(checks))
5. **严格要求 check 只报 diff 新增/修改行**（在 system prompt 里强调） —— 噪声控制的核心

### 不抄的 2 个点

1. **把合并结果不去重** —— Alva 应该按 `(file, line, severity)` 去重，main 和 check 重叠时优先保留 check 的（因为 check 的 `source` 标签对用户更有用）
2. **`checksOnly: true` 发送空 `<codeReview></codeReview>` 给主路径** —— 这是 Amp 的 hack。Alva 直接在合并层加分支更清晰

### 配套建议

1. 做一个 `alva review` 子命令，不需要启动 agent 会话就能跑 code_review（CI 友好）
2. 输出格式支持 `--format json|github-actions|markdown|tty`
3. `--fail-on critical,high` 让 CI 在高严重度 issue 时失败
4. 把 check 的 `description` 字段通过 `alva checks list` 暴露出来，用户能查有哪些 check 生效

### 交叉引用

- `../prompts/subagents.md` Code Reviewer 节 —— 主 reviewer prompt 原文
- `./check-skill-format.md` —— check 文件格式细节
- `./code-review-integration.md` —— tool 调用到结果合并的完整链路
- `../skills/SKILL.md` —— skill 系统（checks 复用的部分）
