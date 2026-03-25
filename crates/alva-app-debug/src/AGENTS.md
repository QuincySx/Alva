# alva-app-debug
> 调试服务器，提供 HTTP API 查看日志、调整日志级别和检查 GPUI 视图树

## 地位
开发调试基础设施。在 debug 构建中提供 HTTP 服务，支持实时日志查看、日志级别动态调整、GPUI 视图树检查。通过 `traced!` / `traced_listener!` 宏为 GPUI 事件处理器添加零开销追踪（release 构建完全消除）。

## 逻辑
```
DebugServerBuilder → DebugServer → DebugServerHandle
                          │
                          ├─→ router.rs (HTTP 路由)
                          │     ├─→ GET  /health
                          │     ├─→ GET  /logs          (查询日志)
                          │     ├─→ PUT  /logs/level    (调整日志级别)
                          │     └─→ GET  /inspect/tree  (GPUI 视图树快照)
                          │
                          ├─→ LogCaptureLayer (tracing Layer)
                          │     └─→ LogStore (环形缓冲区存储)
                          │
                          └─→ gpui/ViewRegistry
                                └─→ GpuiInspector (视图树构建 & 快照)

traced! / traced_listener! 宏
  └─→ debug 构建：注入 tracing::info 调用
  └─→ release 构建：编译为原始 handler（零开销）
```

## 约束
- 仅在 `#[cfg(debug_assertions)]` 下启用完整功能
- LogStore 使用环形缓冲区，固定容量，旧日志自动丢弃
- ViewRegistry 线程安全（parking_lot::RwLock），支持动态注册/注销
- traced! 宏仅支持 3 参数闭包（GPUI event handler），traced_listener! 支持 4 参数闭包（cx.listener handler）

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| 服务器构建 | `builder.rs` | DebugServerBuilder / DebugServer / DebugServerHandle |
| HTTP 路由 | `router.rs` | 路由定义（/health、/logs、/logs/level、/inspect/tree） |
| HTTP 服务 | `server.rs` | HTTP 服务器启动与运行 |
| 日志捕获层 | `log_layer.rs` | LogCaptureLayer（tracing Layer 实现）、LogHandle |
| 日志存储 | `log_store.rs` | LogStore 环形缓冲区、LogQuery、LogRecord |
| 视图检查 | `inspect.rs` | InspectNode、Inspectable trait、Bounds、DebugInspect |
| GPUI 集成 | `gpui/mod.rs` | ViewRegistry / ViewEntry / GpuiInspector — 视图树构建 |
| 追踪宏 | `traced.rs` | traced! / traced_listener! — 零开销 GPUI 事件追踪宏 |
