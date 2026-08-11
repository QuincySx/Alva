# alva-engine-adapter-pi
> Earendil Works Pi coding-agent 的 EngineRuntime 适配器（`pi --print --mode json`）。

## 地位
Hub multi-engine 的 Pi 后端。子进程和 JSONL 映射都留在本 crate；不依赖
`alva-app-core`，也不改变 Pi 自己的 session 格式。

## 逻辑
1. `PiAdapter::execute` 为每个 Hub execute 启动一个 Pi print-mode 子进程。
2. `--session-id` + `--session-dir` 保持 Pi 原生会话连续性。
3. stdout 的 session header、message/tool/retry 事件映射为 `RuntimeEvent`。
4. custom Provider 通过临时 owner-only `models.json` 注册，API key 只进入环境变量。
5. `cancel` kill 子进程；Pi print mode 不提供交互式 permission callback。

## 约束
- 依赖宿主安装 `pi`。
- workspace-write 映射为只读工具 allowlist，宁可更严格，不得伪装成等价沙箱。
- custom Provider 密钥不得进入 argv、日志或临时 `models.json`。
- stdout EOF 后 adapter 统一补唯一 `Completed`。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| PiAdapter | adapter.rs | EngineRuntime 实现与 session kill map |
| PiAdapterConfig | config.rs | binary、模型、Provider、session 目录和工具策略 |
| PiProcess | process.rs | 子进程、临时 models.json、JSONL 读取与 kill |
| PiEvent | protocol.rs | Pi JSON event 的最小稳定线格式 |
| EventMapper | mapping.rs | Pi JSONL 到 RuntimeEvent |
