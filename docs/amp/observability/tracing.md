# Amp OpenTelemetry Tracing

> Amp 直接依赖 `@opentelemetry/api` + `@opentelemetry/sdk-node`。进程里的所有 agent turn / tool 调用 / plugin hook / fetch 请求都被 span 包裹。

## NodeSDK 初始化

从 strings line 63060 提取（去混淆）：

```js
new NodeSDK({
  serviceName: "amp.cli",
  sampler: new AlwaysOnSampler(),
  contextManager: new AsyncLocalStorageContextManager(),
  instrumentations: [new FetchInstrumentation()],   // 自建
  traceExporter: undefined,                         // 没有外发 exporter
  metricReader: undefined,                          // 没有 metrics
}).start();
```

注意要点：

- **`AlwaysOnSampler`** —— 每个 span 都采样（本地进程成本低）
- **`AsyncLocalStorageContextManager`** —— Node 原生 ALS，支持 async/await 自动传递上下文
- **没有 traceExporter** —— 所有 trace 停留在本进程；由自定义 `traceStore` 吸收，沿 WebSocket 发去 DTW
- **没有 metrics** —— `metricReader: void 0`

## 自建的 `FetchInstrumentation`

Amp 自己写了一个，每个 `fetch(...)` 调用都变成 CLIENT span：

```js
class FetchInstrumentation extends InstrumentationBase {
  enable() {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = async (input, init) => {
      const url = typeof input === "string" ? new URL(input)
                : input instanceof URL ? input
                : new URL(input.url);
      const method = init?.method || "GET";
      return this.tracer.startActiveSpan(
        `fetch ${url.pathname}${url.search}`,
        {
          kind: SpanKind.CLIENT,
          attributes: {
            [ATTR_HTTP_REQUEST_METHOD]: method,
            [ATTR_URL_FULL]: url.toString(),
          },
        },
        async (span) => {
          try {
            const res = await originalFetch.call(globalThis, input, init);
            span.setAttribute("http.response.status_code", res.status);
            if (res.status >= 400) {
              span.setStatus({ code: SpanStatusCode.ERROR, message: `HTTP ${res.status}` });
            }
            span.end();
            return res;
          } catch (err) {
            span.recordException(err);
            span.setStatus({ code: SpanStatusCode.ERROR, message: err.message });
            span.end();
            throw err;
          }
        },
      );
    };
  }
  disable() { if (this.originalFetch) globalThis.fetch = this.originalFetch; }
}
```

## Thread-local traceStore

Amp 没有用 OTEL exporter 外发，而是每个 `ThreadWorker` 自己维护一个 `traceStore`（strings line 63013）：

```js
this.traceStore = {
  startTrace:         (span)          => this.updateThread({ type: "trace:start",      span }),
  recordTraceEvent:   (span, event)   => this.updateThread({ type: "trace:event",      span, event }),
  recordTraceAttributes: (span, attrs) => this.updateThread({ type: "trace:attributes", span, attributes: attrs }),
  endTrace:           (span)          => this.updateThread({ type: "trace:end",        span }),
};
```

这些 delta 会流入 thread 数据、通过 WebSocket 发到 DTW，最终出现在 Cloudflare dashboard 上（见 `./debug-package.md`）。

## 把 OTEL 包成内部 tracer

`j7T(traceStore, parentSpan)` 函数（strings line 62347）把 traceStore 适配成 OTEL-like tracer：

```js
function j7T(traceStore, parentSpan) {
  return {
    startActiveSpan: async (name, options, callback) => {
      const spanID = $7T();   // 5 字符随机 ID
      traceStore.startTrace({
        name,
        label: options.label,
        id: spanID,
        parent: parentSpan,
        startTime: new Date().toISOString(),
        context: options.context ?? {},
        attributes: options.attributes,
      });
      const span = {
        id: spanID,
        addEvent: (message) => traceStore.recordTraceEvent(spanID, {
          message, timestamp: new Date().toISOString(),
        }),
        // setAttribute / setStatus / end 等，实现类似
      };
      // ... 调 callback，结束时 traceStore.endTrace(spanID)
    },
  };
}
```

## 自动包装的 span 位置

扫全部 `startActiveSpan(...)` 调用，列表（strings line 63013 / 63080 / 63081）：

