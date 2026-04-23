# Amp Logging — `TT` logger 实现

> Winston JSON logger，文件输出 + OTEL span event 镜像。进程宽度的全局单例 `TT`，全代码里 9000+ 处 `TT.debug/info/warn/error`。

## 环境变量 / CLI flag

从 `fm0()`（环境变量 help 文档生成函数，strings line 63375）以及 logger 初始化代码提取：

| Env var | 等价 CLI flag | 作用 | 默认值 |
|---|---|---|---|
| `AMP_LOG_LEVEL` | `--log-level` | 日志级别 | `info`（或命令 override） |
| `AMP_LOG_FILE` | `--log-file` | 日志文件路径 | `~/.amp/logs/cli.log`（见下） |
| `AMP_CLI_STDOUT_DEBUG` | — | 把 debug 级别同时打到 stdout | `false` |
| `AMP_MAX_LOG_FILE_SIZE` | — | 单文件上限字节 | `10485760`（10MB）|
| `AMP_SETTINGS_FILE` | `--settings-file` | settings.json 路径 | `~/.config/amp/settings.json` |

原文（错误提示）：
```
Invalid log level, must be one of debug, error, or warn     // line 33990
logLevel must be one of "verbose", "debug", "info", "warn", or "error"  // line 43226（来自 OpenAI SDK）
```

Amp 自己真正支持的级别由 `PdT = Object.keys(TT)` 动态得出，包括多一个**`audit`**（见下）。

## 默认日志文件路径

```js
Uc0 = path.join(iA, "logs")     // iA = ~/.amp
eeT = path.join(Uc0, "cli.log") // 最终：~/.amp/logs/cli.log
```

（`iA` 在其他位置拼成 `path.join(os.homedir(), ".amp")`。）

## Winston 配置

从 strings line 63312 `createLogger` 周围提取：

```js
// 略微去混淆后：
const formats = winston.format.combine(
  winston.format.timestamp(),
  (info) => { info.pid = process.pid; return info; },          // 注入 PID
  (info) => {                                                  // Error 序列化
    for (const key of Object.keys(info)) {
      if (info[key] instanceof Error) {
        info[key] = { name, message, stack };
      }
    }
    return info;
  },
  winston.format.json(),
  winston.format.errors({ stack: true }),
);

const transports = [
  new winston.transports.File({ filename: logFilePath }),
  new _KT(),                 // 见下 —— OTEL bridge transport
];
if (process.env.AMP_CLI_STDOUT_DEBUG === 'true') {
  transports.push(new winston.transports.Console({
    level: 'debug',
    format: winston.format.combine(
      winston.format.colorize(),
      winston.format.simple(),
    ),
  }));
}

const logger = winston.createLogger({
  level: PdT.includes(userLevel) ? userLevel : 'info',
  format: formats,
  transports,
});
```

## OTEL bridge transport (`_KT`)

这是 Amp 的**神来一笔**：每条 log 同时被 OTEL active span 当 event 收。

```js
_KT = class _KT extends winston.Transport {
  log({ message, ...rest }, callback) {
    wc0.trace.getActiveSpan()?.addEvent(message, rest);
    callback();
  }
};
```

效果：

- 如果 log 是在 `tracer.startActiveSpan(...)` 嵌套里发出的，它变成 span 的 event
- 不在任何 span 里时，`getActiveSpan()` 返回 undefined，直接丢（其实已经写了 File transport，不丢）

这种双写让 Cloudflare dashboard 直接能看到 log 时间线，不需要单独一条日志通道。

## `audit` 自定义级别

从 strings line 63312 附近的 `aCR(...)` 返回对象：

```js
return {
  error: (msg, ...meta) => uI(logger, 'error', msg, meta),
  warn:  (msg, ...meta) => uI(logger, 'warn',  msg, meta),
  info:  (msg, ...meta) => uI(logger, 'info',  msg, meta),
  debug: (msg, ...meta) => uI(logger, 'debug', msg, meta),
  audit: (msg, ...meta) => {
    const extra = typeof meta[0] === 'object' && meta[0] !== null
      ? { audit: true, ...meta[0] }
      : { audit: true };
    uI(logger, 'info', msg, [extra]);
  },
};
```

