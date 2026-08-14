# Provider Plane
> 将模型声明与 Provider/Engine 路由规则收敛成无 I/O、可版本化的产品策略层。

## 地位

- 位于 Hub Desktop L7 产品层，不进入稳定 SDK kernel 或 agent-core。
- 上游由 `providers.rs` 提供已解析的 Provider/Model 配置事实。
- 下游只产出确定性的 engine route 与模型控制能力，不拥有网络、密钥或持久化。

## 逻辑

1. `ModelRegistry` 按版本维护产品确认过的模型声明、canonical identity 与 per-engine override。
2. `RoutePlanner` 校验显式 registry binding，并将协议兼容矩阵、模型声明和 engine 约束求交集。
3. 远端发现只能产出 non-authoritative binding preview；用户确认并保存后由后端再次校验。
4. `ProviderCatalog` 消费规划结果，叠加连接启用状态、凭证和 endpoint 健康事实后投影公开 DTO。

## 约束

- 未知模型不得依据名称猜测 reasoning、Fast 或其它能力。
- CLI 订阅只允许各自原生 engine；第三方 API route 按 wire protocol 扇出。
- 本目录禁止文件系统、网络、环境变量和全局可变状态。
- 新远端发现结果必须作为独立输入快照接入，不能静默改写 bundled registry。
- 手工未知模型保持 custom；前端不能通过伪造 `registry_id` 获得 reasoning 或 Fast 能力。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Module seam | `mod.rs` | 公共模型选项 DTO 与内部模块边界 |
| Model registry | `model_registry.rs` | 版本化模型身份、默认能力与 per-engine override |
| Route planner | `route_planner.rs` | 显式 binding 校验、发现预览及 Provider × Model × Engine 纯函数规划 |
