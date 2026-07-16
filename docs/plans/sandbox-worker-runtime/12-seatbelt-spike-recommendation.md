# Ticket 12 — Seatbelt spike 证据报告与 Ticket 13 实施建议

> 日期：2026-07-17
>
> 环境：macOS 26.5.1；仓库分支 `feat/sandbox-12-seatbelt-spike`
>
> 结论状态：**动态验证被当前托管环境阻断，Ticket 12 不能据此宣称通过**
>
> 交付性质：调研证据与 Ticket 13 建议；没有合入代码

## 0. 必须最先执行的安全结论：所有授权路径先 canonicalize

Ticket 13 在把任何 workspace、grant、临时目录、工具链目录或内部 job 目录写入
Seatbelt profile 前，**必须先对既存目录执行 `canonicalize()`，验证结果仍为目录，再去重**。
不能把用户输入或 `std::env::temp_dir()` 原样写入 profile。

这是硬门槛，不是优化项。macOS 的 `/tmp` 是 `/private/tmp` 的符号链接，Seatbelt
按解析后的真实路径匹配。下面是任务委托方已经在同一台机器完成、并明确要求本票直接采用而
不要重复的实测证据：

```text
# profile 写未解析路径
(deny file-read* file-write* (subpath "/tmp/sbtest/outside"))
$ sandbox-exec -f p.sb /bin/cat /tmp/sbtest/outside/secret.txt
secret

# profile 写真实路径
(deny file-read* file-write* (subpath "/private/tmp/sbtest/outside"))
$ sandbox-exec -f p.sb /bin/cat /tmp/sbtest/outside/secret.txt
cat: ...: Operation not permitted
$ sandbox-exec -f p.sb /bin/sh -c 'cat /tmp/sbtest/outside/secret.txt'
cat: ...: Operation not permitted
```

因此，“profile 成功应用”并不能证明路径规则生效；未 canonicalize 的 profile 可能只是
**看起来在工作**。Ticket 13 的集成测试必须至少保留一个“输入是 `/tmp/...`、profile 中实际
出现 `/private/tmp/...`”的回归用例。

证据来源标记在本文中统一如下：

- **[U]**：任务委托方已实测并提供的命令与输出。
- **[L]**：本次 Codex 托管环境实际执行的命令与输出。
- **[S]**：仓库或本机系统文件的只读静态证据。
- **[PENDING]**：当前环境无法执行，必须在无限制环境补测；不是结论。

## 1. 本次动态实验的阻塞点

本环境能启动 `/usr/bin/sandbox-exec`，但宿主策略禁止它调用 `sandbox_apply`。最小实验在
profile 内规则生效前就失败：

```sh
set -eu
ROOT=$(mktemp -d /private/tmp/alva-seatbelt-12-smoke.XXXXXX)
GRANT="$ROOT/grant"
OUTSIDE="$ROOT/outside"
mkdir "$GRANT" "$OUTSIDE"
printf 'inside\n' > "$GRANT/inside.txt"
printf 'secret\n' > "$OUTSIDE/secret.txt"
PROFILE="(version 1)
(allow default)
(deny file-read* file-write* (subpath \"$OUTSIDE\"))"
/usr/bin/sandbox-exec -p "$PROFILE" /bin/sh -c \
  'printf "shell=ok\n"; /bin/cat "$1/inside.txt"; /bin/cat "$2/secret.txt"' \
  sh "$GRANT" "$OUTSIDE"
```

实际输出 **[L]**：

```text
root=/private/tmp/alva-seatbelt-12-smoke.aSmRhQ
sandbox-exec: sandbox_apply: Operation not permitted
```

这与 profile 内的 `Operation not permitted` 不同：没有任何 sandboxed command 得到执行。
按 Ticket 12 的止损要求，本次在此立即停止 Seatbelt 动态实验。后文不会把 shell、git、cargo、
网络或逃逸面的推测写成实测结论。

## 2. 六个问题的答案

### 2.1 Profile 模板长什么样