| Span name | 上下文 | 位置 |
|---|---|---|
| `"tools"` | `messageId` | `toolOrchestrator.runTools()` 入口，包住一轮所有 tool 的并发执行 |
| `"inference"` | `messageId = currentAgentSpan?.messageId` | `runInference()` 每次 LLM 调用 |
| `"plugin"` + `label: "<plugin>#tool.call#<tool>"` | `plugin` name | 插件 `tool.call` hook 调度 |
| `"plugin"` + `label: "<plugin>#tool.result#<tool>"` | `plugin` name | 插件 `tool.result` hook |
| `"plugin"` + `label: "<plugin>#agent.start"` | `plugin` name | 插件 `agent.start` hook |
| `"plugin"` + `label: "<plugin>#agent.end"` | `plugin` name | 插件 `agent.end` hook |

其他 `startActiveSpan` 调用（strings line 64782 里的 `startActiveSpan(i, s, c, A)`）是 `@opentelemetry/api` 内部的 tracer 实现。

## Plugin tracer 暴露给插件

```js
getPluginTracer() {
  const parentSpan = this.currentSpan?.id ?? this.currentAgentSpan?.span;
  if (!parentSpan) return;
  return this.createTracer(parentSpan);   // 返回 j7T() 包装的 tracer
}
```

Plugin subprocess 通过 JSON-RPC 拿到这个 tracer 接口，可以在自己的 hook 里 `tracer.startActiveSpan(...)` 叠加子 span，串进主 trace 树里。

## CPU/Heap profile（Bun runtime flag，非 Amp 业务代码）

Amp 被打包进 Bun runtime。这些 flag 是 **Bun 自带的**（strings line 1462-1471），Amp 业务代码没直接暴露对应 option：

```
--cpu-prof
--cpu-prof-md                   # markdown 输出
--cpu-prof-name <name>
--cpu-prof-dir <dir>
--cpu-prof-interval <microseconds>   # 默认 1000 = 1ms
--heap-prof
--heap-prof-md
--heap-prof-name
```

要用的话，可以：
```sh
amp --cpu-prof --cpu-prof-md ...
```
会在当前目录生成 `CPU.<timestamp>.cpuprofile[.md]`。Amp 没专门暴露给用户这些 flag，但 Bun runtime 自动识别。

## 怎么用这些 trace 调试 Amp？

1. 运行 `amp debug` 命令，CLI 生成 `Debug Instructions` markdown（见 `./debug-package.md`）
2. 里面有 Cloudflare Logs URL（`WhT(threadID)`），跳过去能看到所有发到 DTW 的 span 事件
3. 同时 `~/.amp/logs/cli.log` 里有本地 log，通过 `_KT` transport 跟 span 绑定

## 对 Alva 的启发

Rust 有 `tracing` + `tracing-opentelemetry` 的一对组合，能做到和 Amp 几乎完全一样的模式：

### 推荐抄的 4 点

1. **`serviceName`+`AlwaysOnSampler`**：本地 dev / 内部工具不要担心采样成本，开全的
2. **自建 HTTP client instrumentation**：任何 LLM 请求都该自动变成 span（`reqwest-tracing` 可以用，但 Amp 那种"拦截 fetch 保持原 API"的思路在 Rust 里是 `reqwest` middleware chain）
3. **Span 命名层级**：`"agent.turn" > "inference" > "fetch …"` / `"agent.turn" > "tools" > "tool.<name>"` / `"agent.turn" > "plugin" > "<plugin>.<hook>"`。抄进 `agent-core` 的 hot path
4. **thread-local trace store**：不急着外发 exporter。先做好 in-memory 的 span tree，让 `alva debug` 命令能 dump 出来。等确认格式后再接 Jaeger / Tempo

### 具体在 `AnalyticsExtension` 里做什么

目前它是空壳。可以在 `configure()` 里：

```rust
async fn configure(&self, ctx: &ExtensionContext) {
    use tracing_subscriber::prelude::*;
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(open_log_file()?);
    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(init_tracer("alva.cli"));
    tracing_subscriber::registry()
        .with(EnvFilter::from_env("ALVA_LOG_LEVEL"))
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}
```

然后在 `agent-core` 的 inference / tool / plugin loop 加 `#[tracing::instrument(name = "inference", fields(message_id = %id))]`，免费拿到整棵 trace。

---

## 交叉引用

- 日志跟 span event 的绑定细节见 `./logging.md`
- Cloudflare Logs URL 怎么生成见 `./debug-package.md`
- trace 在 DTW 侧怎么存见 `../remote-runtime/dtw.md`

## 原始产物位置

- strings line 62347：`j7T` 自建 tracer
- strings line 63060：NodeSDK init + FetchInstrumentation
- strings line 63013：`traceStore` 定义
- strings line 63080/63081：plugin span 调用
- strings line 64782：OTEL api 内部实现
- strings line 1462-1471：Bun profile flags
