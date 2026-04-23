# `code_review` 工具 × checks 框架 集成

`code_review` 是 Amp 的 builtin tool，入口变量 `XtT`。当 agent 调用它时，内部 fork 两条并行流：主 reviewer（一次）+ 每个 check 一条（N 条），然后结果合并。

---

## Tool spec

```js
{
  name: "code_review",
  description: "Review code changes, diffs, outstanding changes, or modified files...",
  inputSchema: {
    diff_description: "string (required)",
    files:            "string[] (optional) — focus subset",
    instructions:     "string (optional) — user guidance passed to main reviewer",
    checkScope:       "string (optional) — directory to search checks from",
    checkFilter:      "string[] (optional) — only run these check names",
    checksOnly:       "boolean (optional) — skip main reviewer",
    thinking:         '"low" | "high" (default "high")'
  },
  meta: { disableTimeout: true },
  source: "builtin"
}
```

---

## 两路执行（`oGR` → `tGR` + `nGR`）

```
user prompt
    │
    ▼
code_review tool
    │
    ├─► nGR()  = main reviewer  (code-review subagent, Gemini 3.1 Pro)
    │   └─ 产出 <codeReview> XML, 里面若干 <comment>
    │
    └─► tGR()  = check runner for each check discovered
        ├─ Y2R() 发现所有 checks（项目 + 全局，按 name 去重）
        ├─ 过滤 checkFilter（如有）
        └─ 对每个 check 起 codereview-check subagent (Claude Haiku 4.5)
           └─ 产出 <checkResult> XML, 里面若干 <issue>
```

两路通过 `U3(observer1, observer2)` 合并成一个 RxJS observable，主工具返回最终的 `{ main, checks }` 结构。

---

## Subagent Spec 对比

两个 subagent 在 `Ue` 注册表里的定义：

```js
"code-review": {
  key: "code-review",
  displayName: "Code Review",
  model: Y3("GEMINI_3_1_PRO_PREVIEW"),
  includeTools: ["Read", "Grep", "glob", "web_search", "read_web_page", "Bash"],
  allowMcp: false,
  allowToolbox: false,
},

"codereview-check": {
  key: "codereview-check",
  displayName: "Codereview Check",
  model: Y3("CLAUDE_HAIKU_4_5"),
  includeTools: ["Read", "Grep", "glob", "Bash"],
  allowMcp: false,
  allowToolbox: false,
},
```

**对比**：

| 维度 | `code-review` (main) | `codereview-check` (per check) |
|---|---|---|
| 模型 | Gemini 3.1 Pro Preview | Claude Haiku 4.5 |
| 定位 | 综合 reviewer，理解 diff 整体意图 | 规则 matcher，只看一条规则 |
| 工具 | 含 `web_search` / `read_web_page` | 不含网络，纯本地 |
| MCP | ❌ | ❌ |
| Toolbox | ❌ | ❌ |
| 调用次数 | 1 次 per review | N 次 per review（N = 发现的 check 数量） |

**为什么不同模型？** 主 reviewer 需要理解跨文件的 diff 关系（比如"这个函数被改了，它的调用点更新了吗？"），需要高推理。Check runner 只需要"看到就报"，小模型够用，快、便宜、并行容易。

---

## Check 运行的完整 flow

### Step 1: 发现 checks（`Y2R`）

```js
// 入参: filesystem, targetFiles[], workspaceRoot, excludeDirs[], userConfigDir
// 出参: { allChecks: CheckInfo[], checksPerFile: Map<filePath, CheckInfo[]> }
```

- 对每个 `targetFile`，从它所在目录向上走到 `workspaceRoot`，每层扫 `.agents/checks/*.md`
- 再 append 全局位置：`~/.config/amp/checks/*.md` + `~/.config/agents/checks/*.md`
- 按 `name` 去重：先发现的赢（项目级覆盖全局级）
- **如果不同文件各自在不同目录里有自己的 check**，`checksPerFile` 保留每个文件对应的那份列表（虽然当前版本实际跑的是 `allChecks` 合集）

### Step 2: 过滤（`tGR` 主体）

