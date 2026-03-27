# Provider Port
> 应用核心层的 Provider 端口定义，声明模型提供者的抽象接口与注册表

## 地位
alva-app-core 六边形架构的出端口（outbound port）之一。定义 `Provider` trait 和 `ProviderRegistry` trait，供适配器层（adapters）实现具体的 AI 模型提供者（如 OpenAI、Anthropic 等）。实际 trait 定义位于 alva-types crate 中，本模块通过 re-export 统一暴露给 app-core 内部使用。

## 逻辑
- **Provider** — 模型提供者 trait，通过 `id()` 标识自身，通过 `language_model(model_id)` 返回具体的 `LanguageModel` 实例。
- **ProviderRegistry** — 提供者注册表 trait，管理多个 Provider 的注册与查找，支持按 provider_id 获取 Provider。
- **ProviderError** — 提供者相关错误类型，从 alva-types re-export。
- **types** — 占位模块，V4 版本的 ProviderOptions/ProviderMetadata 等类型已移除，模型能力 trait 现定义在 alva-types 中。

## 约束
- 本模块是 re-export 薄层，实际 trait 定义在 alva-types 中，修改接口需去 alva-types 操作
- 新增 Provider 实现必须放在 adapters 层，不可在 ports 层包含具体实现
- provider_registry.rs 中包含的 `#[cfg(test)]` MockModel/MockProvider 仅供本模块单元测试使用
- types.rs 当前为空占位，未来如需 provider 级共享类型可在此添加

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | mod.rs | 模块入口，re-export errors、Provider、ProviderRegistry |
| errors | errors.rs | 错误类型：re-export alva_types::ProviderError |
| provider_registry | provider_registry.rs | re-export Provider trait 与 ProviderRegistry trait，附带单元测试 |
| types | types.rs | 占位模块，预留 provider 级共享类型 |
