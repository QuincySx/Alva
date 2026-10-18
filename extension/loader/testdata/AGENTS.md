# L4 · AEP runtime fixtures
> Explicit local author-SDK interoperability inputs, not discovered plugins.

## 地位
Test-only code executed only with the installed system Python and the explicit integration/protocol/aep/python author SDK path.
## 逻辑
Fixture exercises real SDK decorators, reverse state/log callbacks and message/tool mutations.
## 约束
No user code, third-party packages, credentials or global config; Python site/user imports are disabled.
Tests disable bytecode generation so importing the SDK does not write into the source tree.
## 业务域清单
| 文件 | 职责 |
|---|---|
| sdk_fixture.py | Real alva_sdk AEP event/tool protocol fixture |