```js
let filteredChecks = checkFilter
  ? allChecks.filter(c => checkFilter.includes(c.name))
  : allChecks;

if (filteredChecks.length === 0) {
  // 短路：无 check 运行，直接返回空数组
  return;
}
```

### Step 3: 并行启动

对每个 check 起一个独立 RxJS 流：

```js
for (let check of filteredChecks) {
  accumulator[check.uri] = {
    check,
    status: { status: "in-progress", turns: [] }
  };
}

// 所有 checks 并发，不序列化
await $v(...filteredChecks.map(c => runSingleCheck(c)));
```

### Step 4: 失败重试（`huT=1`）

```js
// 跑完一轮后：
for (let attempt = 1; attempt <= huT; attempt++) {  // huT = 1
  let failed = filteredChecks.filter(c => acc[c.uri].status.status === "error");
  if (failed.length === 0) break;
  // 重置状态，再跑一遍
  ...
}
```

即**每个 check 最多跑 2 次**（原始 + 1 retry）。仍失败则 `status: "error"` 留在结果里，不阻断其他。

### Step 5: 解析 `<checkResult>`（`bGR`）

check runner 的最终 message 是一段 XML：

```xml
<checkResult>
  <checkName>no-panic</checkName>
  <status>completed</status>
  <filesAnalyzed>3</filesAnalyzed>
  <linesAnalyzed>147</linesAnalyzed>
  <patternsChecked>
    <pattern>panic!() macro calls</pattern>
    <pattern>.unwrap() on Result/Option</pattern>
  </patternsChecked>
  <issues>
    <issue severity="high" file="src/handler.rs" line="42">
      <problem>handle_message(): calls .unwrap() on deserialize result</problem>
      <why>Malformed payload crashes the worker</why>
      <fix>Return Result and log the error upstream</fix>
    </issue>
    <issue severity="medium" file="src/handler.rs" line="89">
      <problem>process_batch(): .expect("never fails") can actually fail</problem>
      <why>Race condition on shutdown</why>
      <fix>Use if let Ok(..) pattern instead</fix>
    </issue>
  </issues>
</checkResult>
```

`bGR` 解析这段 XML 成：

```ts
{
  check: CheckInfo,
  result: {
    name,
    status: "completed" | "error",
    filesAnalyzed?: number,
    linesAnalyzed?: number,
    patternsChecked?: string[],
    issuesFound: number,
  },
  issues: CheckIssue[],
}
```

每个 `<issue>` 被转成：

```ts
{
  check: checkName,
  severity: "critical" | "high" | "medium" | "low",
  file: absPath,     // 自动 join workingDir 如果是相对路径
  line: number,
  problem, why, fix,
  source: checkName,
}
```

---

## 主 reviewer 的 `<codeReview>` 格式（`AGR`）

主 reviewer 按 `aGR` 模板输出：

```xml
<codeReview>
  <comment>
    <filename>/abs/path/to/file.ts</filename>
    <startLine>42</startLine>
    <endLine>45</endLine>
    <severity>high</severity>
    <commentType>bug</commentType>
    <text>Description of the issue / suggestion</text>
    <why>Why this matters</why>
    <fix>How to fix it</fix>
  </comment>
  <comment>...</comment>
</codeReview>
```

`AGR` 解析成 `{ comments: Comment[] }`，`Comment` 的 schema（`sGR`，zod）：

```ts
{
  filename: string,
  startLine: number,
  endLine: number,
  text: string,
  commentType?: "bug" | "suggested_edit" | "compliment" | "non_actionable" | "unknown",
  severity?: "critical" | "high" | "medium" | "low",
  source?: string,   // 主 reviewer 的 comment 这里是 undefined
  why?: string,
  fix?: string,
}
```

---

## 合并 main + checks（`VfT` + `dTR`）

