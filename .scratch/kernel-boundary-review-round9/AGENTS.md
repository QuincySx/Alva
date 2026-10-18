# Round 9 review
> 2026-09-18 对 R72–R75、R80–R83、R90 新实现的固定范围复审。

## 地位
本地 review，不修改产品实现；区分 Agent SDK、多引擎衔接和 Hub 产品。
## 逻辑
REVIEW 汇总规范/需求两轴，TASKS 列新增卡和旧卡补验，evidence 保存反例。
## 约束
基线 c8aca9c8，审查 HEAD bae66d0f；不把未实施的旧卡重复算新发现；静态/动态证据分开。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 入口 | REVIEW.md / TASKS.md | 复审结论、范围和执行队列 |
| Adapter 专项 | ADAPTERS-EXTRA.md | 本机 Pi 版本时序依据与适配器边界 |
| 卡片 | issues/ | 新增有限修复卡 |
| 证据 | evidence/ | 当前真实实现的独立探针 |
