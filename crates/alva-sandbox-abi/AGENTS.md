# alva-sandbox-abi
> 零 workspace 依赖、wasm-clean 的 sandbox guest/host 共享能力 ABI。

## 地位

本 crate 是 L0 纯值协议层，位于 native sandbox host 与 WASIp1 guest 之间；不实现网络、策略或 runtime。

## 逻辑

1. guest 构造版本化 fetch 请求并经 ptr/len import 发送给宿主。
2. 宿主返回版本化结果信封；HTTP 成功携带状态、头和有界 body，策略/传输失败携带可恢复错误文本。
3. body 在 wire 上用 base64 避免 JSON 数字数组放大；两侧共享请求、响应和 JSON/body 大小上限，避免协议漂移与 guest 内存失控。

## 约束

- 不得依赖其他 workspace crate、HTTP client、runtime、provider 或 credential；仅允许 serde/base64 这类 wire 依赖。
- ABI 版本或限额变更必须同时覆盖 host 与 guest 测试。
- DTO 只表达 wire 数据；域名白名单及重定向策略只能由宿主执行。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 配置 | `Cargo.toml` | 声明 serde-only 依赖。 |
| fetch ABI | `src/lib.rs` | 版本、限额、请求/响应 DTO 与可恢复结果信封。 |
