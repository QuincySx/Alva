# Go SDK public surface
> 稳定SDK接口变化的显式审核快照。

## 地位
不编译、不运行；由architecture测试读取。

## 逻辑
包含导入身份、导出类型/字段/tag、泛型约束、接口封口、函数/receiver与常量组。

## 约束
仅显式 ALVA_UPDATE_GO_SURFACE=1 的机械更新；同一改动保留测试与职责审查。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| wire | llm-wire.txt | 消息/媒体/模型配置/流累积值契约 |
| bus | kernel-bus.txt | typed能力/独立有界通知，不包含另一套装配生命周期 |
| kernel ABI | kernel-abi.txt | 最小循环词汇、运行事实与执行上下文 |
| agent ABI | agent-abi.txt | 可选Task/Scope/Spawn/phase/provider/analytics |
| agent core | agent-core.txt | Plugin/Registrar/Finalizer/Builder与快照 |
