# alva-app-extension-loader
> Native-only AEP bridge for bounded, staged subprocess extension bundles.

## 地位

位于 L4 app extension 层。它把非 Rust 插件桥接为标准 `Plugin`，不得让子进程、安装或信任策略反向进入 kernel/agent-core。

## 逻辑

1. loader 发现并验证 `alva.toml`。
2. plugin bundle 在目录项、文件数、大小和深度限制内复制为 owner-only snapshot，拒绝 symlink/特殊文件并校验可选 SHA-256 pin。
3. subprocess 从 snapshot 启动，以限帧、限队列、限并发和有超时的 JSON-RPC/AEP 完成握手；握手身份/版本/能力/tools/subscriptions fail-closed，host reverse calls 按 manifest capabilities 授权。
4. proxy 将 remote tools 与 phase handlers 接入普通 Plugin 装配链。

## 约束

- Native-only；wasm target 保持空 crate。
- manifest entry 不得为绝对路径或包含父目录跳转。
- 子进程不得继承宿主 provider key、认证信息或任意环境变量。
- bundle 源目录仅用于快照，运行时只执行 owner-only staged copy。
- 源 bundle 必须通过 capability directory handle 相对遍历，文件与子目录打开后核对 inode/时间元数据，拒绝 symlink、跨根解析和检查后替换；AEP frame、stderr 转发、写队列、pending call 与 host callback 都必须有硬上限，callback task 必须随 dispatcher 退出而取消并回收。
- AEP capability 只授权 host API，不代表 OS 文件系统或网络沙箱。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| 源码 | `src/` | manifest、bundle staging、subprocess、dispatcher、proxy、loader 与 host API |
| 协议文档 | `docs/aep.md` | AEP wire contract、安全边界与冻结决策 |
| 集成测试 | `tests/` | handshake、loader、SDK smoke |
| 示例 | `examples/demo.rs` | 最小 AEP 插件演示 |
