# 横向支撑 · WASIp1 probe
> 用实际 WASI syscall 验证授权和资源反例。

## 地位

只由 sandbox host tests 显式编译运行，不被产品注册。

## 逻辑

根据显式 argv 触发有限文件/环境/fetch/escalation/资源测试模式，任务值经 context import 接收。

## 约束

不得加入真实模型/ambient key 或宿主 shell；spin/memory/output 模式预期被宿主真实限制。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `main.go` | 实际 guest probe 命令 |
