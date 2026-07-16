# alva-worker-wasm guest source

> 最小 WASIp1 guest 的文件入口与 core-wasm LLM 代理 ABI。

## 地位

本目录只承载 Ticket 05 spike guest；生产 worker 的 agent loop 与协议尚未接入。

## 逻辑

`main.rs` 在 WASI 下读取任务、调用阻塞式宿主 import、接管宿主写回的分配区并落盘结果；在其他 target 下只输出平台提示以保持 workspace native 构建可用。

## 约束

- `alloc` 必须保持 C ABI 和未改名导出，供宿主从实例中查找。
- import 模块名与函数名必须和宿主测试接线完全一致。
- 从宿主返回的 buffer 必须由 guest 重新接管并释放。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| WASIp1 command | `main.rs` | 文件输入输出、`alloc` export 与 `llm_complete` import。 |
