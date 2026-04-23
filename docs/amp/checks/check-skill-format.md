# Check Skill 文件格式

每个 check 是一个独立的 `.md` 文件，格式和 Amp 的普通 skill 非常相似，但 **frontmatter 里多一个 `severity-default` 字段**，且 body 被当作给 check runner 的 system prompt 追加内容。

---

## 文件位置（硬编码常量）

Amp 二进制里常量：

```
ZX  = "checks"       // 目录名
W2R = ".agents"      // 项目子目录
q2R = "amp"          // 全局配置子目录 A（~/.config/amp/checks/）
z2R = "agents"       // 全局配置子目录 B（~/.config/agents/checks/）
```

**发现顺序**（`G2R` 函数）：

1. **从 cwd 向上遍历**到 workspace root，对每层检查 `<dir>/.agents/checks/*.md`
2. 再加上两个全局位置：`~/.config/amp/checks/*.md` + `~/.config/agents/checks/*.md`
3. 按 `name` 去重 —— **先发现的优先**（项目级覆盖全局级）

如果指定了 `checkScope` 参数，只从这个目录开始向上搜，全局位置还是会算。

---

## frontmatter schema

Amp 解析出来的字段（`F2R` 函数）：

```ts
{
  name: string                        // 必需；没有时 fallback 到 file basename 去掉 .md
  description?: string                // 可选；UI 里显示
  "severity-default"?: string         // 可选；默认 "medium"
  tools?: string[]                    // 可选；白名单，空则用 codereview-check 默认工具
}
```

**关键字段**：

| 字段 | 作用 | 默认 |
|---|---|---|
| `name` | check 在 UI 和 issue.source 里的显示名 | filename (without `.md`) |
| `description` | 给人看的一行说明 | — |
| `severity-default` | 当 body 没显式说 severity 时，issue 用这个 | `"medium"` |
| `tools` | frontmatter 里能限定子 agent 可用的工具 | `["Read","Grep","glob","Bash"]`（codereview-check 默认） |

**YAML 解析失败**会走 fallback 路径：`frontmatter = null`，整个文件当 body 处理，`name = filename`。

---

## Body 的语义

Body **不是**原样送给 LLM。Amp 先用 `Q2R` 函数拼装一个完整 system prompt，把 body 当成"描述要查什么模式"那部分拼进去：

```
# ${T.name} Check                         ← 来自 frontmatter.name

Working directory: ${cwd ?? "unknown"}

${body}                                   ← 用户写的检查规则描述

## Files to Review                        ← Amp 自动加
Focus on these files:                     ← 如果 files[] 有值
- path/to/changed-file.ts
- ...
（或 "Review all relevant files in the working directory."）

## Diff Description                        ← Amp 自动加（如果 diffDescription 非空）
Use this description to gather the full diff using git or bash commands:
${diffDescription}

1. Review the git diff to see what changed
2. Search for patterns described above ONLY in the changed lines (+ lines in diff)
3. Report issues ONLY for code that was added or modified in this diff
4. Do NOT report issues for unchanged/pre-existing code

End your response with:
<checkName>${T.name}</checkName>
<status>completed</status>
<filesAnalyzed>NUMBER</filesAnalyzed>
<linesAnalyzed>NUMBER</linesAnalyzed>
<pattern>Brief description of pattern 1</pattern>
<pattern>Brief description of pattern 2</pattern>
<issue severity="${severity-default}" file="path/to/file.ts" line="LINE">
<problem>functionName(): What is wrong (include method/function name if applicable)</problem>
<why>Why this matters</why>
<fix>How to fix it</fix>

IMPORTANT: The "file" attribute MUST use the EXACT path from the diff header
(e.g., "core/src/tools/file.ts"), not just the filename.

## Severity (default: ${severity-default})
- critical: Security vulnerability, data loss, crash
- high: Bug or performance issue
- medium: Code smell or maintainability
- low: Style suggestion
```

所以 **body 应该聚焦于"要找什么"**，不需要写"怎么报告"、"severity 是什么意思"、"只看 changed lines"这些 —— Amp 的 wrapper 已经加了。

---

## 推荐 body 模板

```markdown
---
name: no-panic
description: Ensure no new panic!/unwrap/expect/todo! are introduced in production code
severity-default: high
---

# Patterns to look for

Flag any newly added code that:

1. Calls `panic!(...)` in non-test / non-build-script Rust code.
2. Uses `.unwrap()` or `.expect()` on `Result` or `Option` outside
   of `#[cfg(test)]` modules, `tests/` dirs, or example programs.
3. Uses `todo!()` / `unimplemented!()` in code that is expected to ship.

# Exceptions

- Test code (anywhere under `tests/`, `benches/`, or inside `#[cfg(test)] mod tests`)
- `build.rs` scripts
- Example code under `examples/`
- Inside documentation doctests

# What to report

