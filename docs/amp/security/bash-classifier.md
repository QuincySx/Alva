# Amp Bash Classifier

**关键发现：Amp 没有 Bash 命令分类器**。搜遍 strings.txt 没有任何 `BashClassifier` / `classify_command` / `is_destructive` / `destructive_patterns` 这类结构。

Amp 完全依赖两种机制替代 classifier：

1. **Prompt 层**：在 system prompt 里反复叮嘱模型"不要执行 destructive 命令"
2. **Rules 层**：让用户自己写 `reject Bash --cmd '*rm -rf*'` 这类规则

这和 Alva 的 `alva-agent-security::BashClassifier`（硬编码 read-only / destructive 列表）路线完全不同。

## Prompt 里的"软 classifier"

这是 Amp 唯一的"分类"机制——**在 system prompt 里告诉模型什么命令算 destructive**。

### 硬规则（executor prompt 里多次出现）

```
**NEVER** use destructive commands like `git reset --hard` or `git checkout --`
unless specifically requested or approved by the user.
**ALWAYS** prefer using non-interactive versions of commands.
```

这条出现在 `prompt line 62547`、`62970` 等多处——Amp 把它重复很多遍加强模型记忆。

### 可逆性原则（pair programming prompt）

```
Consider the reversibility and potential impact of your actions. You are
encouraged to take local, reversible actions like editing files or running
tests freely. For actions that are hard to reverse, affect shared systems,
or could be destructive, ask the user before proceeding.
```

Amp 把"可逆 vs 不可逆"而不是"read vs write"作为决策轴。这个视角其实**比 Alva 的二分更好**——`rm file.tmp` 和 `rm -rf /` 都是"destructive"但影响完全不同；`git commit` 是"destructive"但完全可逆（`git reset` 就回来）。

### 不破坏保护罩

```
When encountering obstacles, do not use destructive actions as a shortcut.
For example, don't bypass safety checks (e.g. --no-verify) or discard
unfamiliar files that may be in-progress work.
```

这条很具体——`--no-verify` 跳 pre-commit hook、`git clean -fd` 误删 in-progress 工作都被点名。

## 并行读取提示（暗示"只读工具"集合）

```
Run independent read-only tools (${ee}, ${vt}, ${P8}, ${yN}) in parallel.
```

从 prompt 占位符字典（`../prompts/placeholder-dictionary.md`）反查，这里的只读工具是：

- `${ee}` = Grep
- `${vt}` = Glob / ListFiles
- `${P8}` = Read
- `${yN}` = mermaid / walkthrough（可能）

**推论**：Amp 在**工具维度**有"read-only"概念（用于并发调度），但**不在 bash 命令维度**做分类。Bash 要么是全块"serial:true"串行执行，要么全靠 rules 判断允许不允许跑。

## 为什么 Amp 不做 bash classifier？

推测的设计理由：

1. **Shell 语法太复杂**：`ls | xargs rm` 看起来像只读（`ls`），实际上是删除。想做对 classifier 需要完整 bash parser + 数据流分析，维护成本高。
2. **Glob 规则足够灵活**：`reject Bash --cmd '*rm -rf*'` 这种通配比硬编码列表好维护。
3. **错误优先 fail-closed**：Amp 在 execute mode 下若 bash 不在 allowlist 就**直接退出 agent** —— 强迫用户显式管理 allowlist 而不是依赖分类器猜测。

这和 Alva 的 `BashClassifier::READ_ONLY_COMMANDS = ["ls", "cat", ...]` 是**两种哲学**：

- **Alva**：提前分类 → 安全命令自动 approve，危险命令 ask/deny
- **Amp**：不分类 → 所有 bash 调用走 rules，要么 allow 要么 reject 要么 ask 要么 delegate

## `ask` 分类器（prompt-based，非 bash-specific）

Amp 有一个**通用**的 yes/no classifier（`v40` 函数），用 Claude 小模型（`by` 常量指向的 model）做二分类。**不是 bash 专用**，用在若干"这个任务完成了没？"之类的场合：