`TT.audit(...)` 记 `info` 级别但元数据里多一个 `audit: true`。用来标记 mutative / security-relevant 操作（tool 运行、权限改动等），下游可以 `jq 'select(.audit == true)'` 过滤。

## 关闭 / flush

```js
function uy() {              // flush + close 当前 transport
  if (cachedPromise) return cachedPromise;
  const current = Pl;
  if (!current) return Promise.resolve();
  cachedPromise = new Promise((resolve) => {
    let done = false;
    const finalize = () => { if (!done) { done = true; if (Pl === current) Pl = undefined; resolve(); } };
    setImmediate(() => {
      try { current.once('finish', finalize).once('error', finalize).end(); }
      catch { finalize(); }
    });
    setTimeout(finalize, 500);   // 500ms 超时保险
  });
  return cachedPromise;
}
```

进程退出前 `await uy()` 保证 File transport 落盘；兜底 500ms 超时避免卡住 exit。

## 日志信息结构约定

扫 strings 里所有 `TT.debug/info/warn/error(...)`，发现一致的 shape：

```js
TT.info("Event description starting with uppercase", {
  contextKeyInCamelCase: 'value',
  threadID: '...',
  durationMs: 123,
  error: errObj,     // 自动序列化为 { name, message, stack }
});
```

第一个参数永远是**字符串事件描述**（不是模板），第二个是结构化 metadata 对象。这方便下游用 `jq` 查询。

典型样例（strings line 63028、63069、63075 等抽取）：
```js
TT.debug("REPL tool completed with subthread usage", {
  subThreadID: b, inferenceCount: B.length, exitCode: O
});
TT.debug("OAuth callback server client error", { error: T.message });
TT.warn("Ignoring broken descendant symlink while globbing files.", {
  basePath: a, glob: R?.pattern, stderr: o
});
```

## 级别选择指南（从代码行为推断）

| 级别 | Amp 用法 |
|---|---|
| `error` | 不可恢复 / 需要用户干预（API 失败、权限错误、插件崩溃） |
| `warn` | 可恢复降级（handoff 跳过、ripgrep symlink 跳过、retry 后成功） |
| `info` | 生命周期大事件（login 成功、thread 开始、workspace 切换） |
| `debug` | 高频细节（每次 inference / 每次 tool 调 / WebSocket 心跳） |
| `audit` | 改变状态的操作（settings 写入、fs edit、secret 存取） |

## 对 Alva 的启发

现在 Alva 用 `tracing` crate（Rust）而不是 winston，但结构化 logging 的要义完全一样：

1. **统一的 env var 命名**：`ALVA_LOG_LEVEL / ALVA_LOG_FILE / ALVA_CLI_STDOUT_DEBUG`（抄 Amp 的 `AMP_*` 命名法）
2. **OTEL bridge**：`tracing-opentelemetry` 已经能做这件事。让每条 `tracing::info!` 自动成为当前 span 的 event（在 `AnalyticsExtension.configure()` 里注册 layer）
3. **`audit` 级别**：Rust 没有原生 audit，但可以用一个 `#[instrument(level = "info", target = "audit")]` 约定，`EnvFilter` 里过滤 `audit=info`
4. **默认路径**：抄 `~/.amp/logs/cli.log` → `~/.alva/logs/cli.log`（当前 `analytics.jsonl` 命名不清晰，日志 != analytics）
5. **Error 序列化**：Rust 的 `tracing::error!(?err)` 已做了，但注意 `?` 只格式化 Display。Amp 把 `{name, message, stack}` 显式拆字段，便于 jq 查询——Alva 可以加 `backtrace` / `err.chain()`

---

## 交叉引用

- OTEL 集成的另一半见 `./tracing.md`
- 日志在 `amp debug` package 里被如何呈现见 `./debug-package.md`
- 错误信息里的具体字符串（`NUT`、`Out of credits` 等）见 `./rate-limit-errors.md`

## 原始产物位置

- strings line 63312 周围：`createLogger` 定义
- strings line 63375：`AMP_*` env var help table
- strings line 65969：`PdT / Uc0 / eeT / AMP_MAX_LOG_FILE_SIZE / _KT` 定义
