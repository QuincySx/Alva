# alva-worker-wasm

> Ticket 05 spike 的最小 WASIp1 worker guest，用来证实阻塞式 LLM ptr/len ABI。

## 地位

`alva-worker-wasm` 是 WASIp1 command guest，不是宿主 runner，也不是完整 agent worker。它加入主 workspace，让 CI 与开发者能直接用 `-p alva-worker-wasm` 编译；非 WASI 目标只保留一个可编译的提示入口。

## 逻辑

1. guest 从 preopen 的 `/work/task.txt` 读取任务字节。
2. guest 调用 `alva:host/llm::llm_complete(req_ptr, req_len)`，同步取得打包的响应指针和长度。
3. 宿主调用 guest 导出的 `alloc` 分配响应缓冲区并写回线性内存。
4. guest 解包响应并写入 `/work/result.txt`。

## 约束

- 这是 spike 垂直切片，不包含 agent loop、provider、CLI 参数或错误协议。
- guest 不包含 API key、provider 配置或任何宿主 secret。
- ABI 当前只接受长度可装入 `i32` 的请求/响应，错误由 trap 表达。
- 生产化前必须把裸字节约定版本化，并定义结构化错误与资源上限。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 配置 | `Cargo.toml` | 声明 workspace 内无依赖的 WASIp1 command binary。 |
| guest 源码 | `src/` | 实现 `/work` 文件流和阻塞式 ptr/len LLM import。 |