#### 当前仓库模板不能复用为 Ticket 13 的隔离模板

现有 `SandboxConfig::generate_sb_profile()` 的 restrictive 分支是：

```scheme
(version 1)
(deny default)
(allow file-read*)
(allow file-write* (subpath "<writable-dir>"))
(allow process-exec)
(allow process-fork)
(allow sysctl-read)
(allow mach-lookup)
;; open/proxied 模式再加 (allow network*)
```

它只能作为语法参考，不能直接进入 Ticket 13，原因有四个：

1. 委托方已实测，`deny default` 配这组少量 allow 后连 `/bin/cat` 都不能正常启动 **[U]**。
2. `(allow file-read*)` 允许读取所有用户权限可读文件，不满足“只见授权路径”。
3. 路径由 `dir.display()` 直接插入字符串，既未 canonicalize，也未使用 profile 参数。
4. `RestrictiveProxied` 只生成 `(allow network*)`，没有任何 proxy 强制规则；它与
   `RestrictiveOpen` 的生成结果相同。

仓库证据 **[S]**：

```sh
nl -ba crates/alva-agent-security/src/sandbox.rs | sed -n '84,120p'
```

```text
84  pub fn generate_sb_profile(&self) -> String {
...
93      sb.push_str("(deny default)\n");
96      sb.push_str("(allow file-read*)\n");
...
100     sb.push_str(&format!(
101         "(allow file-write* (subpath \"{}\"))\n",
102         dir.display()
...
115     if self.allow_network {
116         sb.push_str("(allow network*)\n");
```

#### Ticket 13 的 allow-list 起始原型（尚未动态验证）

下面只能作为无限制环境的**第一版待测原型**，不是本票已验证模板：

```scheme
(version 1)
(deny default)

;; Apple 随系统提供的最小 dyld / system service 基线；这是 Private Interface，
;; 每个受支持 macOS 版本都必须跑兼容性测试。
(import "system.sb")

(allow process-exec)
(allow process-fork)
(allow sysctl-read)

;; 只读运行时/工具链。每个值均由 host canonicalize 后通过 -D 传入。
(allow file-read* file-map-executable
       (literal (param "SHELL"))
       (literal (param "GIT"))
       (subpath (param "RUSTUP_HOME"))
       (subpath (param "CARGO_CACHE")))

;; Job 明确授权的读写根与 job 私有临时目录。
(allow file-read* file-write*
       (subpath (param "GRANT_0"))
       (subpath (param "JOB_TMP")))

;; closed 模式不添加 network allow；open 模式才显式添加：
;; (allow network*)
```

使用 `(param ...)` 而不是字符串拼接有本机系统证据。`sandbox-exec(1)` 明确支持
`-D key=value` **[S]**：

```sh
MANWIDTH=100 man sandbox-exec | col -b | sed -n '1,45p'
```

```text
sandbox-exec [-f profile-file] [-n profile-name] [-p profile-string]
             [-D key=value ...] command [arguments ...]
...
-D key=value
        Set the profile parameter key to value.
```

系统自带 profile 也实际采用此形式 **[S]**：

```sh
rg -n '\(param "' /System/Library/Sandbox/Profiles/quicklook-thumbnail.sb | head
```

```text
18:(when (param "application_bundle")
19:      (allow-read-directory-contents (param "application_bundle"))
20:      (allow file-link (subpath (param "application_bundle"))))
21:(allow-read-write-directory-contents (param "application_darwin_user_dir"))
```

`system.sb` 能作为探索 dyld 基线的依据，但不能当稳定 API。文件自己写明是 Private
Interface 且随时可变 **[S]**：

```sh
sed -n '1,40p' /System/Library/Sandbox/Profiles/system.sb
```

```text
;; Copyright (c) 2026 Apple Inc.  All Rights reserved.
;; WARNING: The sandbox rules in this file currently constitute
;; Apple System Private Interface and are subject to change at any time and
;; without notice.
...
(import "dyld-support.sb")
...
;;; Read access to standard system paths
(allow file-read* file-test-existence ...)
```

