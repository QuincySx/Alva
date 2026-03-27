# alva-test
> 项目级测试基础设施 crate，提供 Mock 对象和工厂方法，供其他 crate 的单元/集成测试使用

## 地位
开发时依赖（dev-dependency）。为 alva-types 中定义的核心 trait（LanguageModel、Tool）提供可配置的 Mock 实现，以及常用 Message 固定数据的工厂函数。所有需要测试 Agent 循环、工具调用、Provider 交互的 crate 都应依赖此 crate 而非自行编写 Mock。

## 逻辑
- **MockLanguageModel** — 通过 builder 模式队列化预设响应（成功/错误），支持流式事件注入，自动记录每次 `complete` 调用的参数以供断言。内部使用 `Arc<Mutex>` 确保克隆后状态共享。
- **MockTool** — 通过 builder 模式配置预设 ToolResult 或错误返回，记录每次 `execute` 的输入 JSON。
- **fixtures** — 工厂函数快速构造 `Message`（user/assistant/tool_call），自动生成 UUID。
- **assertions** — 领域级断言辅助（预留模块）。

## 约束
- 仅作为 `[dev-dependencies]` 使用，禁止在生产代码中引用
- Mock 对象的 builder 方法必须返回 `self`（消费型 builder），保持链式调用风格
- 所有 Mock 内部状态使用 `Arc<Mutex>` 包裹，因为 trait 方法签名为 `&self`
- 新增 Mock 类型时需在 lib.rs 中声明并 pub mod 导出

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| lib | lib.rs | 模块入口，声明并导出所有子模块 |
| mock_provider | mock_provider.rs | MockLanguageModel — LanguageModel trait 的测试替身，支持响应队列与调用记录 |
| mock_tool | mock_tool.rs | MockTool — Tool trait 的测试替身，支持预设结果与调用记录 |
| fixtures | fixtures.rs | Message 工厂函数：make_user_message、make_assistant_message、make_tool_call_message |
| assertions | assertions.rs | 领域级断言辅助函数（预留） |
