# Amp Sensitive Files & Secret Handling

Amp 对 secret（API tokens、凭证、`.env` 文件等）的处理分**两层**：

1. **Prompt 层约束**：system prompt 明确告诉模型"不许读写 secret 文件"
2. **Runtime 层 redaction**：低层文件 pipeline 把 secret 替换成 `[REDACTED:xxx]` 占位，**模型根本拿不到原文**

**关键发现**：redaction 不在 prompt 里做——是**读文件时就脱敏**，模型看到的 Read/Grep 结果已经是替换过的。这比 Alva 现状（仅靠 `SensitivePathFilter` 拒绝访问）更深一层。

## 1. Prompt 层：`<secret-file-instruction>`

从 line 62412 提取的原文（触发条件：`errorCode === "reading-secret-file"`）：

```xml
<secret-file-instruction>
You MUST never read or modify secret files in any way, including by using cat,
sed, echo, or rm through the Bash tool.
Instead, ask the user to provide the information you need to complete the task,
or ask the user to manually edit the secret file.
</secret-file-instruction>
```

反编译代码（`wBT` 函数）：

```js
function wBT(T) {
  let R = [`<error>${T.message}</error>`];
  switch (T.errorCode) {
    case "reading-secret-file":
      R.push($t`<secret-file-instruction>
You MUST never read or modify secret files in any way, including by using cat, sed, echo, or rm through the Bash tool.
Instead, ask the user to provide the information you need to complete the task, or ask the user to manually edit the secret file.
</secret-file-instruction>`);
  }
  return R.join(...);
}
```

**触发时机**：当工具返回 `errorCode: "reading-secret-file"`，把这段 instruction **直接附加到工具错误结果里返回给模型**。这样模型下次不会重试同样的路径，也不会想"那我用 sed 读吧"——因为 instruction 显式点名 `cat, sed, echo, rm`。

这是**即时强化**设计：不在 system prompt 常驻（节省 token），只在触发时注入。

## 2. Runtime 层：`[REDACTED:xxx]` 占位符

从 line 62959 / 64263 提取的 prompt 原文：

```
- Redaction markers like [REDACTED:amp-token] or [REDACTED:github-pat] indicate
  the original file or message contained a secret which has been redacted by a
  low-level security system. Take care when handling such data, as the original
  file will still contain the secret which you do not have access to. Ensure you
  do not overwrite secrets with a redaction marker, and do not use redaction
  markers as context when using tools like ${dr} as they will not match the file.
```

`${dr}` 从占位符字典大概是 Edit 或 StringReplace 工具（因为"匹配文件内容做替换"）。

### 关键信息拆解

1. **"low-level security system"**：redaction 是**底层管道**做的，不是 prompt 层装的样子。文件读取 / stdout 抓取管道里就已经替换了。

2. **命名规范 `[REDACTED:<kind>]`**：
   - `[REDACTED:amp-token]` — Amp 自己的 API token
   - `[REDACTED:github-pat]` — GitHub personal access token
   - 推测还有 `[REDACTED:aws-key]` 之类（AWS SDK strings 也出现 `accessKeyId: "[REDACTED]"` / `secretAccessKey: "[REDACTED]"` 在 line 39192-39194，虽然那是 AWS SDK 自己 log 脱敏，但命名一致说明 Amp 有统一规范）

3. **双向保护**：
   - **读取时脱敏**：模型收到 `[REDACTED:github-pat]`，不知道原 token
   - **写入时保护**：prompt 明确说"不要用 redaction marker 覆盖 secret"——因为如果模型做 `Edit(file, old="actual_token", new="[REDACTED:github-pat]")` 类操作，**会把真 token 覆盖成占位符，数据丢失**

4. **搜索的 corner case**：模型收到的 Read 内容是 `[REDACTED:...]`，但如果用 `Grep` 搜同一文件想定位 secret 上下文，**grep 结果也是占位符**——prompt 提醒"不要用 marker 作为 tool 参数"（`dr` 工具），因为真文件里是原 token、marker 匹配不到。

### 这个设计的巧妙之处

- **模型无法泄露 secret**：LLM 看不到真 token → 没法在 response 里打印出来 → 没法通过 API 外泄
- **但模型仍能帮你修代码**：知道"这里有个 github-pat"，可以写代码 `process.env.GITHUB_PAT` 去读，而不是硬编码
- **透明给用户**：文件里还是原 token，Amp 运行完不会破坏用户的配置

**这是顶级 defense-in-depth 设计**——Alva 可以直接抄。

## 3. 错误码 `reading-secret-file`

从 `wBT` 函数的 switch case 反推，Amp 的文件工具（Read、Edit、Bash 读取 etc.）能返回结构化错误：

