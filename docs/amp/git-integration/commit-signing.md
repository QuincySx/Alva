# Commit Signing

> Amp 让**服务端托管 GPG key** 签 commit。沙箱里 `gpg.program = amp sign-commit`，commit object 转发到 `/api/internal`，输出 GPG 协议伪装的 SIG_CREATED 让 git 满意。

---

## 设计动机

沙箱里（DTW、Cloudflare Workers runtime、agent container）**不方便分发 GPG 私钥**：
- 私钥落盘 → 任何拿到 disk 镜像的人都能冒签
- 每次 spawn 生成新 key → GitHub 侧要不断批准新 key 才显示 Verified
- 用用户本地 gpg-agent → 跨 sandbox 做 socket forwarding 又蠢又不安全

Amp 的解法是 **"把 gpg.program 换成一个 RPC stub"**：git 需要签名时本来要 fork `gpg -bsau <keyid>`，现在改成 fork `amp sign-commit`；`amp sign-commit` stdin 读 commit object（binary），HTTPS 到 Sourcegraph 服务端，服务端用托管 key 签好再返回。

## CLI 入口（hidden commands）

在 `r20(T)` —— commander.js 顶级 CLI 注册里：

```js
R.command("sign-commit", { hidden: true })
 .summary("Git commit signing helper")
 .description("Internal: implements the gpg signing interface for git commit signing. Used inside sandboxes as gpg.program.")
 .allowUnknownOption()
 .action(async (b, _) => {
   let m = _.optsWithGlobals();
   let y = await N8(m);
   await qn0(y.ampURL, y.secrets);
   process.exit(process.exitCode ?? 0);
 });
```

- **hidden**：`amp --help` 不列出。只有沙箱 bootstrap 脚本知道用
- **allowUnknownOption**：git 调 gpg 时会传 `-bsau <key>` 等 flag，这里都忽略
- 走 `qn0(ampURL, secrets)` 做真正工作

## `qn0` 主体

```js
async function qn0(T, R) {
  let t = await R.get("apiKey", T);
  if (!t) {
    TT.error("No API key found. Run `amp login` first.");
    process.exitCode = 1;
    return;
  }
  let r = [];
  for await (let a of process.stdin) r.push(a);   // 读整个 commit object
  let e = Buffer.concat(r).toString("utf-8");
  let h = await Wn0(T, t, e);                     // RPC 签名
  if (!h) { process.exitCode = 1; return; }
  process.stdout.write(h);                        // ASCII-armored signature to stdout
  process.stdout.write("\n");
  process.stderr.write("\n[GNUPG:] SIG_CREATED ...\n");   // 伪装 GPG machine-readable status
}
```

关键：
- stdin 一次性读完。commit object 通常 < 1KB
- stdout 必须是合法的 ASCII-armored OpenPGP signature（`-----BEGIN PGP SIGNATURE-----`...）
- stderr 必须吐 `[GNUPG:] SIG_CREATED ...` —— git 的 `--status-fd` 协议（见 `doc/gnupg.texi#SIG_CREATED`），否则 git 认为 gpg 失败

## `Wn0` —— signCommit RPC

```js
async function Wn0(T, R, t) {
  let r = await fetch(`${T}/api/internal`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${R}`
    },
    body: JSON.stringify({ method: "signCommit", params: { commitObject: t } })
  });
  if (!r.ok) return TT.error("Failed to fetch commit signature", {status: r.status}), null;
  let e = await r.json();
  if (!e.ok || !e.result?.signature)
    return TT.error("Commit signing failed", {error: e.error}), null;
  return e.result.signature;
}
```

- Endpoint: `${AMP_URL}/api/internal`（内部 JSON-RPC-ish，method="signCommit"）
- Auth: `Bearer ${apiKey}`，就是 `amp login` 存的那个
- Body: `{ method: "signCommit", params: { commitObject: <utf-8 blob> } }`
- Response: `{ ok, result: { signature }, error }`

服务端用**托管 key pair** 做签名。public key 要在 GitHub / 企业 IdP 侧登记才会显示 "Verified"。

## 沙箱如何 wire 进来

Amp CLI 描述里写得明确："**Used inside sandboxes as `gpg.program`.**"

推测沙箱 bootstrap 脚本做这些：

```bash
git config --global gpg.program "amp sign-commit"
git config --global commit.gpgsign true
git config --global user.signingkey "amp-managed"    # 占位，服务端忽略
```

这样所有 `git commit`（尤其 trailer-aware 的那条）会自动签，不需要 agent 显式调 GPG。

## 为什么不在本地用 gpg-agent

假设用 socat forward host gpg-agent socket 到 sandbox：
- 需要 agent 提前知道 user UID / agent socket 路径
- 远程 DTW 根本没法直连用户桌面 socket
- forward 过程中 commit object 泄漏给任何拿到 socket 文件的人

用 RPC 托管：
- 服务端做 authz：apiKey 绑用户，签名记审计日志
- 跨 sandbox/machine/DTW 无缝
- 换 key 只要在服务端轮转一次，不动任何 agent

## 可能的副作用

- **离线不能签**。amp sign-commit 必须联网到 ampURL
- **Sourcegraph 服务端能看到所有 commit object**（作者、message、tree hash）—— 对有合规要求的企业客户可能是 blocker
- **Key 撤销会让历史 commit 集体变 Unverified**（需要重 sign 或接受）

## 对 Alva 的启发

Alva 尚无 commit signing 功能。如果要抄：

1. **实现层选择**：
   - 如果 Alva 有 hosted backend（类似 Sourcegraph）：全抄，最直接
   - 如果纯本地：考虑包一个 `alva sign-commit` 转发到用户本机 gpg-agent，或接 `ssh-keygen -Y sign` 的 SSH signing（Git 2.34+ 支持 `gpg.format = ssh`）

2. **SSH signing 是更轻量的选项**：
   - 用户 `~/.ssh/id_ed25519` 本来就有
   - `git config gpg.format ssh` + `user.signingkey ~/.ssh/id_ed25519.pub`
   - 不需要服务端 key 托管
   - GitHub 侧认 SSH signing key

3. **`[GNUPG:] SIG_CREATED` 协议小知识**：如果你要做任何 `gpg.program` 替代，**必须** 往 stderr 吐这条 —— git 只读 stderr 上的这行判断签名是否成功，stdout 只当 signature body。漏了就会出 `gpg failed to sign the data` 这种迷惑错误。

4. **和 `git.commit.alvaThread.enabled` 组合**：签名 commit + trailer 一起上，才能做出"Verified by Alva, Thread: ..."的完整审计链。
