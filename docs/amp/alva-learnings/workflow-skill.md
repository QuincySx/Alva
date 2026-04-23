# WorkflowSkill 类型 —— Canonical Prompt 固化

> 把 "merge" / "deploy" / "release" 这类高风险动作做成**预定义 workflow**，LLM 只决定触发不决定内容。

---

## 背景

详见 [`../orchestration/canonical-workflows.md`](../orchestration/canonical-workflows.md)。

核心洞察：**高风险动作不让 LLM 即兴写 prompt**。

```
用户：merge it
  │
  ▼
LLM 调 workflow("merge_changes")   ← 只决定触发
  │
  ▼
Server 端发送预存的完整 prompt      ← 内容完全固化
  │
  ▼
执行者按 verbatim prompt 执行
```

好处：
- **确定性**：同一 workflow 永远一样
- **可审计**：log 里一眼看见
- **可版本化**：改 prompt 不用改 LLM
- **防 prompt injection**：用户消息里的"顺便 rm -rf"会被忽略

---

## Alva 的方向

你们 `alva-protocol-skill` 已经是 skill 系统。加一个新的 skill 子类型：

```rust
// alva-protocol-skill/src/workflow.rs

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkflowSkill {
    /// Workflow identifier，作为 tool argument
    pub name: String,                    // e.g. "merge_changes"
    
    /// 描述（给 LLM 看的）
    pub description: String,
    
    /// 触发词白名单（显式匹配才触发）
    pub trigger_words: Vec<String>,       // e.g. ["merge", "ship it", "merge changes"]
    
    /// 反触发词黑名单（这些词不算触发）
    pub anti_trigger_words: Vec<String>,  // e.g. ["do it", "go ahead", "sounds good"]
    
    /// Canonical prompt（verbatim 发给执行者）
    pub canonical_prompt: String,
    
    /// 触发前的 safety check
    pub pre_checks: Vec<PreCheck>,
    
    /// 是否需要显式权限（HITL）
    pub requires_permission: bool,
    
    /// 如果需要权限，用哪个 permission kind
    pub permission_kind: Option<String>,  // e.g. "destructive:git:merge"
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PreCheck {
    /// 检查目标 thread 不在忙碌
    ThreadNotBusy,
    
    /// 检查 git worktree 干净
    GitWorktreeClean,
    
    /// 检查测试通过
    TestsPassing,
    
    /// 自定义 shell 命令 exit 0
    ShellCommand { cmd: String },
    
    /// 自定义（插件提供的）
    Custom { name: String },
}
```

---

## WorkflowSkill 文件格式

存在 `.agents/workflows/*.md`（或内置 `builtin:///workflows/`）：

```markdown
---
kind: workflow
name: merge_changes
description: Merge the current feature branch into main after verification
trigger_words:
  - merge
  - merge it
  - merge changes
  - ship it
  - let's ship it
anti_trigger_words:
  - do it
  - go ahead
  - sounds good
  - make that change
pre_checks:
  - ThreadNotBusy
  - GitWorktreeClean
  - TestsPassing
requires_permission: true
permission_kind: destructive:git:merge
---

You are about to merge changes. Follow these exact steps:

1. Verify you are on the correct feature branch (`git branch --show-current`)
2. Ensure all tests pass: run the project's test command
3. Rebase onto the target branch (usually main):
   ```
   git fetch origin
   git rebase origin/main
   ```
   If there are conflicts, STOP and report them.
4. Push the rebased branch: `git push --force-with-lease`
5. If the project uses PR-based merge:
   - Create a PR using `gh pr create` if one doesn't exist
   - Wait for CI to pass
   - Merge with `gh pr merge --squash --delete-branch`
6. If the project uses direct merge:
   - `git checkout main && git merge --ff-only <feature-branch> && git push`
7. Report the result, including commit SHA and any warnings.

Do NOT:
- Force push to main
- Skip CI checks
- Delete branches without confirming merge succeeded
- Perform destructive operations without explicit need
```

frontmatter 用 `kind: workflow` 区别于普通 skill。body 就是 canonical prompt。

---

## `workflow` 工具

