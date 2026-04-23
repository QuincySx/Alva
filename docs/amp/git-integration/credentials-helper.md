# GitHub Credentials Helper

> 沙箱里 `git push`、`git fetch` 访问 github.com 时，凭证不来自 `~/.git-credentials` 也不来自 PAT —— 走 `amp git-credential-helper`，apiKey 换一次性 `x-access-token`。

---

## 目的

agent 跑在沙箱（Docker / DTW / CI）里时，需要能 `git push` 但**不能落盘 PAT**：
- PAT 落盘 → sandbox 镜像泄露会泄 token
- `gh auth login` 风格的 OAuth → 沙箱没浏览器
- Personal access token 长效 → 吊销困难，审计麻烦

Amp 的解：**用 amp login 的 apiKey 作为唯一凭证**，每次 git 需要密码时现场去服务端换一个短命 GitHub install token（`x-access-token` user 样式，通常是 GitHub App installation token）。

## CLI 入口（hidden command）

```js
R.command("git-credential-helper [action]", { hidden: true })
 .summary("Git credential helper for GitHub")
 .description("Internal: implements the git credential helper protocol. Used inside sandboxes to authenticate git operations with GitHub.")
 .action(async (b, _, m) => {
   let y = m.optsWithGlobals();
   let u = await N8(y);
   await Hn0(b ?? "get", u.ampURL, u.secrets);
   process.exit(process.exitCode ?? 0);
 });
```

- action 参数: git 调 credential helper 用 `get` / `store` / `erase`，这里只处理 `get`
- 同样 hidden，只给 sandbox 自动配置用

## Git credential helper 协议回顾

git 调 helper 的协议（`gitcredentials(7)`）：

```
git → helper stdin:
  protocol=https
  host=github.com
  path=owner/repo.git

helper → git stdout:
  protocol=https
  host=github.com
  username=x-access-token
  password=ghs_XXXXXXXXXXXXXXXXXXX
```

空行结束。password 字段要是 ASCII（token 格式无问题）。

## `Hn0` 主体

```js
async function Hn0(T, R, t) {
  if (T !== "get") return;                       // 忽略 store / erase

  let r = await Nn0();                           // 从 stdin 读 protocol=/host=/... 对
  if (r.protocol !== "https" || r.host !== "github.com") return;    // 其他 host 放弃，让 git 走下一个 helper

  let e = await t.get("apiKey", R);
  if (!e) {
    TT.error("No API key found. Run `amp login` first.");
    process.exitCode = 1;
    return;
  }

  let h = await Un0(R, e);                       // 用 apiKey 换 GitHub install token
  if (!h) { process.exitCode = 1; return; }

  process.stdout.write(`protocol=https
username=x-access-token
`);
  process.stderr.write(`\n`);                    // ... plus password= line output elsewhere
}
```

关键行为：
- **只处理 github.com**，非 GitHub host 直接 return，让 git 走 fallback helper chain（例如 OS keychain）
- action 为 `store` / `erase` 也直接 return（无状态 helper，不记忆）
- `Nn0()` 解析 git 的 stdin 协议
- 错误时 exit code != 0，git 会跳到下一个 helper

## `Un0` —— apiKey → install token

函数体未完整反编译出来，但从上下文拼出来它是：

```js
async function Un0(ampURL, apiKey) {
  let res = await fetch(`${ampURL}/api/internal`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${apiKey}`
    },
    body: JSON.stringify({ method: "getGitHubToken", params: {} })   // (name inferred)
  });
  if (!res.ok) return null;
  let body = await res.json();
  if (!body.ok || !body.result?.accessToken) {
    if (body.error?.message) process.stderr.write(`${body.error.message}\n`);
    return null;
  }
  return body.result.accessToken;
}
```

反编译出来相关片段：
```js
`),null;let r=await t.json();if(!r.ok||!r.result?.accessToken){
  if(TT.debug("GitHub git token not available",{error:r.error}),r.error?.message)
    process.stderr.write(`${r.error.message}\n`);
  `);return null}return r.result.accessToken}
```

服务端返回 `{ ok: true, result: { accessToken: "ghs_..." } }`。从用户名 `x-access-token` 看 —— **是 GitHub App installation token**（GitHub App 装在 user/org 上，token 短命，可限制 scope）。

## 沙箱如何 wire 进来

推测 bootstrap 脚本：

```bash
git config --global credential.helper ""            # 清空默认
git config --global credential.helper "!amp git-credential-helper"
git config --global credential.useHttpPath false    # host 级复用 token
```

`!` 前缀让 git 以 `shell` 方式执行（否则会找 `git-credential-amp` binary）。

## 为什么不是 PAT / OAuth

| 方案 | 问题 |
|---|---|
| 硬编码 PAT | 泄露后要人工轮转，审计难 |
| GitHub OAuth Device Flow | 沙箱无浏览器 |
| ssh key | DTW/sandbox 无法动态注入私钥 |
| `gh` CLI credential store | 仍要分发 token 到沙箱 |
| **Amp apiKey → install token** | 短命 token（几十分钟），可按 repo scope，吊销 apiKey 即全吊销 |

## 吊销链

```
user clicks "Revoke apiKey" on ampcode.com
  ↓
apiKey 立即失效
  ↓
下次 git push 时 Hn0 换 token 失败，exit 1
  ↓
git 尝试 next helper（通常没有）→ push 失败
```

不用碰 GitHub PAT 列表。

## 对 Alva 的启发

Alva 目前没有 git credential 代理。对比 Amp：

1. **抄核心思路**：把 `alva login` 的 token 当沙箱内的唯一凭证，换 GitHub token。
   - 需要 Alva 有 hosted backend 或 GitHub App

2. **无 backend 的替代方案**：
   - 让 `alva git-credential-helper` 桥接宿主机的 `gh auth token`
   - Agent 在沙箱里请求凭证 → helper 调 host 上已 auth 好的 `gh`
   - 不需要服务端，但依赖 `gh` CLI

3. **协议实现细节别漏**：
   - 只处理 `get`，`store` / `erase` 默默 return
   - 只处理 `protocol=https` + `host=github.com`（或将来扩 `gitlab.com`）
   - 失败时 stderr 吐原因，exit non-zero
   - **绝对不要**输出到 stdout（非协议行），否则 git 会当成 password

4. **可以和 `SecurityExtension` 整合**：把 "是否允许沙箱访问 git" 纳入 policy；默认 push 要二次确认。

5. **Bootstrapping**：Amp 在沙箱启动时注入 `credential.helper` 全局 config。Alva 如果有 `SandboxExtension`，可以在 `on_sandbox_start` hook 里同样注入。
