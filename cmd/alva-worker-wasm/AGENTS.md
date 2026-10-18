# L6 · WASIp1 agent command
> 唯一 worker 入口调用 sandbox/guest 的实际 SDK 循环。

## 地位

只为 GOOS=wasip1 GOARCH=wasm 构建，native 入口明确报错，不偷偷降级原生执行。

## 逻辑

任务/环境仅经版本化宿主 context import；完成后输出最终 assistant 文本，失败返回非零并写错误。宿主默认有界捕获 stdout/stderr。

## 约束

不发现 key/provider/环境；无宿主真实路径，无复制内核，无 Rust compatibility ABI。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `main_wasip1.go` | 真 guest 启动 |
| `main_other.go` | 错误目标显式拒绝 |
