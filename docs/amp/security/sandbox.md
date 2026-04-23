# Amp Sandbox

**核心发现：Amp 没有 OS 级 sandbox**。这是本次反编译最意外的结果之一。

在 66622 行 strings.txt 里搜索：

- `Seatbelt` → 0 hit
- `sandbox-exec` → 0 hit
- `sandbox-preview` → 0 hit
- `.agents/preview` → 0 hit（Claude Code 风格）
- `SandboxConfig` → 0 hit
- `SandboxProfile` → 0 hit

唯一的 `sandbox` 匹配：

- `NodeVMSpecialSandbox`（WebKit 内部常量，和 Amp 无关）
- `The path to the directory to write sampling profiler output to... unless the path is in the sandbox.`（WebKit profiling 注释）

**结论**：Amp 不做进程隔离、不写 `.sb` profile、不用 macOS Seatbelt、不用 Linux seccomp/namespaces。

## Amp 的替代方案：Rules + User Approval + Kill Switch

既然没有 sandbox，Amp 怎么防止 agent 搞破坏？靠**四件套**：

### 1. Permission Rules（软隔离）

见 `./permission-model.md`。用户可以写：

```
reject Bash --cmd '*rm -rf*'
reject Bash --cmd '*sudo*'
reject Bash --cmd 'curl*|*sh*'
```

**但这依赖用户自己写全**——规则不全就有漏洞。Amp 内置规则 `$N` 存在，但具体内容没法从反编译看出，推测很小。

### 2. User Approval（交互兜底）

TUI 模式下每个未 allowlist 的 tool call 都会弹确认框。用户按 `y/n` 或选 "always allow"。

### 3. Execute Mode 硬退出（failsafe）

非交互模式（无 TTY / 输出被 pipe）下碰到需要 consent 的工具**直接让 agent 退出**：

```js
TT.warn("Tools require user consent - exiting execute mode", {
  blockedTools: E.map((B) => ({name: B.name, id: B.id}))
});
process.stderr.write(`Error: The ${tool} tool tried to run a command that isn't allowlisted.
Rerun with --dangerously-allow-all to bypass, or add to the command allowlist in permissions`);
process.exit();
```

这是 **fail-closed** 设计——默认拒绝，不会"默默跑"。

### 4. `--dangerously-allow-all` Kill Switch（用户自担）

启动时加 `--dangerously-allow-all` 绕过所有 `ask` 规则。description 里明说"agent will execute all commands without asking"——**把风险转嫁给用户**。

## 文件系统访问控制

虽然没进程 sandbox，Amp 在**文件操作 API 层**有一点点隔离。`AVT` 函数（line 63676 附近）：

```js
function AVT(T, R) {
  let t = NR.parse(T);
  if (t.scheme !== "file") throw new B_("INVALID_URI", "Only file:// URIs are supported");
  if (!t.path.startsWith("/")) throw new B_("INVALID_URI", "File URI path must be absolute");
  if (t.path.split("/").some((i) => i === ".."))
    throw new B_("ACCESS_DENIED", "File URI resolves outside workspace root");
  let r = gI.posix.normalize(t.path).replace(/^\/+/u, "");
  if (r === ".." || r.startsWith("../"))
    throw new B_("ACCESS_DENIED", "File URI resolves outside workspace root");
  let e = gI.resolve(R);  // R = workspaceRoot
  let h = r.length > 0 ? gI.resolve(e, r) : e;
  let a = gI.relative(e, h);
  if (a.startsWith("..") || gI.isAbsolute(a))
    throw new B_("ACCESS_DENIED", "File URI resolves outside workspace root");
  return h;
}
```

**功能**：把 `file://` URI 规范化到 workspace root 内，拒绝 `..` 路径遍历。

错误码：`INVALID_URI`、`ACCESS_DENIED`。

**限制**：

- **只对 DTW (Distributed Thread Worker) 远程执行场景生效**（这个函数在 WebSocket worker 路径里）——本地 CLI 的 Bash/Edit 工具不经过这里
- **只检查路径语法**，不检查符号链接（symlink 可以逃出 workspace）

这不是真 sandbox，更像 "web API 参数校验"。

## 为什么 Amp 不做 sandbox？

推测理由：

### 1. **Bun 编译二进制没法简单嵌 sandbox**

Amp 是 Bun 把整个 JS 应用打包成单 Mach-O。要调 macOS Seatbelt 得 fork 出子进程 `sandbox-exec`，但那样每个 Bash tool call 都有 ~30ms 额外开销。

### 2. **Cross-platform 一致性**

macOS 有 Seatbelt、Linux 有 seccomp/landlock/namespaces、Windows 基本没有。做 cross-platform sandbox 工作量巨大——Amp 选择"一个都不做"而不是"某些平台做"。

### 3. **Web 模式（ampcode.com）已经有隔离**

Amp 在 web 模式下把 execution 放到 Cloudflare Workers（DTW，见 `../remote-runtime/`）上跑，那里是 V8 isolate + WASM sandbox + 完全隔离的 workspace。**远端模式天然有沙箱**，本地 CLI 模式就靠用户自己负责。

