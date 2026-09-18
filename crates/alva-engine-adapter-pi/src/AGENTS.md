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
- stream contract R2：转发 fatal `Error`（`recoverable: false`）后**立即停止读取循环**，只补 `Completed`。判定用 `alva_engine_runtime::is_fatal_error`，不要在本 crate 重写。
- R95 暂时性错误结算：Pi 0.85.1 真实时序是 `message_update(error)` → `message_end(stopReason="error")` → `agent_end{willRetry}` → `auto_retry_*`。assistant 错误帧只是**待定状态**（`mapper.pending_error`），不外发；定性点在 `agent_end{willRetry:false}`（携带未恢复错误 → 唯一 fatal）或 `auto_retry_end{success:false}`（重试耗尽 → 唯一 fatal）。成功 assistant 消息清除待定错误（重试恢复，无 fatal）。错误后 `agent_end{willRetry:true}` 即 EOF 仍按缺终态 outage 报 fatal。
- R94 终态结算：`agent_end{willRetry:false}`（原生终态）提交后，尾部传输失败与迟到 cancel 只结束读取、不产生 fatal，已提交结果原样保留；终态之前的读取失败/取消仍按 fatal + `result=None` 结算。禁止"fatal Error + 成功答案"并存。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| PiAdapter | adapter.rs | EngineRuntime 实现与 session kill map |
| PiAdapterConfig | config.rs | binary、模型、Provider、session 目录和工具策略 |
| PiProcess | process.rs | 子进程、临时 models.json、JSONL 读取与 kill |
| PiEvent | protocol.rs | Pi JSON event 的最小稳定线格式 |
| EventMapper | mapping.rs | Pi JSONL 到 RuntimeEvent |