**给 Ticket 13 的决定：** 只有上述 allow-list 原型通过第 4 节全部实测后，才可以把它称为
`--sandbox os`。若无法得到稳定、可跑 cargo 的 allow-list，宁可不发布 os 档，也不要把
allow-default deny-list 标成“内核只见授权路径”。

### 2.2 动态路径如何注入

建议 host 对每个 grant 执行以下确定性流水线：

1. 要求路径已经存在且 `is_dir()`；Ticket 13 不接受一个未来才创建的授权根。
2. `std::fs::canonicalize()`；失败即拒绝启动，不能退回原始路径。
3. 再次检查 canonical 结果 `is_dir()`。
4. 拒绝 `/`，并对过宽根（用户 home、`/private/tmp`、`/Users`）给出显式错误或至少强警告。
5. canonical 路径排序、去重，并消除被另一 grant 完全覆盖的子根。
6. 用独立 `-D GRANT_N=<canonical-path>` 参数传给固定 profile；不要用 `format!` 拼 Scheme。
7. profile 文件与 job 私有 `TMPDIR` 由 host 创建为 `0700`；`TMPDIR` 也先 canonicalize。
8. worker 启动前把 `HOME` 指向 job 私有 home，清除不需要的环境变量；不要默认授权真实 home。

当前 CLI 的 wasm grant 解析已经实现了第 1–3 步，可提取为 app 层共享 helper；不要让
`alva-agent-security` 反向依赖 app 层。证据 **[S]**：

```sh
sed -n '705,735p' crates/alva-app-cli/src/main.rs
```

```text
if !path.exists() { ... }
if !path.is_dir() { ... }
grants.push(path.canonicalize().map_err(|error| { ... })?);
```

当前 Seatbelt 代码则完全没有这条防线，而且默认加入整个进程临时目录 **[S]**：

```sh
nl -ba crates/alva-agent-security/src/sandbox.rs | sed -n '74,77p;160,168p'
```

```text
74  pub fn add_writable_dir(&mut self, dir: std::path::PathBuf) {
75      if !self.writable_dirs.contains(&dir) {
76          self.writable_dirs.push(dir);
...
161 pub fn for_workspace(workspace: &Path, mode: SandboxMode) -> Self {
163     config.add_writable_dir(workspace.to_path_buf());
166     config.add_writable_dir(std::env::temp_dir());
```

本机路径解析也说明 cargo 测试不能只授权 workspace；rustup shim、toolchain/cache 和 job
临时目录要分别建模 **[S]**：

```sh
realpath /bin/sh
realpath "$(command -v git)"
realpath "$(command -v cargo)"
realpath ~/.rustup
realpath ~/.cargo
realpath "${TMPDIR%/}"
```

```text
/bin/sh
/usr/bin/git
/Users/smallraw/.cargo/bin/rustup
/Users/smallraw/.rustup
/Users/smallraw/.cargo
/private/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T
```

不要因此授权整个 `~/.cargo` 读权限：它可能包含 registry credential。优先让 host 为 job
准备不含 credential 的只读依赖 cache，或精确授权 `registry/{cache,index,src}` 和所需 git
checkout；若 cargo 需要 cache lock 写入，则给 job 私有 `CARGO_HOME`，不要放宽真实 home。
这条具体目录集合必须由第 4 节的拒绝日志反向收敛，当前未实测。

### 2.3 shell / git / cargo 在圈禁下是否可用

**本次不能给出“可用”结论。** 当前环境在执行任何 sandboxed command 前就被
`sandbox_apply` 拒绝，见第 1 节 **[L]**。

委托方已经证明 `/bin/sh -> /bin/cat` 子进程继承拒绝规则 **[U]**；但 Ticket 12 明确要求的
真实 `shell -> git -> cargo test` 链路仍是 **[PENDING]**。Ticket 13 不得把 cat 的结果外推成
cargo 可用。

仓库当前指定的最小真实测试命令应为：

