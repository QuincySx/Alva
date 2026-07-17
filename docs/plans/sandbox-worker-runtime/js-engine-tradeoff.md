# `run_script` JavaScript 引擎取舍：QuickJS / rquickjs vs Boa

> 调研日期：2026-07-17。候选版本为仓库现状 `rquickjs 0.12.1` 与
> `boa_engine 0.21.1`；目标为 `wasm32-wasip1`。本票只做选型，不修改现有
> `run_script`。

证据标记：`[M]` 本机实测，`[M-user]` 任务委托方在同机实测，`[S]` 固定版本源码
检查，`[O]` 上游官方公开数据，`[U]` 本轮未验证。

## 决策摘要

**推荐保留 QuickJS / rquickjs，并把 WASI SDK 24.0 显式预装、校验和缓存。**
Boa 0.21.1 能编到 WASIp1，也能借 `evaluate_async_with_budget` 在 fresh context 上合作式
取消无限循环；但它没有 QuickJS `set_memory_limit` 的公开等价 API，因而不能维持现有
“脚本 OOM 变成可读 tool error、agent 继续”的硬契约。仅为消除 113 MB 首次下载而换
Boa，还会付出 release WASM 3.5 → 6.6 MB、约 200 行引擎接线重写和 ES 差异风险；显式
SDK 缓存只需小幅 CI / 测试 helper 调整，保留现有运行时语义和产物体积。

换言之：**Boa 的中断机制成立，但 Boa 仍因缺少每脚本内存上限而不满足当前硬约束。**

## 对比表

| 维度 | QuickJS / rquickjs（现状） | `boa_engine 0.21.1` | 判定 |
|---|---|---|---|
| WASIp1 编译 | `[M]` 现有 worker、集成测试与 CI 均在编译 | `[M-user][M]` 可编译且 spike 可由 wasmtime 执行 | 两者通过 |
| 只终止脚本的超时 | `[M][S]` `Runtime::set_interrupt_handler` 直接按墙钟轮询，错误回给 agent | `[M][S]` 无同名 interrupt hook；`Script::evaluate_async_with_budget` 可在 VM budget yield 处由外层 deadline 取消，随后销毁 fresh context | Boa **有条件通过**；实现更复杂 |
| 每脚本内存上限 | `[M][S]` `set_memory_limit(32 MiB)`，脚本 OOM 可读、agent 继续 | `[S]` 无公开 heap byte limit / allocator limit；`RuntimeLimits` 只有 loop、recursion、VM stack、backtrace | Boa **不通过硬约束** |
| Rust 原生函数绑定 | `[M]` `Function::new`；现有 13 个底层绑定 | `[M][S]` `NativeFunction` + `register_global_builtin_callable` 可用 | 两者通过；Boa 需重写转换和状态持有 |
| 无 `require` / module | `[M]` 未注册 loader，集成测试覆盖静态 import 与 require | `[M]` `Script` 模式下 `require` 为 `undefined`，静态 import 解析失败；不要引入 `boa_runtime` / module loader | 两者通过 |
| 常用 ES 内置 | `[M]` 现有 JSON、数组、字符串及 bootstrap 已由 worker 测试承重 | `[M][O]` JSON / 数组 / 字符串 spike 通过；Boa v0.21 官方 Test262 94.12% | 基础需求可用；Boa 仍有约 5.88% 总体缺口 |
| 构建代价 | `[S]` C/FFI；未提供 SDK 时 build.rs 用 curl 拉 113 MB、解包约 452–462 MB | `[M-user]` 纯 Rust；零 C 工具链、零 build-time 下载 | Boa 胜 |
| debug WASM | `[M-user]` 约 22 MB | `[M-user][M]` 约 65–67 MB | Boa 约 3 倍 |
| release WASM | `[M-user]` 约 3.5 MB | `[M-user]` 约 6.6 MB | Boa 大约 89% |
| 性能量级 | `[U]` 未做当前 WASIp1 + 文件绑定 workload 的可比 benchmark | `[U]` 同左 | 本轮不据此决策；上线前若重启候选需补测 |
| 项目内成熟度 | `[M]` 已有实现、契约文档和 timeout/OOM/module/fetch 集成测试 | `[S]` 0.x API，新接入；Test262 高但不是现有契约兼容证明 | QuickJS 风险显著较低 |
| 迁移成本 | 0；只治理构建输入 | `run_script.rs` 约 180–250 行接线改动，依赖与错误测试另计；内存上限还没有现成解 | QuickJS 胜 |