```
{
  errorCode: "reading-secret-file",
  message: "Cannot read .env (detected as secret file)"
}
```

客户端把 errorCode 映射到对应 instruction（当前只看到 `reading-secret-file` 这一个 case，可能还有其他——反编译看不全）。

这是"结构化错误 → 针对性 prompt 注入"模式，非常好的 UX。对比：Alva 现在的 `SensitivePathFilter` 只返回 `Err(anyhow!("sensitive path"))`，模型只看到"失败了"，没有 instruction 告诉它怎么处理。

## 4. 与 `SensitivePathFilter` 的关系

Amp 没 `SensitivePathFilter` 这种**硬编码路径列表**的公共结构（反编译里没看到）。推测 Amp 是：

1. **不在路径层拦截**，允许工具访问 `.env`、`~/.ssh/id_rsa` 等
2. **内容层做 redaction**：读到文件后扫内容、识别 secret pattern、替换成 `[REDACTED:<kind>]`
3. **发现 secret 了才报 `reading-secret-file`**（可能是 .env 等特定文件名的硬规则 + 内容 pattern 扫描的组合）

这和 Alva 的"**拒绝访问**"策略相反——Alva 根本不让模型读 `.env`，Amp 让读但只给脱敏版本。

**两种路线各有千秋**：

| 维度 | "拒绝访问" (Alva) | "读但脱敏" (Amp) |
|---|---|---|
| 安全性 | 模型完全看不到文件 | 模型看到文件但看不到 secret |
| 模型可用性 | 模型不知道有这个文件 | 模型能看到非 secret 部分 |
| 能力边界 | 模型不知道 `.env` 里有啥变量 | 模型能 `grep VAR_NAME .env` 看有哪些变量 |
| 实现复杂度 | 低（路径匹配） | 高（需要内容扫描 + regex detector） |
| 误报率 | 高（`.env` 里的非 secret 注释也读不到） | 低（只脱敏 pattern 匹配的部分） |

## 5. Prompt 里的 file link 示例

Amp prompt 里出现 file link 示例时**特意避开**真实 secret 路径：

```markdown
- [Configure the JWT secret](file:///Users/alice/project/config/auth.js#L15-L23)
  in the configuration file
```

这里用 `alice/project/config/auth.js`（虚构用户），**不是用真环境里的路径**——防止在 few-shot example 里泄露用户隐私。细节但体现谨慎。

## 6. 其他 secret 相关观察

### `dangerouslyAllowBrowser` 选项

第三方 SDK（Anthropic、OpenAI）的 browser-mode 警告（line 62385, 62437）：

```
you can set the `dangerouslyAllowBrowser` option to `true`, e.g.,
new Anthropic({ apiKey, dangerouslyAllowBrowser: true });
```

这不是 Amp 自己的代码，是 SDK 警告——**在浏览器里嵌 API key 会让 key 暴露**。Amp 内部肯定是 false。

### AWS SDK 的 log 脱敏

line 39192-39194：

```
accessKeyId: "[REDACTED]"
secretAccessKey: "[REDACTED]"
sessionToken: "[REDACTED]"
```

这是 AWS SDK 自己在 log 层的脱敏（标准行为）。和 Amp `[REDACTED:<kind>]` 格式略不同（没有 `:<kind>`），说明 Amp 的 redaction 是**自己的独立 pipeline**，不是靠第三方 SDK。

## 对 Alva 的启发

对比 `alva-agent-security/src/sensitive_paths.rs`：

```rust
pub struct SensitivePathFilter {
    denied_dirs: Vec<PathBuf>,          // ~/.ssh, ~/.aws, ~/.gnupg, ...
    denied_extensions: Vec<String>,      // .pem, .key, .p12, ...
    denied_filenames: Vec<String>,       // .env, .env.local, credentials.json, ...
    denied_patterns: Vec<Regex>,         // catch-all
}
```

### 1. **抄 Amp 的 `[REDACTED:<kind>]` 占位符机制**

这是最值得抄的。实现思路：

```rust
// crates/alva-agent-security/src/redactor.rs (新)
pub struct SecretRedactor {
    detectors: Vec<Box<dyn SecretDetector>>,
}

pub trait SecretDetector {
    fn kind(&self) -> &'static str;  // "github-pat", "aws-key", "slack-token", ...
    fn detect(&self, text: &str) -> Vec<(Range<usize>, &'static str)>;  // ranges + kind
}

impl SecretRedactor {
    pub fn redact(&self, text: &str) -> String {
        // 对每个 detector 找出 ranges，替换成 [REDACTED:<kind>]
    }
}

// 在 Read / Grep / Bash stdout 的管道里调用
let content = fs::read_to_string(path)?;
let redacted = redactor.redact(&content);
// 返回 redacted 给模型
```

