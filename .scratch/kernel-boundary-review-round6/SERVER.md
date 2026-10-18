# Round 6 HTTP/run seam

固定 HEAD `5290e033`。主审读取正式 server projector、Stop handler、desktop run seam。

## S1 / P1 / R51：取消仍过早结算且存在跨轮竞态

`crates/alva-hub-server/src/main.rs:925–965` 把 Failed 当作“engine stream really ended”。但 `crates/alva-hub-desktop/src/run.rs:992–1012` 收到 fatal RuntimeEvent::Error 时先发 Failed，继续读流到 Completed（1015–1048）/EOF 才完成。因此已 ack Stop 遇到 Error 时可提前发 Interrupted。随后 callback 又无条件发原 Failed/Completed，一个取消结算变成相邻的互相冲突状态。`record_attempt_cancelled` 的错误也被忽略；它只写取消事实，不能声称它本身修改 session status。

Stop handler `main.rs:598–608` 先 await cancel 后插入 stopping。若终态在 await 返回前发出，projector 看不到 marker；之后 marker 残留并可能应用到同 session 后继轮。应以 attempt/运行代际和真正结束信号建模，并让 HTTP/SSE 集成覆盖 ack/终止的两种顺序。不要只换一个 matches 分支。

动态探针使用正式 `run_persisted_with_target` 和逐字复制的当前 server projector body：假引擎先 Started，gate 后 fatal Error，再 gate 后 Completed；在第二 gate 尚未释放时检查取消终态。复制正文归档 `evidence/server-projector-source.txt`。它验证 run/projector seam，**不是**真实 Axum HTTP/SSE 集成；下一卡仍要求真实 HTTP 验收。

## S2 / P2 / R51：启动失败泄漏 ActiveHubs 条目

`crates/alva-hub-desktop/src/run.rs:664–693` 新增在 execute 前发布 sink；execute Err 直接 return，绕过正常尾清理。静态路径明确，失败会话仍保留已无运行的 Hub。探针调用正式公开入口，注入同步 execute Err，要求返回后 sink 为空。

专项未执行：首次 Cargo 锁等待超过 11 分钟，重试仍等待 package cache，已中止自己的进程。源码归档 `evidence/server-projection.rs`，日志 `evidence/server-probe.txt`。S1/S2 以静态代码路径确认，不计入动态复现数量。