```rust
// alva-agent-extension-builtin/src/tools/workflow.rs

pub struct WorkflowTool {
    skill_service: Arc<dyn SkillService>,
}

impl Tool for WorkflowTool {
    fn name(&self) -> &str { "workflow" }
    
    fn description(&self) -> String {
        // 动态生成：列出所有可用 workflow + 其 trigger_words
        let workflows = self.skill_service.get_workflow_skills();
        let mut desc = String::from(
            "Trigger a predefined canonical workflow. The workflow's exact prompt will 
be sent verbatim to the execution context. Use this for high-stakes actions 
where consistency is more important than flexibility.\n\n## Available workflows:\n"
        );
        for wf in workflows {
            desc.push_str(&format!("\n### {}\n{}\n", wf.name, wf.description));
            desc.push_str(&format!("Trigger on: {}\n", wf.trigger_words.join(", ")));
            desc.push_str(&format!("Do NOT trigger on: {}\n", wf.anti_trigger_words.join(", ")));
        }
        desc
    }
    
    fn input_schema(&self) -> JsonSchema {
        let workflow_names: Vec<String> = self.skill_service
            .get_workflow_skills()
            .iter()
            .map(|w| w.name.clone())
            .collect();
        
        json_schema!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "enum": workflow_names
                },
                "target": {
                    "type": "string",
                    "description": "Optional: target thread ID for orchestrator mode"
                }
            },
            "required": ["name"]
        })
    }
    
    async fn call(&self, args: ToolInput, ctx: ToolContext) -> ToolResult {
        let name = args.get_string("name").ok_or_else(|| anyhow!("name required"))?;
        
        let workflow = self.skill_service
            .get_workflow(&name)
            .ok_or_else(|| anyhow!("workflow not found: {}", name))?;
        
        // 1. Run pre-checks
        for check in &workflow.pre_checks {
            let result = self.run_pre_check(check, &ctx).await?;
            if !result.passed {
                return Ok(ToolResult::Error(format!(
                    "Pre-check failed: {} ({})",
                    check.name(), result.reason
                )));
            }
        }
        
        // 2. Request permission if needed
        if workflow.requires_permission {
            let kind = workflow.permission_kind.as_deref().unwrap_or("workflow");
            let granted = ctx.permission_manager.request(kind, json!({
                "workflow": name,
                "description": workflow.description
            })).await?;
            if !granted {
                return Ok(ToolResult::RejectedByUser);
            }
        }
        
        // 3. Inject canonical_prompt
        let target = args.get_string("target");
        if let Some(target_thread_id) = target {
            // Orchestrator mode: send to another thread
            ctx.thread_service.append_message(
                &target_thread_id,
                Message::user(workflow.canonical_prompt.clone()),
            ).await?;
            Ok(ToolResult::Done(json!({
                "sent_to_thread": target_thread_id,
                "workflow": name
            })))
        } else {
            // Same-thread mode: append to current thread
            ctx.thread_service.append_message(
                &ctx.thread_id,
                Message::user(workflow.canonical_prompt.clone()),
            ).await?;
            Ok(ToolResult::Done(json!({
                "continued_in_current_thread": true,
                "workflow": name
            })))
        }
    }
}
```

---

## System Prompt 中的触发规则（自动生成）

从 workflow skills 列表自动生成 prompt 段：

```
# Canonical Workflows

You have access to predefined workflows for high-stakes actions. When 
triggered, the `workflow` tool sends the exact canonical prompt verbatim. 
Do NOT compose freeform messages for these actions.

## merge_changes

Description: Merge the current feature branch into main after verification.

Trigger on: "merge", "merge it", "merge changes", "ship it", "let's ship it"

Do NOT trigger on: "do it", "go ahead", "sounds good", "make that change"

Before triggering: verify thread is not busy, git worktree is clean, 
tests pass.

This action requires user permission (destructive:git:merge).

## deploy_production

...
```

**关键技巧**：把 `anti_trigger_words` 写进 prompt。LLM 看到白名单 + 黑名单双写，判断更准确。

---

## 集成点

- 新 crate：`alva-protocol-skill::workflow` (或放 `alva-protocol-skill` 里作子模块)
- 新 tool：`alva-agent-extension-builtin::tools::workflow`
- 修改 system prompt 装配：从 skill service 拿 workflow skills → 生成触发规则段
- 修改 `SkillsExtension`：识别 `kind: workflow` 的 frontmatter
- 新 CLI：`alva workflows list` / `alva workflows run <name>`

---

## 风险

### 1. 过度规范化

workflow 模板写死了所有步骤，但代码库各异。**建议**：workflow 留 placeholder，让执行者在模板基础上适配。

### 2. 权限模型变复杂

每个 workflow 可能有不同权限需求。**建议**：用 `permission_kind` 字段统一接入现有 `PermissionManager`，不单独发明权限体系。

### 3. LLM 忽略触发规则

尽管 prompt 写清楚，LLM 偶尔会 "freeform merge"。**兜底**：在 `workflow` tool description 里强调 "for any merge/deploy/release request you MUST use this tool rather than composing your own prompt"。

---

## 优先级

**高**。特别是：

- 你们有 `PlanModeMiddleware` + `PermissionManager` —— 补这个扩展协同
- 用户在生产里让 agent 跑 `git push` 的事故可以减少
- 对 agent 产品的信任度提升显著

---

## 简化版（MVP）

如果不想一次性上完整设计，先做最简版：

1. 在 `~/.alva/workflows/` 存 `.md` 文件（frontmatter + body）
2. 加载为特殊类型的 "skill"
3. `workflow` tool 只做：收 name → 读对应文件 → append 到 thread
4. 不做 pre-checks / permission / anti-triggers（先观察）

这样一天可以上线。之后按使用数据加复杂度。