```js
async function v40(T, R, t) {
  // 用 Claude 小模型问 yes/no 问题
  let response = await client.messages.create({
    model: by,
    max_tokens: 300,
    system: "You are a classifier that answers yes/no questions. You must use the provided tool to give your answer with reasoning.",
    messages: [{role: "user", content: T}],
    tools: [{
      name: "answer_question",
      input_schema: {
        type: "object",
        properties: {
          probability_yes: { type: "number", description: "Probability that the answer is yes, 0-1" },
          reasoning: { type: "string", description: "Brief (2-sentence max) explanation" }
        }
      }
    }]
  });

  // 阈值化：>=0.8 -> yes, <0.2 -> no, 否则 uncertain
  let h = response.input.probability_yes ?? 0.5;
  if (h >= 0.8) return {result: "yes", probability: h};
  else if (h < 0.2) return {result: "no", probability: h};
  else return {result: "uncertain", probability: h};
}
```

**不是 permission 系统的一部分**——这是个通用工具。但它说明 Amp 偏好"用 LLM 做模糊判断"而不是"写规则做精确判断"。

理论上 Amp **可以**用 `v40` 问 `"Is this command destructive? <cmd>"`，但反编译里没看到这样的用法。

## 对 Alva 的启发

对比 `alva-agent-security/src/classifier.rs`：

```rust
// Alva 现状
pub struct BashClassifier;
impl BashClassifier {
    pub fn classify(command: &str) -> CommandClassification {
        if is_destructive(trimmed) { return CommandClassification::Destructive; }
        if is_read_only(trimmed) { return CommandClassification::ReadOnly; }
        CommandClassification::Unknown
    }
}
const READ_ONLY_COMMANDS: &[&str] = &["ls", "cat", "head", ...];
```

### 1. **保留 classifier，但降级为"提示"而非"授权"**

Alva 的 classifier 有价值（让 PermissionMode::Auto 能自动放行 `ls`），但**不应该是 deny 的唯一依据**。建议：

- 读 → `ReadOnly`：Auto 模式自动 allow，其他模式走 rules
- 写/执行 → `Unknown`：不因为 classifier 就拒绝；交给 rules + user approval
- 明确危险（`rm -rf /`、`sudo`、`:(){...}:&`） → `Destructive`：默认插入一条 built-in `Deny` rule，**可以被 user rule 覆盖**

### 2. **添加"可逆性"维度**

抄 Amp 的 prompt 哲学。在 `CommandClassification` 里加一个 `Reversible: bool` 字段：

```rust
pub struct CommandClassification {
    pub safety: Safety,          // ReadOnly / Write / Destructive
    pub reversible: Reversibility, // Yes / No / Unknown
}
```

然后在 prompt 层告诉模型：

> 对于 reversible actions 你可以直接做；hard-to-reverse 的要先问 user。

模型 + rules + classifier **三层防护**，比 Alva 现在只有"分类 → 决策"两层更稳。

### 3. **丰富 destructive 识别模式**

Alva 现在的 `is_destructive` 可以参考 Amp prompt 里点名的模式：

- `git reset --hard` / `git checkout --` / `git clean -fd`
- `--no-verify` flag（跳 pre-commit hook）
- `rm -rf` / `rm -r /*`
- `sudo`（破坏权限模型）
- `chmod -R` / `chown -R`（全量改权限）
- `dd` / `mkfs`（低层磁盘）
- `:(){ :|:& };:`（fork bomb）

这些可以写成 regex 列表在 `classifier.rs` 里。

### 4. **考虑：用 LLM 做模糊分类？**

Amp 的 `v40` 结构可以抄，用于**非 bash** 场景——比如判断 user input 是不是在暗示"危险操作"。但对 bash 命令本身，实时调 LLM 太慢、太贵，**不推荐**。Alva 还是应该靠 prompt + rules + 轻量 classifier 组合。

### 5. 对比表

| 维度 | Amp | Alva 现状 | 建议 |
|---|---|---|---|
| Bash 分类器 | 无 | 有（硬编码列表） | **保留**，但降级为"提示" |
| 硬编码 read-only 列表 | 无 | 有（26 条） | 保留 |
| 硬编码 destructive 模式 | 无（靠 prompt） | 有（is_destructive） | 扩展到 Amp prompt 级别 |
| 可逆性维度 | 在 prompt 里 | 无 | **新增** |
| LLM 做 yes/no 分类 | 有（通用，不用于 bash） | 无 | 不用于 bash（性能成本），其他场景可考虑 |
| Prompt 层的软约束 | 强（执行模式反复叮嘱） | 弱 | **加强**：在 system prompt 里抄 Amp 的 destructive 描述 |

**一句话总结**：Amp 靠 prompt + rules + delegate 三件套做 bash 安全，Alva 多了 classifier 这一层。Alva 的 classifier **该留**，但不要让它成为安全的唯一防线——应该和 rules / prompt 协同。