### 4. **Amp 目标用户是开发者**

和 Claude Code 一样，Amp 假设用户**懂自己在做什么**，会检查 permission 弹窗。对 `rm -rf` 这种明显危险操作，靠 user approval + prompt 警告就够。

## 对比：Claude Code / Codex / Alva

| 项目 | Sandbox 策略 |
|---|---|
| **Amp** | 无 OS sandbox；rules + user approval + execute mode fail-closed + `--dangerously-allow-all` |
| **Claude Code** | macOS Seatbelt profile（`.agents/preview` 机制）+ permission prompts |
| **Codex** | Seatbelt + Docker container（Codex Cloud）+ file-level path checks |
| **Alva** | `SandboxConfig` + `SandboxMode::{RestrictiveOpen, RestrictiveClosed, RestrictiveProxied, PermissiveOpen}` + macOS Seatbelt profile builder |

**Alva 在这方面比 Amp 做得好**——有真正的 OS 级隔离能力。

## 对 Alva 的启发

对比 `alva-agent-security/src/sandbox.rs`：

### 1. **保留并加强 Alva 的 sandbox**

Alva 的 `SandboxConfig` + 四档 `SandboxMode` + Seatbelt profile 是**好的设计**，比 Amp 强。不要因为 Amp 没做就砍掉这块——这是 Alva 的**差异化优势**。

### 2. 抄 Amp 的：**Execute Mode Fail-Closed**

Alva 目前没有明确的"execute mode"概念。建议加：

```rust
// alva-app-cli 里
pub enum RunMode {
    Interactive,   // 默认：有 TTY，允许弹 approval UI
    Execute,       // --execute 或 stdin not TTY：遇到 ask 规则直接 fail
}

// 对接到 SecurityGuard
impl SecurityGuard {
    pub fn check(&self, decision: RuleDecision) -> Result<Allowed, Blocked> {
        match (self.mode, decision) {
            (RunMode::Execute, RuleDecision::Ask) => Err(Blocked::NeedsApprovalInExecuteMode),
            _ => /* normal flow */,
        }
    }
}
```

这让 CI 场景（`alva --execute` 或 piped input）**明确 fail-closed**，不会因为"默默等用户输入"卡死。

### 3. 抄 Amp 的：**文件 URI 路径规范化**

`AVT` 函数那套（拒绝 `..`、规范化到 workspace root、用 `path.relative` 验证）**值得抄**。

`alva-agent-security::AuthorizedRoots` 现在应该有类似逻辑？但要**严格**起见，加上：

- 拒绝所有绝对路径（除非在 `authorized_roots` 里）
- 拒绝所有 `..` 序列（即使规范化后回到 root 内也拒绝——防 `/allowed/../../../etc/passwd` 规范化成 `/etc/passwd` 这类 corner case）
- **考虑 symlink 解析**：`realpath(path)` 后再比对 `authorized_roots`，防符号链接逃逸

### 4. 不抄 Amp 的：**`--dangerously-allow-all`**

Alva 应当**没有**这种全局 kill switch。理由：

- 一旦加上，新手用户会当"方便模式"用，安全防护失效
- 要绕过限制应该走规则（`alva permissions add allow Bash '*'`），这样**有记录**
- 实在要"什么都允许"可以 `PermissionMode::Bypass` + sandbox 组合——Alva 已经有这个，**强迫 sandbox 配合使用**（从 `only_bypass_requires_sandbox` 测试看得出这是刻意设计）

Alva 的 `PermissionMode::Bypass + requires_sandbox=true` 比 Amp 的 `--dangerously-allow-all` 安全得多——**保持这个设计**。

### 5. **Sandbox vs Rules 关系要清晰**

Amp 没 sandbox 所以没这问题，Alva 两者并存，要想清楚**评估顺序**：

1. **Rules 先评**：明确 `Deny` 直接拒绝（连 sandbox 都不启动）
2. **Rules 允许或 Ask 后** → 用户 approve → 进 Sandbox 跑
3. **Bypass 模式**：Rules 全 skip → 但 **sandbox 必须启用**（`requires_sandbox=true`）

这样 sandbox 是"对允许运行的东西做二次封装"——和 rules 互补，不冲突。

### 6. 对比表

| 维度 | Amp | Alva 现状 | 建议 |
|---|---|---|---|
| OS sandbox | **无** | 有（Seatbelt） | **保留 Alva** |
| Sandbox profiles | 无 | 4 档 | 保留 |
| Execute mode fail-closed | **有** | 无 | **抄** |
| 路径规范化 | 有（DTW only） | 有（authorized_roots） | 加 symlink realpath |
| `--dangerously-allow-all` | 有 | 无 | **不抄** |
| Bypass 模式 | 无（靠 flag） | 有（requires sandbox） | 保留 |

**一句话总结**：Amp 在 sandbox 上走的是"全量依赖用户"路线，Alva 走的是"提供工具、默认安全"路线。这是 Alva 比 Amp 强的地方之一，不要抄、不要砍。从 Amp 该抄的是 **execute mode fail-closed** 这个简单但有效的 failsafe。