```js
function VfT(checks) {
  // 把所有 check 的 issues 扁平化成和 main comments 同结构
  let R = [];
  for (let t of Object.values(checks)) {
    if (t.status !== "done") continue;
    for (let r of t.result.issues) {
      R.push({
        filename: r.file,
        startLine: r.line ?? 0,
        endLine: r.endLine ?? r.line ?? 0,
        text: r.problem,
        severity: r.severity,
        source: r.source ?? r.check,   // ← 来源 check 名字，UI 里标 [no-panic]
        why: r.why,
        fix: r.fix,
      });
    }
  }
  return R;
}

function dTR(T, R) {
  if (R.checksOnly) {
    // 只跑 checks 的模式
    return { comments: VfT(T.checks), checks: T.checks };
  }
  if (T.main.status !== "done") throw Error("Review did not complete successfully");

  let mainComments = T.main.review.comments.map(e => ({ ...e, source: undefined }));
  let checkComments = VfT(T.checks);

  return {
    comments: [...mainComments, ...checkComments],  // 拼接，不去重
    checks: T.checks,
  };
}
```

**关键发现**：
- `main` 的 comments 和 `check` 的 issues **直接拼接，不做去重**
- `source` 字段区分来源：`undefined` = 主 reviewer；`"no-panic"` = check 名字
- UI 可以按 `source` 分组或过滤显示

---

## 最终 CLI 输出（`T70` + `xTR`）

```js
function xTR(T, R, t) {
  let r = R.comments.filter(e => e.severity !== "low");  // 丢 low
  ...
}
```

**默认过滤掉 `severity: low`**，保证输出噪声低。然后按 filename 分组，每个 comment 用 OSC 8 hyperlink 格式输出：

```
● src/handler.rs
@L42 [no-panic] handle_message(): calls .unwrap() on deserialize result

@L89 [no-panic] process_batch(): .expect("never fails") can actually fail
```

`[no-panic]` 就是 `comment.source`（来自 check 的 issue），主 reviewer 的 comment 这里没有标签。

---

## 状态机与用户反馈

运行中 tool 周期性 emit `yGR` 格式：

```ts
{
  status: "in-progress" | "done" | "error" | "cancelled",
  result: {
    main: { status, review?, toolUses } | { status, toolUses },
    checks: {
      [checkUri]: {
        status: "done"     => { result: CheckResult },
        status: "error"    => { error: string },
        status: "in-progress" => { message: "Running check..." | lastTurnMessage }
      }
    }
  },
  progress: [...turns],
  "~debug": { mainAgent, checks }
}
```

CLI 里看到的 `Running checks...` vs `Reviewing...` 是根据 `checksOnly` 切换的状态行标题。

---

## 工具参数组合的常见玩法

```jsonc
// 1. 完整 review（default）
{ "diff_description": "git diff main" }

// 2. 只跑 checks，不要综合评论（CI 场景）
{ "diff_description": "git diff main", "checksOnly": true }

// 3. 只跑某几个 check
{ "diff_description": "git diff main",
  "checkFilter": ["no-panic", "no-unsafe"] }

// 4. 把 check 搜索范围限制到某个子目录（monorepo）
{ "diff_description": "git diff main",
  "checkScope": "/abs/path/to/crates/my-crate" }

// 5. 只 review 特定文件
{ "diff_description": "git diff HEAD~1",
  "files": ["src/handler.rs", "src/main.rs"] }

// 6. 快速模式（低思考）
{ "diff_description": "git diff", "thinking": "low" }
```

---

## 对 Alva 的启发

**重点：可插拔 code review 框架怎么实施**

### 设计契约

Alva 已有 `Extension` / `Tool` / `Middleware` 三层。code review 这一套最自然的抽象：

```
crates/alva-extension-codereview/      ← 新 crate
├── src/
│   ├── lib.rs                         ← Extension 注册
│   ├── tool.rs                        ← code_review tool spec
│   ├── discovery.rs                   ← 发现 .alva/checks/*.md
│   ├── check_runner.rs                ← 单个 check 的 subagent 驱动
│   ├── main_reviewer.rs               ← 主 reviewer 的 subagent 驱动
│   ├── merge.rs                       ← 合并 main + checks 结果
│   └── format.rs                      ← XML 解析 / 输出
```

