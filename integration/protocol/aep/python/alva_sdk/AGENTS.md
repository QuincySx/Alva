# alva_sdk
> Plugin-side Python API and AEP wire runtime.

## 地位
Optional Python client, not a Rust implementation or the Go kernel.
## 逻辑
Decorators declare handlers/tools, Plugin builds explicit actions, runtime dispatches stdio JSON-RPC and reverse callbacks.
## 约束
Host owns authority and cleanup. No ambient discovery or automatic installation; retain original body behavior during relocation.
## 业务域清单
| 文件 | 职责 |
|---|---|
| __init__.py | Public author imports |
| decorators.py | Event/tool declarations |
| extension.py | Plugin, typed payloads and explicit host helpers |
| runtime.py | Plugin-side stdio JSON-RPC lifecycle |
