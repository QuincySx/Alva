# L4 · Optional telemetry
> 可替换的无正文诊断编码与保存政策。

## 地位
普通 Plugin；SDK 只给通用模型/流/hook 观察值，不认识指纹算法、持久化 schema 或日志输出政策。

## 逻辑
Plugin 以 `kernel/core.DiagnosticObserver` 类型贡献可选 Bus 能力，实际消费者为 DefaultAgentLoop。
缺失时不写诊断记录；harness 默认安装，可用同名 Plugin 替换。模型输入先由执行机制验证，
诊断失败经显式 reporter 报告后继续；默认 logger 只报固定操作及会话，不复制私有错误正文。
指纹保持 version 1：稳定 system/model/有序 schemas 与动态 prepared system 分开；流指标每响应合批。

## 约束
仅标准库与 SDK。无 token/prompt/result 正文；不拥有授权、重试或执行事实。
真实 intent/outcome、已提交操作的恢复证据、会话 Flush 失败仍须中断。
Observer/report 回调须有界并支持调用方 context；没有 detached goroutine 或后台持久化 worker。
Plugin 不拥有另一个数据库，也不更改原始执行记录。

## 业务域清单
| 文件 | 职责 |
|---|---|
| plugin.go | 可选能力贡献、单次阶段写入与显式失败报告 |
| records.go | 指纹、cache presence 和有界流记录编码 |
| records_test.go | 稳定/动态指纹、坏 JSON、实际 identity 与 cache 未报告/零值区别 |

真实 SDK hook/流/故障回归在 `../../sdk/agent/core/`，由测试显式安装本 Plugin；纯 SDK 无诊断默认项。
