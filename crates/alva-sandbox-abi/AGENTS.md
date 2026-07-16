# alva-sandbox-abi
> 零 workspace 依赖、wasm-clean 的 sandbox guest/host 共享能力 ABI。

## 地位

本 crate 是 L0 纯值协议层，位于 native sandbox host 与 WASIp1 guest 之间；不实现网络、策略或 runtime。

## 逻辑

1. guest 构造版本化 fetch 或 host escalation 请求并经 ptr/len import 发送给宿主；宿主通过独立 context import 向 guest 下发已解析的环境 prompt。
2. 宿主返回版本化结果信封；环境 context 携带纯 prompt 文本，HTTP 成功携带状态/头/body，升级成功携带 stdout/stderr/exit_code，策略/传输失败携带可恢复错误文本。
3. escalation `cwd` 始终是 guest 视角；DTO 不携带 host path、PermissionMode 或审批结论，翻译与策略只属于宿主。
4. guest 以独立、有界 log import 上报无时间戳/路径的审计事件，宿主决定是否及如何持久化。
5. body 在 wire 上用 base64 避免 JSON 数字数组放大；两侧共享 context、请求、响应和 JSON/body 大小上限，避免协议漂移与 guest 内存失控。

## 约束

- 不得依赖其他 workspace crate、HTTP client、runtime、provider 或 credential；仅允许 serde/base64 这类 wire 依赖。
- ABI 版本或限额变更必须同时覆盖 host 与 guest 测试。
- DTO 只表达 wire 数据；域名白名单、重定向、cwd 翻译、审批与命令执行策略只能由宿主执行。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 配置 | `Cargo.toml` | 声明 serde-only 依赖。 |
| capability ABI | `src/lib.rs` | host→guest environment context、fetch / escalation 请求响应与 guest→host 审计事件的版本化、有界 DTO。 |