## Boa 证据

### 1. WASIp1、纯 Rust 和体积

`[M-user]` 委托方已在本机完成决定性构建，不重复凿同一结论：

```text
$ cargo build --target wasm32-wasip1
Finished in 25.39s
$ find target -iname "*wasi-sdk*"
# 无输出

QuickJS guest: debug ~22 MB, release ~3.5 MB
Boa spike:     debug ~65 MB, release ~6.6 MB
```

本轮为验证 cancellation 在真正 WASIp1 guest 中也工作，又对 API spike 做了离线构建与
wasmtime 执行；产物为 67 MB，与上述 debug 量级一致：

```text
$ cargo build --offline --manifest-path scratchpad/boa-api-spike/Cargo.toml \
    --target wasm32-wasip1
Finished `dev` profile ... in 22.33s
$ ls -lh scratchpad/boa-api-spike/target/wasm32-wasip1/debug/boa-api-spike.wasm
... 67M ... boa-api-spike.wasm
```

Boa 的默认 feature 只有 `float16` 与 `xsum`，但 Unicode / parser 等核心路径仍带入一组
ICU4X 依赖；不能把“以后关掉 Intl 就一定回到 QuickJS 体积”当成已验证缓解。若未来因
其他理由重启 Boa 方案，应另开 size-minimization spike。

### 2. 中断 / 超时：行为成立，但不是 QuickJS 式 handler

`[S]` `boa_engine-0.21.1/src/script.rs` 提供：

```rust
pub async fn evaluate_async_with_budget(
    &self,
    context: &mut Context,
    budget: u32,
) -> JsResult<JsValue>
```

