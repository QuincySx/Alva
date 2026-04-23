---
name: amp-git-integration
description: Amp 对 git 的深度集成 —— 自动注入 Amp-Thread-ID / Co-authored-by trailer、服务端托管的 commit GPG 签名、GitHub token 代理的 credential helper、thread 级 diff snapshot。想懂 agent 怎么把 "thread ↔ git commit" 双向追溯、或 agent 在沙箱里怎么 push 到 GitHub 时加载。
trigger_words:
  - git integration
  - auto commit
  - git commit ampThread
  - Amp-Thread-ID
  - Co-authored-by Amp
  - commit signing
  - signCommit
  - git-credential-helper
  - credential helper
  - diffStat
  - git trailer
  - gpg.program amp
  - sandbox git
  - thread commit traceability
---

# Amp Git Integration

Amp 的 git 集成不是"agent 调 `git commit` 工具"这种表层使用 —— 它在 **Bash tool 命令解析层**、**CLI 的 hidden 子命令层**、**服务端 `/api/internal`** 三处深度挂钩，让 git 本身变成 agent 审计链的一部分。

## 反直觉的 3 个事实

1. **Amp 从不主动帮用户 commit**。System prompt 明确说 "Do not commit or push without explicit consent"。真正的自动化在 **trailer 注入** —— 只要用户（或 agent）说 "git commit ...", Amp 在 Bash 执行前改写命令，塞入 `Amp-Thread-ID` trailer。
2. **Commit 签名不走本地 GPG**。沙箱里 `gpg.program = amp sign-commit`，commit object 通过 `POST /api/internal signCommit` 回服务端，服务端托管 key 给出签名，输出 `[GNUPG:] SIG_CREATED ...` 让 git 认。
3. **GitHub 凭证不存 token**。沙箱里 git 的 credential.helper = `amp git-credential-helper`，走 protocol=https 协议与 amp login 的 apiKey 换一个 `x-access-token`，不在 disk 上留痕。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./auto-commit.md` | `git.commit.ampThread.enabled` + `git.commit.coauthor.enabled` 两个 setting、Bash 命令改写器 `$2R` 的 AST 修改流程、trailer 的 replace / addIfDifferent 策略 | 想懂 "commit 里怎么追溯到 thread" 或想抄 trailer 注入 |
| `./commit-signing.md` | `amp sign-commit` CLI + `signCommit` internal API + SIG_CREATED 协议伪装 | 想懂沙箱里怎么做 verified commit |
| `./credentials-helper.md` | `amp git-credential-helper` 的 protocol=https / username=x-access-token 协议实现 + apiKey → GitHub install token 的服务端换取 | 想懂沙箱里怎么 push 到 GitHub 而不存 PAT |
| `./diff-stats.md` | `uVT` thread-level snapshot + `pVT` status parser + `V_0` diffStat 算法 + `X_0` sha256 摘要 | 想懂 diffStat / changeType / aheadCount 怎么算 |

## 常见问答

**Q：agent 会自动 commit 吗？**  
A：**不会**。Amp 的 system prompt 明令 "Do not commit or push without explicit consent"。但如果用户批准了一次 `git commit`，Amp 会**改写命令本体**，追加 `Amp-Thread-ID` / `Co-authored-by: Amp` trailer。参见 `auto-commit.md`。

**Q：怎么在 commit 里追溯 thread？**  
A：每个 commit 里自动带 `Amp-Thread-ID: https://ampcode.com/threads/T-xxx` trailer。用 `git log --show-notes` 或 `git interpret-trailers --parse` 可读出。配合 `trailer.AmpThread.ifexists=replace` 保证单个 commit 只留最新 thread。

**Q：怎么关掉 Amp-Thread trailer？**  
A：两层开关：
- Setting `git.commit.ampThread.enabled = false`（全局）
- Env `AMP_DISABLE_AMP_COAUTHOR_TRAILER=1`（只关 co-author，thread 依然注入）

**Q：Amp 本地能不能 verify 自己的签名？**  
A：不能在本地 verify —— 签名是 Sourcegraph 服务端托管 key 做的。GitHub 侧配置 Sourcegraph public key 才会显示 "Verified"。优势是沙箱里不需要分发 GPG 私钥。

**Q：credential helper 只代理 GitHub 吗？**  
A：是。代码里硬编码 `if (r.protocol !== "https" || r.host !== "github.com") return;`。其他 host 让 git 自己降级到下一级 helper。参见 `credentials-helper.md`。

**Q：diffStat 里的 `changed` 怎么定义？**  
A：每个 hunk 里 `min(added_lines, deleted_lines)`，剩余算 pure added/deleted。这样 `A 行改成 B 行` 不会既算 +1 又算 -1。参见 `diff-stats.md` 的 `V_0` 算法。

**Q：thread-level 的 diff snapshot 谁在用？**  
A：上传到 agentic remote runtime (DTW) 作为 `git_status` artifact，让 Agg Man (orchestrator persona) 跨 thread 查 working tree 差异，也支持 `live-sync` 子命令把 DTW 的远程 diff 反向同步回本地。

## 不在本 skill 里（但相关）

- `amp live-sync` 子命令：把 DTW thread 的 live working-tree mirror 到本地 checkout。见 `../remote-runtime/dtw.md`。
- `read_github` / Bitbucket 工具：外部代码检索。见 `../tools/catalog.md`。
- `commit_search` on GitHub: `code_search_agent` 可接到 GitHub API，但那是查询别人 repo，不是本地 git。

## 相关源码定位（反编译后的 symbol）

| Symbol | 作用 |
|---|---|
| `H4 = ["-c", "core.quotepath=false", "diff", "--no-color", "--no-ext-diff"]` | 所有 `git diff` 的 argv 前缀，确保输出稳定 |
| `$2R(cmd, configArgs, trailers)` | Bash 命令改写器，AST 级 locate `git commit`，插入 `-c trailer.*` + `--trailer` |
| `j2R(setting)` | 读 `AMP_DISABLE_AMP_COAUTHOR_TRAILER` 环境变量 + setting |
| `Hn0("get", url, secrets)` | `amp git-credential-helper get` 入口，stdout 吐 `protocol=https / username=x-access-token / password=<token>` |
| `Un0(url, apiKey)` | 用 apiKey 换 GitHub install token，POST 到 `/api/internal` |
| `Wn0(url, apiKey, commitObject)` | 请求 `signCommit`，body = commit object，回 signature |
| `qn0(url, secrets)` | `amp sign-commit` 主入口，stdin 读 commit object 走 Wn0，stdout 吐签名 + stderr `[GNUPG:] SIG_CREATED` |
| `uVT(cwd)` | Thread-level git snapshot (status + diff + aheadCount) |
| `pVT(porcelainZ)` | 解析 `git status --porcelain=v1 -z` 输出 |
| `V_0(diffText)` | 统计 added/deleted/changed，hunk 内 min() |
| `X_0(files[])` | sha256 over `path\0diff\0path\0diff...`，作为 snapshot 唯一键 |
