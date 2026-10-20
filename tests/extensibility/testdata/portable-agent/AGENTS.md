# Portable agent consumer
> 同一入口在 native、JS/WASM、WASIp1 中消费普通能力。

## 地位
显式测试 fixture；不新增 Alva 产品命令、默认插件或真实交互通道。
## 逻辑
plan/question/schema 三种本地模式均通过公开 Builder.Tool 与默认 AgentLoop 真正调用工具，打印下一模型请求实际收到的 JSON。
## 约束
只有显式的确定性 fixture 模型/人类答案；不用 Plugin、数据库、引用存储或读取工具，不冒充真实模型/用户验收。失败非零退出，不截断或重放。
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | 独立消费者与三种明确测试模式 |
