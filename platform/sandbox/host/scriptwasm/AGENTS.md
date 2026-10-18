# WASM · Isolated QuickJS engine asset
> Fresh scripting instance, independent from the long-lived SDK worker.

## 地位
Private host implementation asset, not native FFI, an SDK dependency or a JS desktop stack.
## 逻辑
Pinned QuickJS-NG v0.17.0 core sources (without quickjs-libc) compile to a WASIp1 reactor. bridge.c installs only explicit synchronous host bindings. build.py verifies the source archive checksum and uses an explicit WASI SDK clang. Ordinary Go builds embed the checked binary and need no C/Node/npm tools.
## 约束
No std/os modules, module loader, process, filesystem mount, ambient environment or credentials. Per instance32MiB tracked heap,512KiB soft C stack and64MiB WASM linear memory; host owns10-second deadline, actual cancellation, shared job fuel and every file/network operation. The upstream WASI stack guard is used directly with a2MiB linker stack reservation; no QuickJS source patch is applied. Ordinary script errors leave the Agent worker alive. No automatic download/build on startup. The source/bridge/bootstrap hashes and binary provenance must match; both build and runtime provenance tests keep the core asset within1MiB.
## 业务域清单
| 文件 | 职责 |
|---|---|
| bridge.c | Core-only bounded QuickJS eval, accurate allocation accounting and synchronous JSON capability imports |
| bootstrap.js | Fixed synchronous file/JSON/print/fetch wrappers over explicit C capability functions |
| build.py | Checksum-pinned source, explicit WASI clang and reproducible reactor build |
| quickjs.wasm / provenance.json | Embedded immutable engine and exact source/toolchain/build/binary provenance |
| LICENSE.quickjs | Unmodified upstream MIT copyright and permission notice |