```sh
cargo test --offline -p alva-agent-security
```

无限制环境必须从 `sandbox-exec -> /bin/sh -c` 发起它，同时验证：

- `sh -c 'printf shell-ok'`：退出 0，stdout 为 `shell-ok`。
- `git status --short`：退出 0；读取 workspace 内 `.git` 成功。
- `cargo test --offline -p alva-agent-security`：退出 0，不能出现联网或授权外写入。
- 同一个 shell 中读取和写入 grant 外路径：非零退出且 stderr 含
  `Operation not permitted`。
- cargo/rustc 任意孙进程尝试访问 grant 外 canary：仍被拒绝。

这里必须保留完整 stdout、stderr 和退出码，不能只写“pass”。建议把原始输出保存到
`docs/plans/sandbox-worker-runtime/evidence/`，建议书只引用摘要。

#### 补测（委托方在无限制环境实测，2026-07-17）—— PENDING 已解除

Codex 的沙箱拒绝 `sandbox_apply`，所以 2.3 原为 [PENDING]。委托方在无限制的
macOS 26.5.1 上补跑，结论如下（全部 **[V]** 已验证）：

**结论一：deny-default + 手写 allow-list 起不来 cargo。**
allow-list 覆盖 /usr /bin /System /Library /private/var/db /private/etc
/dev/null /dev/urandom ~/.rustup ~/.cargo + 项目目录 + TMPDIR：

```
$ sandbox-exec -f p.sb cargo test --offline
exit=134            # SIGABRT，零输出，进程未能启动
```

与 `/bin/cat` 在 deny-default 下的表现一致。**建议书 2.5 推荐的
deny-default+allow-list，在"读"这一侧目前没有可运行的最小集**；若 13 坚持该方向，
必须先解决"哪些读是 cargo/rustc/dyld 启动所必需"这个问题，代价可能很高。

**结论二：deny-default + `(allow file-read*)` + 写白名单，cargo 可用。**

```
$ sandbox-exec -f p3.sb cargo test --offline
exit=0
test result: ok. 1 passed; 0 failed
```

写圈禁真实有效（内核拒绝，非工具层）：

```
$ sandbox-exec -f p3.sb /bin/sh -c 'echo pwned > /private/tmp/.../outside/hacked.txt'
/bin/sh: ...: Operation not permitted        # 文件确实未被创建
```

**结论三：TMPDIR 必须用真实值，不能用 `/private/tmp`。**
cargo 需要 TMPDIR 写权限。macOS 的真实 TMPDIR 是每用户目录：

```
$TMPDIR -> /private/var/folders/ct/<hash>/T
```

放开整个 `/private/tmp` 既非必要、又把一个全局共享可写区交给了 worker
（我第一版 profile 正是这样写的，于是授权外写"看起来没被拦住"——实为 profile 过宽，
不是 Seatbelt 失效）。**13 必须注入真实 `$TMPDIR`，且同样要 canonicalize。**

**结论四（安全代价，必须让用户知情）：`(allow file-read*)` 意味着 worker 能读走机器上的一切。**

```
$ sandbox-exec -f p3.sb /bin/sh -c 'ls ~/.ssh'
agent
config
```

即 os 档 worker 可读 `~/.ssh`、`~/.aws/credentials`、任何磁盘上的 API key。
这与 wasm 档"授权外路径根本不存在"的保证是**两个量级**的隔离强度。

**给 13 的取舍结论**：目前可行的唯一形态是"**写受圈禁、读不受圈禁**"。
它对"防止 worker 改坏授权外文件"有效，对"防止 worker 读走密钥"无效。
**13 若采用它，必须在文档与 CLI 措辞里如实说明这个边界，不得宣称"强隔离"**；
若要求读也圈禁，则需先攻克结论一，属未解难题。

### 2.4 已知逃逸面与限制

