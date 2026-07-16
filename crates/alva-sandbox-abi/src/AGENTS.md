# alva-sandbox-abi source
> Sandbox host/guest 共用的纯 serde 能力协议。

## 地位

本目录承载可同时编译到 native 与 WASIp1 的 L0 DTO，不包含任何能力实现。

## 逻辑

`lib.rs` 定义 fetch 请求、成功响应、错误结果信封、body base64 wire 编码及两侧共用的版本/大小上限。

## 约束

- 只允许纯值类型与确定性构造/版本检查。
- 不解析 URL、不匹配域名、不执行网络。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| fetch wire | `lib.rs` | fetch ptr/len JSON ABI。 |
