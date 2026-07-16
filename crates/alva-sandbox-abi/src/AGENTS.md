# alva-sandbox-abi source
> Sandbox host/guest 共用的纯 serde 能力协议。

## 地位

本目录承载可同时编译到 native 与 WASIp1 的 L0 DTO，不包含任何能力实现。

## 逻辑

`lib.rs` 定义 fetch / host escalation 请求响应、审计事件、body base64 wire 编码及两侧共用的版本/大小上限；升级 `cwd` 只表达 guest 路径。

## 约束

- 只允许纯值类型与确定性构造/版本检查。
- 不解析 URL、不匹配域名、不翻译 cwd、不审批或执行命令。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| capability wire | `lib.rs` | fetch / escalation 双向与 log 单向 ptr/len JSON ABI。 |