VM 每耗尽一段带权指令 budget 就 `yield_now().await`。这不是
`Context::interrupt_handler`，但允许外层 future 在每次 poll 检查 `Instant` deadline，
到期后丢弃 evaluation future 和整个 context。官方 API 说明见
[`Script::evaluate_async_with_budget`](https://docs.rs/boa_engine/0.21.1/boa_engine/struct.Script.html#method.evaluate_async_with_budget)。

`[M]` spike 同时在 macOS native 和 WASIp1 / wasmtime 下得到相同输出：

```text
$ cargo run --offline --manifest-path scratchpad/boa-api-spike/Cargo.toml
native binding result=42
module boundary require="undefined", static_import_error=SyntaxError: ...
infinite loop outcome=cancelled, elapsed_ms=20
next fresh context result="[42,\"agent continues\"]"

$ wasmtime run scratchpad/boa-api-spike/target/wasm32-wasip1/debug/boa-api-spike.wasm
native binding result=42
module boundary require="undefined", static_import_error=SyntaxError: ...
infinite loop outcome=cancelled, elapsed_ms=20
next fresh context result="[42,\"agent continues\"]"
```

这满足当前 `run_script` 的行为需求，原因是现有实现本来就为每次调用新建 runtime /
context；取消后无需修复或复用半执行 VM，销毁它即可，agent loop 仍可继续。

但迁移时必须保留以下限制：

- 取消是合作式的，只发生在 Boa VM bytecode budget yield 处；它不是任意指令点的同步
  callback。
- `Script::evaluate_async_with_budget` 未正常返回时不会走函数尾部的 `pop_frame()`；超时
  后必须连同 fresh context 一起丢弃，不能复用该 context。
- parse / bytecode compile 和一次不返回的 Rust native binding 不受这个 budget 抢占。
  当前绑定均为同步、有限操作，但这是需要保持的约束。
- budget 是 VM cost，不是毫秒；墙钟判断仍由 Alva 外层实现。

因此，本报告把“Boa 有没有只杀脚本的超时机制”回答为 **有，合作式替代机制成立**；若
决策者把硬约束收紧为“必须有 QuickJS 同形的同步 interrupt callback”，则应把这一格改为
不通过。

### 3. 内存上限：不成立

`[S]` 对 `boa_engine 0.21.1` 与 `boa_gc 0.21.1` 固定版本源码搜索：

```text
$ rg -n -i 'memory.*limit|heap.*limit|max.*heap' boa_engine-0.21.1 boa_gc-0.21.1
# 无公开 limit API 命中

$ sed -n '1,140p' boa_engine-0.21.1/src/vm/runtime_limits.rs
pub struct RuntimeLimits {
    stack_size: usize,
    loop_iteration: u64,
    backtrace_limit: usize,
    resursion: usize,
}
```

公开的 [`RuntimeLimits`](https://docs.rs/boa_engine/0.21.1/boa_engine/vm/struct.RuntimeLimits.html)
只能限制 loop iterations、recursion、VM stack 与 backtrace。`boa_gc` 虽跟踪
`bytes_allocated` 和 GC threshold，但 `GcConfig` 是私有实现，源码仍写着
`TODO: Add a configure later`；threshold 是触发 GC 的水位，不是拒绝分配的上限。

可选替代均不等价：

- loop iteration limit 能碰巧拦住当前 `while(true) push(ArrayBuffer)` 测试，却拦不住一次
  大分配、无循环的内建操作，也会把合法大批处理变成固定迭代数限制。
- 宿主已有 256 MiB Wasm linear-memory limit，但它是整个 worker 的最后兜底；触发时会
  trap / 终止 agent，不能产出当前的可读 `run_script` memory error。
- 自定义 Rust global allocator 既不是每-context API，分配失败在 Wasm guest 中也未被
  Boa 定义成可恢复 JS exception，不应作为无专项验证的方案。

因此 Boa 未满足硬约束 3；这比构建便利和体积权衡更优先。

### 4. Rust 原生函数 API 与模块边界

`[M][S]` 最小 API 已编译并执行：

```rust
fn double(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::new(args[0].to_number(context)? * 2.0))
}

context.register_global_builtin_callable(
    js_string!("double"),
    1,
    NativeFunction::from_fn_ptr(double),
)?;
```

输出为 `native binding result=42`。官方入口见
[`Context::register_global_builtin_callable`](https://docs.rs/boa_engine/0.21.1/boa_engine/struct.Context.html#method.register_global_builtin_callable)。

当前 QuickJS 接线注册 13 个底层函数：11 个文件函数、`print`、`fetch`。Boa 可表达相同
能力，但不能机械替换类型名：

- 参数需从 `&[JsValue]` 显式做字符串 / 数字转换，返回值与异常改成 `JsValue` /
  `JsNativeError`。
- `Arc<WasiFs>` 和 output buffer 等非 `Copy` 状态，宜包装成 context host data，让
  non-capturing `NativeFunction::from_fn_ptr` 读取；直接用捕获 closure 会进入 Boa 标成
  `unsafe` 的 GC trace 责任区。
- 现有 `BINDING_BOOTSTRAP`、WasiFs 能力边界、fetch host proxy 和 Tool JSON 外壳可保留。

`[M]` `Context::eval` / `Script` 模式实测 `typeof require === "undefined"`，静态
`import value from "missing"` 为 SyntaxError。迁移时不要注册 module loader，也不要引入
提供 Web runtime API 的 `boa_runtime`，即可维持票 06 的 scope。

### 5. ES 覆盖度

`[O]` Boa 官方 v0.21 发布说明给出的 Test262 conformance 是 **94.12%**，从 v0.20 的
89.92% 上升；0.21.1 是同一 0.21 系列补丁版本。来源：
[`Boa release v0.21`](https://boajs.dev/blog/2025/10/22/boa-release-21)。官方也说明其
Test262 runner 使用 TC39 套件，见
[`Testing`](https://boajs.dev/docs/contributing/testing)。

`[M]` 本轮只做了当前 workload 的最小 smoke：JSON、数组、字符串、算术、全局 native
function 均通过。94.12% 足以说明 Boa 不是“玩具级”语法实现，但总体百分比不能证明
Alva bootstrap 的全部边角与 QuickJS 一致；真正迁移仍需把 `RUN-SCRIPT.md` 契约测试
整套移植后运行。

## 迁移成本估算

若忽略内存上限缺口并继续做 Boa 原型，预计改动如下：

| 区域 | 预计工作 |
|---|---|
| runtime / eval | QuickJS runtime/context 改为 Boa context/script；实现 budgeted future + deadline wrapper，超时销毁 context | 
| 13 个原生绑定 | 重写参数/返回值/异常转换；增加 context host-data wrapper 或审计过的 closure capture | 
| 限制 | stack/recursion/loop 可映射；32 MiB heap limit 无可映射 API，是未解设计项 | 
| module scope | 保持 Script 模式、无 loader；复跑 require/import 测试 | 
| 测试 | timeout、OOM、模块、文件逃逸、fetch、错误文本全部复跑；OOM 用例在有真正方案前无法过 | 
| 依赖 / 体积 | `rquickjs` 换 `boa_engine`；重新做 release size 与冷启动 / 峰值线性内存测量 | 

按当前 `run_script.rs` 483 行、其中 engine setup 约 40 行、`install_bindings` 约 180 行
估算，**会触及约 180–250 行 Rust**，另有 Cargo 一行和测试期望调整。bootstrap JS 与
Tool 外壳可保留，所以不是整文件重写；但 resource-limit 核心必须重做。实现加验证预计
1–3 个工程日，**不包含**发明 / 上游推动可恢复 heap limit 的时间。

## 不换引擎的构建缓解

### 已确认的上游开关

`[S]` `rquickjs-sys 0.12.1/build.rs` 固定使用 WASI SDK 24.0：

- `WASI_SDK=<path>`：直接使用 `<path>/bin/clang`、`bin/ar` 与
  `share/wasi-sysroot`，不进入 `download_wasi_sdk()`。
- `RQUICKJS_SYS_NO_WASI_SDK=1`：只跳过自动接线；调用者必须自己正确提供
  `CC`、`AR`、`CFLAGS` / sysroot。

`[M]` 在全新 target 目录中复用预装 SDK，Cargo 保持 offline，构建成功且没有下载：

```text
$ WASI_SDK=$PWD/target/.../out/wasi-sdk \
    cargo check --offline --locked -p alva-worker-wasm \
    --target wasm32-wasip1 --target-dir /private/tmp/alva-rquickjs-preinstalled-sdk.QRWrKS
Finished `dev` profile ... in 14.32s
```

`[M]` 反例：仅设置 NO_WASI_SDK、不给 WASI sysroot，会失败：

```text
$ env -u WASI_SDK RQUICKJS_SYS_NO_WASI_SDK=1 \
    cargo check --offline --locked -p alva-worker-wasm \
    --target wasm32-wasip1 --target-dir /private/tmp/alva-rquickjs-no-sdk.W5QCBB
...
fatal error: 'stdlib.h' file not found
... "clang" ... "--target=wasm32-wasip1" ...
```

所以 `RQUICKJS_SYS_NO_WASI_SDK=1` 不是消除 C 工具链的方案，只是把工具链发现责任从
build.rs 转给 CI。优先采用 `WASI_SDK`，可移植性和失败信息都更直接。

### 当前仓库已经具备的基础

`[S]` `crates/alva-sandbox-wasm/tests/runner.rs::cached_wasi_sdk()` 已会找到首次 debug
构建落在 `target/wasm32-wasip1/debug/build/rquickjs-sys-*/out/wasi-sdk` 的 SDK，并把它通过
`WASI_SDK` 传给临时 release guest 构建；这已经保证同一 job 内只下载 / 解包一次。

`.github/workflows/ci.yml` 的 test 与 coverage job 也已使用 `Swatinem/rust-cache@v2`。
如果其 target build output 命中，跨 workflow 可复用当前下载；但 SDK 藏在 Cargo build
hash 目录里，会随 Cargo.lock、toolchain、job cache key 或 cache eviction 失效，不宜把它
当成明确的工具链供应策略。

### 推荐 CI 方案

将 WASI SDK 当作版本化工具链，而不是某个 crate build.rs 的偶然副产物：

1. 在需要编 worker 的 job 中，把官方 WASI SDK **24.0** 解包到稳定目录，例如
   `~/.cache/alva/wasi-sdk-24.0`；下载时校验固定 SHA-256。
2. 用 `actions/cache` 按 `wasi-sdk-24.0 + runner.os + runner.arch` 缓存该目录。cache miss
   才下载 113 MB；命中后 build.rs 不联网。
3. job 级设置 `WASI_SDK` 指向稳定目录，再执行现有 Cargo 命令。
4. 让 `cached_wasi_sdk()` 优先读取环境变量 `WASI_SDK`，没有时才保留当前 target 搜索
   fallback；这样本地既有行为不变，CI 的临时 release 构建也能复用外部 SDK。

改动量估计：若直接在 test / coverage 两处展开，是每个 job 约 8–12 行 cache + install
YAML，以及 helper 约 4–6 行 Rust；若抽成本地 composite setup action，workflow 每个 job
约 2 行、共享 action 约 15–25 行。无论哪种都明显小于换引擎的约 200 行运行时改写，且
不改变 release WASM、JS 语义或现有资源限制。

缓存只能让下载“按 cache key 首次发生”，不能保证永不再下载；cache eviction、SDK 版本
升级或 runner OS/arch 变化都会产生新 miss。若组织要求冷启动也不访问 GitHub，应把经
校验的 SDK 镜像放入内部 artifact / tool cache；`WASI_SDK` 接线不变。

## 风险与争议点

### 保留 QuickJS 的风险

- `rquickjs-sys` build.rs 自行调用 curl，Cargo `--offline` 无法约束它；未显式设置
  `WASI_SDK` 的开发机或冷 CI 仍受 GitHub 可用性影响。
- 固定版本源码的下载路径未看到 checksum 验证。推荐方案在 CI 自己下载并校验，可同时
  降低可用性和供应链风险。
- 缓存不是 vendoring；需要明确 cache miss、内部镜像和 SDK 升级策略。
- QuickJS 仍需 C 工具链。缓存解决的是下载稳定性，不会让构建变成纯 Rust。

### 换 Boa 的风险

- 缺少每脚本 heap limit 是当前 blocker；用 worker 总内存上限代替会改变“agent 继续”
  契约。
- 合作式 timeout 依赖 fresh context 丢弃语义，且不能抢占 parser 或卡死的 native
  binding；实现比 QuickJS interrupt callback 更容易留边角。
- Test262 94.12% 是总体合规率，不是 QuickJS 行为逐项等价保证；剩余差异可能出现在
  RegExp、Unicode、Date / Intl 或异常细节。
- release 产物约增加 3.1 MB（约 89%），debug 约增加 43 MB（约 195%）；还应关注冷启动
  和 worker 256 MiB 线性内存下的峰值，本轮 `[U]` 未测。
- Boa 仍是 0.x API；迁移后要承担 API 变化和新的 engine-specific glue。

### 争议点的明确裁决

1. **Boa 中断是否成立？** 本报告裁决为“成立但有条件”：当前 fresh-context 结构下，
   budgeted async evaluation + 外层 deadline 能只取消脚本并让 agent 流程继续；它不是
   QuickJS 同形的 handler。
2. **Boa 是否因此可直接替换？** 否。每脚本可恢复 heap limit 不成立，已足以挡住替换。
3. **113 MB 是否不可缓解？** 否。`WASI_SDK=<path>` 已在全新 target + `--offline` 中
   实测成功；问题可以降级为版本化工具链缓存 / 镜像治理。
4. **是否因为 Boa 纯 Rust 就应接受大产物与重写？** 当前不应。纯 Rust 构建的收益真实，
   但现有痛点可用低风险 CI 改动解决，而运行时硬约束缺口尚无解。

## 最终建议

本票维持 `rquickjs 0.12.1`，后续单独开一个小型构建治理改动：显式缓存并校验 WASI SDK
24.0、设置 `WASI_SDK`、让 `cached_wasi_sdk()` 支持环境变量优先。不要修改
`run_script` runtime。

只有在 Boa（或其 GC / embedding API）出现经过 WASIp1 实测的**每-context 可恢复 heap
byte limit** 后，才值得重开引擎替换票；届时同时补齐 release size、冷启动、峰值内存和
当前全部 `run_script` 契约测试，而不是只看“纯 Rust、无下载”这一项。
