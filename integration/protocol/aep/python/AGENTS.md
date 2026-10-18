# AEP Python author client
> Optional plugin-side wire client for the maintained Go host.

## 地位
Cross-language author SDK outside the portable Go SDK. No Rust or Go-product execution dependency.
## 逻辑
Preserves the existing Python AEP client, typed wrappers and decorators. Loader tests execute an explicit checked-in fixture through the actual Go protocol and native Plugin path.
## 约束
No automatic install, discovery, account/config import or startup. Host approval, capability ceilings, cancellation and durable authority remain in Go. Preserve original MIT packaging declaration; no protocol or behavior is broadened by relocation.
## 业务域清单
| 路径 | 职责 |
|---|---|
| alva_sdk/ | Typed actions, decorators and plugin-side JSON-RPC runtime |
| examples/ | Explicit author examples |
| pyproject.toml | Original standalone Python package metadata, no runtime dependencies |
| README.md | Local use and current Go host support |
