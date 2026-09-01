# alva-app-extension-loader/src
> AEP subprocess extension discovery, staging, transport, dispatch and Plugin adaptation.

## 地位

本目录实现 app 层第三方插件宿主；不向稳定 SDK 下沉动态加载策略。

## 逻辑

`loader` → `plugin_bundle` → `proxy` → `subprocess`/`dispatcher`；proxy 通过 `aep_bridge` 和 `remote_tool` 接入 agent-core Plugin 装配，reverse host calls 由 `host_api` 执行 capability gate。

## 约束

- 所有 bundle 必须先验证、限额并 staged，禁止直接执行 discovery source。
- entry、manifest 和 bundle 内任意 symlink/特殊文件均 fail-closed；源树只经已打开 capability directory handle 相对遍历。
- subprocess 环境默认清空，只允许固定非敏感值与调用方显式 override。
- stdout/stdin frame、stderr 日志、dispatcher 队列、pending request、host callback 并发与所有等待远端响应的入口必须有界；callback task 由 dispatcher 拥有并在退出时取消、回收。
- initialize 响应必须与 manifest 的 identity/version/capabilities 一致，且只接受有界、唯一、宿主支持的 tools/subscriptions。
- JSON-RPC transport 不拥有 agent runtime 或 bus policy。

## 业务域清单

| 名称 | 文件 | 职责 |
|------|------|------|
| Crate 入口 | `lib.rs` | native-only 模块导出 |
| Manifest | `manifest.rs` | schema 与路径/能力/pin 校验 |
| Bundle | `plugin_bundle.rs` | capability-relative 源遍历、owner-only snapshot、限额、digest/pin |
| Loader | `loader.rs` | 多目录发现、排序、去重与 Plugin 注册 |
| Proxy | `proxy.rs` | staged bundle 生命周期、AEP handshake 合约校验、event/tool dispatch |
| Subprocess | `subprocess.rs` | secret-minimized child process 与 stdio/shutdown |
| Dispatcher | `dispatcher.rs` | bounded JSON-RPC 队列、pending 生命周期与受管 host callback task |
| Host API | `host_api.rs` | reverse call capability gate 与状态快照 |
| Phase bridge | `aep_bridge.rs` | AEP subscriptions → agent-core phase handlers |
| Remote tool | `remote_tool.rs` | AEP tool declaration → Tool adapter |
| Protocol | `protocol.rs` | AEP wire DTO 与错误码 |