| 面 | 当前证据 | Ticket 13 要求 |
|---|---|---|
| 符号链接 / 未解析路径 | 未 canonicalize 会完全绕过路径规则 **[U]** | 所有 grant/support root/TMPDIR 必须 canonicalize；加 `/tmp` 回归测试 |
| Profile 字符串注入 | 当前代码直接插入 `dir.display()` **[S]** | 固定模板 + `-D` 参数；加入包含引号、换行的目录名测试 |
| 全局临时目录 | 当前 `for_workspace()` 默认授权整个 `temp_dir()` **[S]** | 每 job 私有 `0700` TMPDIR，只授权该 canonical 目录 |
| 网络 | allow-default 会默认保留网络；现有 open/proxied 都只是 `allow network*` **[S]** | closed 默认无 network allow；open 单独显式开启；不能把 open 称为 domain allow-list |
| `RestrictiveProxied` | 没有 proxy 强制，只等价于 open **[S]** | 删除/重命名，或真正通过 host proxy；不可继续沿用误导语义 |
| 只包 shell | 现有 `wrap_command()` 没有调用者；其他文件工具直接 OS I/O **[S]** | 必须圈禁整个 worker，不能只圈禁 `execute_shell` |
| shell 子进程 | `/bin/sh -> cat` 已证明继承 **[U]** | 补 git/cargo/rustc 孙进程链路实测 |
| 硬链接 | **[PENDING]** | 在 grant 内预置指向 grant 外 inode 的硬链接，验证是否可读/写；未通过前列为开放风险 |
| TOCTOU / 根目录替换 | **[PENDING]** | profile 生成后、worker 启动前替换/rename grant，验证 fail-closed；host 保持目录 FD 或拒绝根身份变化 |
| 已继承文件描述符 | **[PENDING]** | 在入沙箱前打开 grant 外 canary FD，验证 child 能否读取；关闭所有非必要 FD，日志改用受控 pipe |
| `/dev` 与 Unix socket | `system.sb` 会允许若干标准 device/socket **[S]** | 列出实际最小集合；测试 `/dev/fd`、SSH agent、Docker socket、用户 Unix sockets |
| Mach/XPC 服务 | `system.sb` 包含系统服务 mach-lookup allow **[S]** | 记录所导入规则；测试能否借服务间接读文件/联网；版本矩阵审计 |
| HOME / Cargo 凭据 | cargo shim/cache 位于用户 home **[S]** | 不授权真实 HOME；job 私有 HOME/CARGO_HOME 或无凭据只读 cache |
| Git 外部 helper / hooks | **[PENDING]** | 测试 hooks、credential helper、pager、diff driver、SSH；清理 `GIT_*`, `SSH_AUTH_SOCK` 等环境 |
| 递归 `sandbox-exec` | **[PENDING]** | 验证 sandbox 内再次调用 sandbox-exec 不能放宽父约束 |
| 后台/脱离进程 | **[PENDING]** | `nohup sh -c ... &`、process group、worker 退出后存活测试；必须仍受策略并可回收 |
| API 废弃与 OS 漂移 | 本机 man page 明确标为 DEPRECATED **[S]** | capability probe + 每个 macOS 版本集成测试；失败时绝不回退裸执行 |

废弃证据 **[S]**：

```sh
MANWIDTH=100 man sandbox-exec | col -b | sed -n '1,18p'
```

```text
NAME
     sandbox-exec – execute within a sandbox (DEPRECATED)
...
DESCRIPTION
     The sandbox-exec command is DEPRECATED. Developers who wish to sandbox an app
     should instead adopt the App Sandbox feature ...
```

### 2.5 allow-default deny-list 与 deny-default allow-list 的取舍

#### allow-default + deny-list

优点是 shell/git/cargo 很可能少遇到兼容性缺口；委托方提供的“deny 一个 canonical outside
目录”实验已经证明这种具体 deny 能由内核执行 **[U]**。

但它不能满足 Ticket 13 的核心契约：未枚举的用户文件、Mach/XPC、Unix socket、device、
network 与未来新增系统能力都默认开放。它只能叫“针对已知路径的防护”，不能叫“只见授权
路径”，也不能作为 `AcceptShell` / `Bypass` 的兜底依据。

