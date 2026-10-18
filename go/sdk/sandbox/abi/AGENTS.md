# L0 · Sandbox ABI
> Go guest/host 共用的版本化、限额化纯值协议。

## 地位

仅依赖标准库与 sdk/llm/wire，不包含 runtime、provider/key、授权判断或 OS 路径。

## 逻辑

`alva_host.exchange(op,ptr,len)` 返回待取 response 字节数，guest 自分配后 `read_response(ptr,len)` 原子取走；一次仅允许一个待取响应，audit 单向无响应。所有流量为严格 UTF-8 JSON。

## 约束

- 版本 1 为新 Go 协议，不兼容 Rust ABI；未知字段/版本/union/限额都拒绝。
- DTO 不携带 host path、key、宿主 PermissionMode、批准结论或任意 import 注册能力。
- 原始引用不自动展开；显式 media 投递与 wire 保持一致。
- Context附件仅引用；只读reference读页≤4KiB、literal search≤20/256byte snippet，history≤20 metadata/ref且固定high-water。任何owner/session/path授权绑定都由真实host reader持有，不由guest请求声明。没有Store/DB API。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `types.go` | context/model/fetch/escalation/audit DTO 与检查 |
| `script.go` / `script_test.go` | 显式advertised隔离脚本请求/有界结果纯DTO；没有engine、权限、OS路径或存储政策 |
| `references.go` | 只读有界引用页/搜索与历史元数据索引 DTO/验证 |
| `codec.go` | 严格 JSON 与有界输出编码 |
| `types_test.go` | 版本/union/byte/UTF8与模型签名、usage保留 |