### 核心 Rust 骨架

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ─────────────────────────────────────────────────────────────
// 1. Check manifest — 用户写的 .md 文件的内存表示
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckManifest {
    pub name: String,
    pub description: Option<String>,
    pub severity_default: Severity,
    pub tools: Option<Vec<String>>,
    #[serde(skip)]
    pub body: String,
    #[serde(skip)]
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Default for Severity {
    fn default() -> Self { Severity::Medium }
}

// ─────────────────────────────────────────────────────────────
// 2. Discovery — 对应 Amp 的 Y2R/G2R
// ─────────────────────────────────────────────────────────────

pub struct CheckDiscovery {
    pub project_checks_dirname: &'static str,   // ".alva"
    pub checks_subdir: &'static str,            // "checks"
    pub global_dirs: Vec<PathBuf>,              // ~/.config/alva/checks, ~/.config/agents/checks
}

impl CheckDiscovery {
    pub async fn discover(
        &self,
        cwd: &Path,
        workspace_root: &Path,
    ) -> Vec<CheckManifest> {
        let mut seen: indexmap::IndexMap<String, CheckManifest> = Default::default();

        // 项目级：cwd 一路往上走到 workspace_root
        let mut dir = cwd;
        loop {
            self.scan_into(
                &dir.join(self.project_checks_dirname).join(self.checks_subdir),
                &mut seen,
            ).await;
            if dir == workspace_root { break; }
            dir = match dir.parent() { Some(p) => p, None => break };
        }

        // 全局级
        for g in &self.global_dirs {
            self.scan_into(g, &mut seen).await;
        }

        seen.into_values().collect()
    }

    async fn scan_into(
        &self,
        dir: &Path,
        out: &mut indexmap::IndexMap<String, CheckManifest>,
    ) {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else { return };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
            let Ok(content) = tokio::fs::read_to_string(&path).await else { continue };
            let Ok(m) = parse_check(&content, &path) else { continue };
            // 先发现的赢：已有同名就跳过
            out.entry(m.name.clone()).or_insert(m);
        }
    }
}

pub fn parse_check(raw: &str, source: &Path) -> anyhow::Result<CheckManifest> {
    let (fm, body) = split_frontmatter(raw)?;
    let fm: CheckFrontmatter = serde_yaml::from_str(&fm)?;
    let name = fm.name.unwrap_or_else(|| {
        source.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").into()
    });
    Ok(CheckManifest {
        name,
        description: fm.description,
        severity_default: fm.severity_default.unwrap_or_default(),
        tools: fm.tools,
        body: body.to_string(),
        source_path: source.to_path_buf(),
    })
}