#### deny-default + allow-list

它符合 fail-closed 与“只允许列出的路径”的契约，但委托方已经实测朴素模板连程序都起不来
**[U]**。即使导入 `system.sb` 后能运行，Apple 明示该文件是 Private Interface **[S]**，所以
维护成本和 OS 漂移是真实风险。

#### 推荐

**正式 `--sandbox os` 只接受 deny-default + 经过实测收敛的 allow-list。**

若 Ticket 13 不能让该模板稳定完成真实 `cargo test --offline`，应把 os tier 留在 experimental/
unavailable 状态，并继续用 wasm tier 承担强隔离；不要降级成 allow-default deny-list 后仍让
`is_enforced()` 返回 true。allow-default 方案最多可作为 `Ask` 权限模式下的 defense-in-depth
实验档，名字和 UI 必须明确“不构成授权根隔离”。

### 2.6 集成点：`is_enforced()`、`SandboxMode`、CLI 形状

#### `SandboxConfig::is_enforced()` 当前会产生错误安全信号

现在它在任何 macOS 进程中都返回 true：

```rust
pub const fn is_enforced() -> bool {
    cfg!(target_os = "macos")
}
```

但 `wrap_command()` 除自身测试外没有生产调用者 **[S]**：

```sh
rg -n 'wrap_command\(' crates --glob '*.rs'
```

```text
crates/alva-agent-security/src/sandbox.rs:129: pub fn wrap_command(...)
crates/alva-agent-security/src/sandbox.rs:146: pub fn wrap_command(...)
crates/alva-agent-security/src/sandbox.rs:230: let cmd = config.wrap_command(...)
```

与此同时 CLI 已用这个静态 bool 放行 `accept-shell` / `bypass` **[S]**：

```text
crates/alva-app-cli/src/main.rs:361:
if mode.assumes_sandbox() && !alva_app_core::SandboxConfig::is_enforced() {
```

所以在 Ticket 13 实装前，macOS native CLI 实际是“未圈禁但门禁认为已圈禁”。Ticket 13 必须
先关闭这个错误信号。

建议把“平台支持”和“本次 worker 已受约束”拆开：

- `SeatbeltSupport::probe()`：检查平台、二进制和一个真实 apply probe；只代表可用性。
- `SandboxEnforcement::None | SeatbeltActive`：由成功启动 sandboxed worker 的 host 路径持有。
- permission-mode gate 检查本次 invocation 的 `SeatbeltActive`，不能检查 `cfg!(macos)`。
- `sandbox-exec` apply/spawn 失败时 worker 不运行，绝不退回裸命令。

#### 必须圈禁整个 worker，而不是只改 `ExecuteShellTool`

现有 shell 两条路径都直接 `Command::new("sh")` **[S]**：

```text
crates/alva-agent-extension-builtin/src/execute_shell.rs:59
    let mut cmd = Command::new("sh");
crates/alva-agent-extension-builtin/src/local_fs.rs:101
    let mut cmd = Command::new("sh");
```

即使把它们接到 `wrap_command()`，`read_file` / `write_file` 等仍在未圈禁 agent 进程内直接做
Tokio filesystem I/O。Ticket 13 的正确边界是 host 生成 profile，然后以
`sandbox-exec ... <worker>` 启动**整个全工具 worker**；由内核自然约束它的所有子进程。

#### `SandboxMode` 与 ACP `SandboxLevel`

当前四个 `SandboxMode` 不能准确表达实施后的契约：open 与 proxied 相同，permissive 没有隔离，
restrictive 模板又未实测可运行。建议 Ticket 13 用更直接的内部结构：

```text
SandboxTier::None | OsSeatbelt
OsNetworkPolicy::Denied | Unrestricted
grants: Vec<CanonicalPath>
```

`alva-protocol-acp::SandboxLevel` 也不是现成接线点。全仓搜索只发现 serde 测试和 delegate 给
它填默认 `None`，没有消费端 **[S]**：

