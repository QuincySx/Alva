# L4 · AEP runtime fixtures
> Explicit local author-SDK interoperability inputs, not discovered plugins.

## 地位
Test-only code executed with an explicit installed Python or Node and the checked-in author SDK, copied into a pinned private bundle.
## 逻辑
Fixture exercises real SDK decorators, reverse state/log callbacks and message/tool mutations.
## 约束
No user code, third-party packages, credentials or global config; Python site/user imports are disabled.
Tests disable bytecode generation so importing the SDK does not write into the source tree.
## 业务域清单
| 文件 | 职责 |
|---|---|
| sdk_fixture.py | Real alva_sdk AEP event/tool protocol fixture |
| sdk_fixture.mjs | Real JavaScript SDK事件/工具、异步宿主回调与进入后取消fixture |