预置 detector：

- GitHub PAT: `ghp_[A-Za-z0-9]{36}`, `github_pat_[A-Za-z0-9_]{80,}`
- AWS key: `AKIA[0-9A-Z]{16}`（access key ID）
- Generic API token: `sk-[A-Za-z0-9]{48}` (OpenAI 风格), `sk-ant-[A-Za-z0-9]{95}` (Anthropic)
- JWT: `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`
- Private key block: `-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----` → 整块替换
- Slack / Discord / Stripe / GCP / Azure 各家 token 格式（借鉴 GitGuardian / trufflehog 的 detector list）

### 2. **双轨策略：重要路径拒访问 + 其他路径脱敏**

保留 Alva 现有 `SensitivePathFilter`（对 `~/.ssh/id_rsa`、`.gpg`、`.p12` 这些**一定是 secret** 的文件保持"拒绝读取"），**额外**对 `.env`、`config.json` 这些**可能含 secret** 的文件走 redactor。

层级：

```
路径访问 → SensitivePathFilter（拒或放）
   ↓ 放
文件内容 → SecretRedactor（扫描 + 替换）
   ↓
返回给模型（已脱敏）
```

### 3. **抄 Amp 的错误码 + prompt 注入模式**

Alva 目前 `SensitivePathFilter` 拦截只返 Error，模型看不到指导。应该：

```rust
// 新增 enum
pub enum SecurityErrorCode {
    ReadingSecretFile,
    WritingToSensitivePath,
    BypassingSafetyCheck,  // --no-verify 等
}

// 对接 prompt injection middleware
impl ToolErrorMiddleware {
    fn augment_error(&self, err: SecurityError) -> ToolResult {
        let instruction = match err.code {
            SecurityErrorCode::ReadingSecretFile => include_str!("prompts/secret-file.md"),
            // ...
        };
        ToolResult::error_with_instruction(err.message, instruction)
    }
}
```

`prompts/secret-file.md` 里写和 Amp 一样的 `<secret-file-instruction>`——可以直接抄。

### 4. **Prompt 里加 `[REDACTED:...]` 使用说明**

在 Alva 的 system prompt 里加一段（抄 Amp 的措辞）：

```
- Redaction markers like [REDACTED:amp-token] or [REDACTED:github-pat]
  indicate the original file or message contained a secret which has been
  redacted by a low-level security system. Take care when handling such data,
  as the original file will still contain the secret which you do not have
  access to. Ensure you do not overwrite secrets with a redaction marker, and
  do not use redaction markers as context when using tools that do string
  matching (they will not match the file).
```

**一定要同时强调 "不要用 redaction marker 做 tool 参数"**——这是 Amp 的 prompt 里点名的 corner case，不说模型可能会踩（`Edit(old="[REDACTED:github-pat]", new="...")` 这种调用会全部失败）。

### 5. **Secret 写入监控**

Amp 的 prompt 有句话：`Ensure you do not overwrite secrets with a redaction marker`。这暗示 Amp 还**监控写入**——如果模型试图把 `[REDACTED:xxx]` 写回文件（会破坏用户配置），应当拦截。

Alva 的 Write/Edit tool middleware 应当：

1. 检查写入内容是否包含 `[REDACTED:...]` marker
2. 如果有，和源文件对比，拒绝"用 marker 覆盖真 secret"的操作
3. 返回错误 + `errorCode: writing-redaction-marker` + 对应 instruction

### 6. 对比表

| 维度 | Amp | Alva 现状 | 建议 |
|---|---|---|---|
| 路径拒访 | 无（或很少） | `SensitivePathFilter` | 保留 |
| 内容 redaction | 有（`[REDACTED:<kind>]`） | **无** | **抄** |
| 错误码 + 针对性 instruction | 有（`reading-secret-file`） | 无 | **抄** |
| Prompt 教育模型 | 有（两处注入） | 无 | **抄** |
| 写入时防回写 marker | 有 | 无 | **加** |
| Secret detector 列表 | 不可见（内置） | 无 | **新建** |

**实施优先级**（如果选一个功能抄）：**SecretRedactor + prompt instruction 打包一起做**。这是 Alva 最缺、收益最高的安全增强。

## 交叉引用

- Alva 现有路径保护：`/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-security/src/sensitive_paths.rs`
- Amp 的 Rules DSL（怎么手动 reject 工具访问路径）：`./permission-model.md`
- Amp 的 classifier 策略（为什么不做 bash 命令分类）：`./bash-classifier.md`