For each violation, report the specific call (e.g., `User::new(): calls .unwrap()
on parse result, should return Result<Self, ParseError> instead`).
```

---

## 为什么 body 要这样写

1. **"要找什么" 比 "怎么找" 重要** —— LLM 自己决定工具顺序。你写 "ONLY report new additions" 只是重复 wrapper，浪费 token。
2. **Examples > Rules** —— LLM 对示例比对抽象规则敏感。写 3 行示例胜过 10 行描述。
3. **显式列 Exceptions** —— 否则 LLM 把 test 代码一起报，噪声大。
4. **"What to report" 提醒 LLM 如何填 `<problem>`** —— 格式统一利于后续处理。

---

## frontmatter 解析行为（反编译确认）

```js
function F2R(T) {
  let R = T.match(/^---\s*\n([\s\S]*?)\n---\s*\n([\s\S]*)$/);
  if (!R) return { frontmatter: null, body: T };

  // ... parse YAML, fallback if fails
  return {
    frontmatter: {
      name: typeof e.name === "string" ? e.name : "unknown",
      description: typeof e.description === "string" ? e.description : void 0,
      "severity-default": e["severity-default"],
      tools: Array.isArray(e.tools) ? e.tools.filter(
        (h) => typeof h === "string"
      ) : void 0
    },
    body: r
  };
}
```

**注意**：frontmatter 只读 4 个字段，其他都被丢弃（不会报错）。所以你可以放额外元数据（`author:` / `version:` / `tags:`）但 Amp 用不上。

---

## Tools 白名单（不常用）

`codereview-check` 默认开的工具是：

```
["Read", "Grep", "glob", "Bash"]
```

没有 `web_search`、没有 `read_web_page`、没有 `edit_file`（check 是只读的，不改文件）。

如果你想进一步限制某个 check 不能跑 Bash（纯静态扫描），frontmatter 加：

```yaml
tools:
  - Read
  - Grep
  - glob
```

---

## 对 Alva 的启发

Amp 的 check 文件格式有几点值得 Alva 抄：

### 1. Check = 小 skill，不是单独资源

Alva 已经有 `ExtensionManifest` / `SkillBuilder`，可以把 check 直接建模成 **一个 `SkillKind::Check` 的变体**：

```rust
// alva-extension-core/src/skill.rs
#[derive(Debug, Clone)]
pub enum SkillKind {
    Regular {
        trigger_words: Vec<String>,
    },
    Check {
        /// Review 时如果 frontmatter 没有显式 severity，用这个
        severity_default: Severity,
        /// 可选：限制 check subagent 能用哪些工具
        tools: Option<Vec<String>>,
    },
}
```

好处：check 和 skill 共享 discovery / loader / frontmatter 解析，减一半代码。

### 2. **向上遍历 + 全局 fallback 的发现路径**

Amp 从 cwd 一路往上到 workspace root 收集 `.agents/checks/*.md`，再加两个全局位置，然后按 name 去重（先发现的赢）。这样：

- 一个大 monorepo 可以在 `crates/foo/` 放专属 check
- 根目录放共享 check
- `~/.config/` 放个人习惯 check（全项目生效）

Alva 的 `ExtensionRegistry` 已经有 scope 概念（project/user/global），check 直接复用即可：

```rust
pub fn discover_checks(
    cwd: &Path,
    workspace_root: &Path,
    user_config_dir: &Path,
) -> Vec<CheckManifest> {
    let mut seen = HashMap::<String, CheckManifest>::new();

    // Project scope: cwd 向上走
    let mut dir = cwd;
    loop {
        scan_dir(&dir.join(".alva/checks"), &mut seen);
        if dir == workspace_root { break; }
        dir = match dir.parent() { Some(p) => p, None => break };
    }

    // Global scope
    scan_dir(&user_config_dir.join("alva/checks"), &mut seen);
    scan_dir(&user_config_dir.join("agents/checks"), &mut seen);

    seen.into_values().collect()
}
```

### 3. `severity-default` 是 check 作者能控制的唯一维度

Amp 没暴露 `enabled` / `disabled` / `severity-override`，只有 `severity-default`。简单到位。Alva 抄这个，**不要引入 `.alvaignore` 之类复杂的配置**。

### 4. Body 写自然语言，不要发明 DSL

Amp 的 check 就是**给 LLM 的自然语言 prompt**。抵制住"发明一种 check DSL"的诱惑。如果你的 check 需要 DSL 才能表达，说明它应该是一个 tool 或者 lint，不是 check。

### 5. 文件命名约定

- `check-skill-format.md` → 技术规格
- `builtin-checks.md` → 用户能装什么
- `code-review-integration.md` → 怎么串起来

把 CHECK 文件名当成**可在文档里直接引用**的稳定 key，比如 issue 里说 `#no-panic check` 能让人直接找到 `.alva/checks/no-panic.md`。