```sh
rg -n 'SandboxLevel::|sandbox_level' crates/alva-protocol-acp crates --glob '*.rs'
```

因此不要为了复用一个死 DTO 把 CLI/jobs 耦合到 ACP。若 ACP 将来需要同一能力，可在 app/host
边界显式映射。

#### CLI 与 jobs

用户面应与 wasm 对齐：

```text
alva -p --sandbox os --grant <DIR> [--grant <DIR> ...] ...
alva jobs submit --sandbox os --grant <DIR> ... "PROMPT"
```

具体建议：

- `--sandbox` 接受 `wasm|os`，两者都要求至少一个既存目录 `--grant`。
- `--grant` 共用 canonicalize helper 和相同错误文本。
- `--allow-domain` 对 `os` 初版应拒绝，而不是假装能限制任意 shell 的域名；Seatbelt os 档初版
  只提供 network denied/unrestricted 的诚实语义。
- direct `-p --sandbox os` 由当前 host re-exec/spawn 一个隐藏 worker；不能等 agent 已构造后才
  尝试 apply。
- `jobs_cmd::submit()` 是自然 spawn 接线点；它目前已经以当前 exe 启动 detached `-p` worker。

静态证据 **[S]**：

```text
crates/alva-app-cli/src/jobs_cmd.rs:112-133
let exe = std::env::current_exe()?;
let mut cmd = std::process::Command::new(exe);
cmd.arg("-p").args(["--output-format", "json"]).args(rest)...;
let child = cmd.spawn()?;
```

不过 jobs 还有一个必须处理的内部路径：`result.json`/`stderr.log` 在 host 预先打开后作为 FD
继承，而 tool logger 通过 `JOB_TOOLS_LOG_ENV` 路径在 child 内再次打开。不要为了这个日志路径
授权整个 `~/.alva/jobs`；应改为 host-owned pipe/预打开 FD，或只授权该 job 的 canonical
目录并明确它是内部 support grant。前一种更不暴露宿主 job 数据。

同理，provider config、全局 skills/plugin 配置不要靠放宽真实 HOME 让 worker 自己读取。
unsandboxed host 应先解析所需配置并把最小 bootstrap 交给 worker。否则 shell 工具也能读取同一
credential/config grant，破坏“授权根之外不可见”的承诺。

## 3. 既有代码为什么成为死代码、能复用多少

`sandbox.rs` 在 2026-03-20 的 `feat(security): implement Sub-7 security layer` 中与
`SecurityGuard` 一起加入。历史版本已经具有 `generate_sb_profile()`、`wrap_command()` 和
`for_workspace()`，之后主要是 crate 搬迁和非 macOS 警告；没有出现生产接线 commit。

```sh
git log --format='%h %ad %s' --date=short --follow -- \
  crates/alva-agent-security/src/sandbox.rs
```

```text
3bba4c7 2026-07-01 fix(security): close credential-leak and silent-failure gaps in agent and CLI
675e107 2026-06-22 refactor: complete plugin-based agent architecture
...
7653e73 2026-03-20 feat(security): implement Sub-7 security layer
```

`SecurityGuard` 只保存 `SandboxConfig` 并提供 getter，没有执行包装；shell tool 也拿不到这个
getter。因此死因不是近期重构遗漏，而是最初就只有“配置/字符串生成”半边，没有选定真正的
process spawn enforcement seam。

可复用部分：

- `SandboxMode` 的网络意图可作为迁移输入，但枚举语义需要重做。
- profile 字符串生成的单测结构可保留，升级为 canonical path、参数数量和网络策略单测。
- 非 macOS fail-closed 的设计意图可保留。

不可直接复用部分：

- 当前 profile 内容。
- `add_writable_dir()` / `for_workspace()` 的原始路径与全局 temp 逻辑。
- `wrap_command()` 只包单条 shell 的边界。
- 静态 `is_enforced()`。
- ACP `SandboxLevel` 作为当前 CLI/jobs 接线协议。