// ─────────────────────────────────────────────────────────────
// 3. Tool spec — 对应 Amp 的 XtT
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CodeReviewArgs {
    pub diff_description: String,
    pub files: Option<Vec<String>>,
    pub instructions: Option<String>,
    pub check_scope: Option<PathBuf>,
    pub check_filter: Option<Vec<String>>,
    pub checks_only: Option<bool>,
    pub thinking: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel { Low, High }

// ─────────────────────────────────────────────────────────────
// 4. Orchestrator — 主流程
// ─────────────────────────────────────────────────────────────

pub struct CodeReviewOrchestrator {
    pub discovery: Arc<CheckDiscovery>,
    pub main_model: ModelHandle,     // ReviewerModel，比如 Gemini Pro
    pub check_model: ModelHandle,    // CheckModel，比如 Haiku
    pub tool_service: Arc<dyn ToolService>,
    pub retries: u32,                // 对应 Amp huT = 1
}

pub struct ReviewOutput {
    pub comments: Vec<ReviewComment>,
    pub check_results: Vec<CheckResult>,
}

pub struct ReviewComment {
    pub filename: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub severity: Severity,
    pub comment_type: Option<CommentType>,
    pub source: Option<String>,     // None = main reviewer; Some("no-panic") = check
    pub why: Option<String>,
    pub fix: Option<String>,
}

impl CodeReviewOrchestrator {
    pub async fn run(&self, args: CodeReviewArgs, cwd: &Path, ws_root: &Path)
        -> anyhow::Result<ReviewOutput>
    {
        // 1. 发现 checks
        let checks = self.discovery.discover(
            args.check_scope.as_deref().unwrap_or(cwd),
            ws_root,
        ).await;

        // 2. 过滤
        let filtered: Vec<_> = match &args.check_filter {
            Some(allow) => checks.into_iter()
                .filter(|c| allow.contains(&c.name))
                .collect(),
            None => checks,
        };

        // 3. 并发跑 main + checks
        let (main_fut, check_fut) = tokio::join!(
            self.run_main(&args, cwd),
            self.run_all_checks(&filtered, &args, cwd),
        );

        let main_comments = if args.checks_only.unwrap_or(false) {
            Vec::new()
        } else {
            main_fut?
        };
        let check_results = check_fut?;

        // 4. 合并
        let mut comments = main_comments;
        for cr in &check_results {
            for issue in &cr.issues {
                comments.push(ReviewComment {
                    filename: issue.file.clone(),
                    start_line: issue.line.unwrap_or(0),
                    end_line: issue.line.unwrap_or(0),
                    text: issue.problem.clone(),
                    severity: issue.severity,
                    comment_type: None,
                    source: Some(cr.check_name.clone()),
                    why: issue.why.clone(),
                    fix: issue.fix.clone(),
                });
            }
        }

        Ok(ReviewOutput { comments, check_results })
    }

    async fn run_all_checks(
        &self,
        checks: &[CheckManifest],
        args: &CodeReviewArgs,
        cwd: &Path,
    ) -> anyhow::Result<Vec<CheckResult>> {
        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futs = FuturesUnordered::new();
        for c in checks {
            futs.push(self.run_one_check(c, args, cwd));
        }

        let mut results = Vec::with_capacity(checks.len());
        while let Some(r) = futs.next().await {
            results.push(r?);
        }
        Ok(results)
    }

    async fn run_one_check(
        &self,
        check: &CheckManifest,
        args: &CodeReviewArgs,
        cwd: &Path,
    ) -> anyhow::Result<CheckResult> {
        let prompt = self.build_check_prompt(check, args, cwd);

        // 重试最多 retries 次
        let mut last_err = None;
        for _ in 0..=self.retries {
            match self.check_model.run_subagent(&prompt, self.check_model_spec(check)).await {
                Ok(msg) => return parse_check_result(&check.name, &msg, cwd),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("unreachable")))
    }

    fn build_check_prompt(&self, check: &CheckManifest, args: &CodeReviewArgs, cwd: &Path) -> String {
        // 见 check-skill-format.md，这里严格照着 Amp 的 Q2R 拼
        format!(
r#"# {name} Check

Working directory: {cwd}

{body}

## Files to Review
{files}

## Diff Description
Use this description to gather the full diff using git or bash commands:
{diff}

1. Review the git diff to see what changed
2. Search for patterns described above ONLY in the changed lines (+ lines in diff)
3. Report issues ONLY for code that was added or modified in this diff
4. Do NOT report issues for unchanged/pre-existing code

End your response with:
<checkResult>
<checkName>{name}</checkName>
<status>completed</status>
<filesAnalyzed>NUMBER</filesAnalyzed>
<linesAnalyzed>NUMBER</linesAnalyzed>
<issues>
  <issue severity="{sev}" file="path/to/file.rs" line="LINE">
    <problem>...</problem>
    <why>...</why>
    <fix>...</fix>
  </issue>
</issues>
</checkResult>
"#,
            name = check.name,
            cwd = cwd.display(),
            body = check.body,
            files = format_files(args.files.as_deref()),
            diff = args.diff_description,
            sev = severity_str(check.severity_default),
        )
    }
}
```

### 不抄的点

- **Amp 把 main reviewer 和 check runner 都放在一个 tool 里** —— 这让 tool spec 复杂（6 个可选参数）。Alva 可以考虑拆：`code_review` (主) + `code_checks` (只跑 checks)，用户显式选。
- **`checksOnly: true` 直接发一个假的 `<codeReview></codeReview>` 空响应** —— 这是为了复用同一个合并路径。不优雅，但简单有效。如果 Alva 想做得干净，应该在合并处理里 if/else 分支。
- **不去重** —— main 和 check 可能对同一行同一问题各报一遍。Amp 选择让 UI 处理。如果 Alva 想去重，按 `(filename, startLine, severity)` 哈希一下。

### 交叉引用

- 主 reviewer 的原文 prompt：`../prompts/subagents.md` 的 Code Reviewer 节（`iuT` + `aGR`）
- subagent 机制：`../prompts/subagents.md` 顶部表格
- skill loading：`../skills/SKILL.md`（check 复用同一个 frontmatter 解析器）
