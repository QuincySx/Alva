# Shared native presentation
> direct 与 Hub 实际复用的聊天呈现与可信消息控件。

## 地位
产品外观能力，不属于 SDK 或执行 owner；各产品独立装配窗口、诊断和订阅。
## 逻辑
chat 提供状态/消息行/原生控件与当前 Inspector 页分类；modules 提供版本化纯数据 renderer。
## 约束
不得直接或传递依赖 direct、Hub、harness、host 执行或 engine；不得把展示缓存作为耐久会话或模型历史。MyGo 仅允许明确的 chat/modules 包，整个 ui 容器不自动获准。
## 业务域清单
| 路径 | 职责 |
|---|---|
| chat/ | 有界聊天呈现、原生输入/消息列表、只读 Inspector 分类 |
| modules/ | 版本化 table/chart renderer 与原文 fallback |