## 4. 无限环境的必跑复测矩阵

以下项目全部通过以前，Ticket 12/13 不应标完成：

1. **Apply probe**：最小 allow-default profile 能执行 `/usr/bin/true`；保存退出码。
2. **Canonical grant**：输入 `/tmp/...`，生成参数必须是 `/private/tmp/...`；grant 内读写成功，
   grant 外读写都得到内核 `Operation not permitted`。
3. **真实链路**：`sandbox-exec -> /bin/sh -> /usr/bin/git` 和
   `sandbox-exec -> /bin/sh -> rustup -> cargo/rustc/linker`。
4. **真实重任务**：在本仓库运行 `cargo test --offline -p alva-agent-security`，保存完整输出和
   退出码；确认没有网络请求。
5. **网络**：closed 下 TCP、UDP、DNS、Unix socket 分别测试；open 下只验证声明允许的行为。
6. **路径攻击**：symlink、硬链接、`..`、引号/换行目录名、profile 后 swap/rename、grant 嵌套。
7. **进程攻击**：孙进程、nohup/background、递归 sandbox-exec、worker 退出后的孤儿进程。
8. **FD/IPC**：预打开 grant 外 FD、SSH agent、Docker socket、Mach/XPC 间接访问。
9. **用户面**：`-p` 与 `jobs submit` 同样的 `--sandbox os --grant` 行为、日志和 permission mode。
10. **失败路径**：profile 语法错误、`sandbox_apply` 失败、grant 消失、unsupported macOS；每项
    都必须拒绝运行，不能 fallback。

建议真实 cargo 命令统一使用 `--offline`，不得删除 `target/`：

```sh
/usr/bin/sandbox-exec \
  -f "$PROFILE" \
  -D GRANT_0="$WORKSPACE_CANON" \
  -D JOB_TMP="$JOB_TMP_CANON" \
  -D SHELL="$(realpath /bin/sh)" \
  -D GIT="$(realpath "$(command -v git)")" \
  -D RUSTUP_HOME="$(realpath ~/.rustup)" \
  -D CARGO_CACHE="$CARGO_CACHE_CANON" \
  /bin/sh -c 'git status --short && cargo test --offline -p alva-agent-security'
```

该命令当前标记为 **[PENDING]**；本文没有伪造它的输出。

## 5. 最终结论与争议点

### 已有结论

- canonicalize 是强制安全边界；未做会让路径圈禁失效 **[U]**。
- 子进程会继承已成功应用的 canonical 路径拒绝规则；目前只实测到 `sh -> cat` **[U]**。
- 本托管环境不能应用 Seatbelt profile，无法完成 shell/git/cargo 动态验收 **[L]**。
- 当前仓库 Seatbelt 是未接线的字符串生成死代码，不能让 `is_enforced()` 成立 **[S]**。
- 当前 macOS `is_enforced() == true` 与实际未圈禁状态矛盾，是 Ticket 13 必须先修的安全门禁
  问题 **[S]**。
- `sandbox-exec` 已由 Apple 标记 deprecated，系统基线 profile 又是 Private Interface，必须
  接受版本矩阵和 fail-closed 维护成本 **[S]**。

### 仍有争议、必须以实测裁决

- `deny default + import system.sb + 精确工具链根` 能否稳定完成本仓库 cargo test。
- cargo 的最小只读 cache 集合，以及哪些 lock/temp 路径必须写。
- hard link、继承 FD、Mach/XPC 与 Unix socket 是否构成可利用的授权根逃逸。
- os 档是否允许全网络；Seatbelt 无法诚实表达现有 wasm 的 host/domain allow-list 语义时，
  CLI 应显式区分，而不是复用 `RestrictiveProxied` 名字。
- 为 direct `-p` 和 jobs 共用 host/worker bootstrap，需要抽取多少现有 wasm host proxy 逻辑。

**Ticket 12 状态建议：blocked（需要无限制 macOS 环境补跑第 4 节），不是 done。**
