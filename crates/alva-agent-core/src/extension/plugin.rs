//! Plugin — 装配期跨层捆绑包（取代 Extension）。
//!
//! `Plugin::register()` 每次装配调用一次，发生在 tools 与 model 定稿之前。
//! 需要读取别家 plugin 提供的能力时用 `Plugin::finalize()`（晚期跨插件接线）。

use async_trait::async_trait;
use std::sync::Arc;

use alva_kernel_abi::tool::Tool;

use super::registrar::{LateContext, Registrar};

/// 自包含的能力捆绑包，可向 [`Registrar`] 注册 tools / middleware /
/// bus 服务 / system-prompt 片段 / command。
///
/// 取代旧的 `Extension` trait（已全面迁移完成，无适配层）。
///
/// # 端到端示例
///
/// 一个完整 plugin：装配期注册 tool + middleware + 向 bus 提供能力，
/// 晚期（`finalize`）跨插件接线、动态发现 tool。
///
/// ```ignore
/// use std::sync::Arc;
/// use async_trait::async_trait;
/// use alva_agent_core::extension::{Plugin, Registrar, LateContext};
/// use alva_kernel_abi::tool::Tool;
/// use alva_kernel_abi::scope::context::ContextLayer;
///
/// struct MyPlugin;
///
/// #[async_trait]
/// impl Plugin for MyPlugin {
///     fn name(&self) -> &str { "my-plugin" }
///     fn description(&self) -> &str { "demo plugin" }
///
///     async fn register(&self, r: &Registrar) {
///         // 1. 注册 LLM 可调用 tool —— 取具体类型，无需手动 Box::new。
///         r.tool(MyTool::new());
///
///         // 2. 注册运行期洋葱中间件（以 Arc 传入，运行期共享所有权）。
///         r.middleware(Arc::new(MyMiddleware::new()));
///
///         // 3. 向 typed bus 提供一个能力，供别家 plugin / 运行期读取。
///         r.provide::<dyn MyService>(Arc::new(MyServiceImpl::new()));
///
///         // 4. 追加一段 system prompt（layer 决定缓存归属）。
///         r.system_prompt(ContextLayer::AlwaysPresent, "<my_context>…</my_context>");
///
///         // 5. 声明一个 /command（元数据）。
///         r.command("my-cmd", "do the thing");
///     }
///
///     async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>> {
///         // 此时所有 register() 都已完成：可读别家在 register 阶段提供的
///         // bus 能力，也可基于完整 tool 集合 / model 动态发现新 tool。
///         let _svc = cx.bus.get::<dyn MyService>();
///         // 晚期 tool 必须从这里返回（此阶段无 Registrar，调不到 r.tool）。
///         vec![Arc::new(MyLateTool::new())]
///     }
/// }
/// ```
#[async_trait]
pub trait Plugin: Send + Sync {
    /// 本 plugin 的唯一标识（用于日志与诊断）。
    fn name(&self) -> &str;

    /// 可选的人类可读描述。
    fn description(&self) -> &str {
        ""
    }

    /// 唯一装配阶段：注册 tools / middleware / bus 服务 / system prompt / command。
    ///
    /// **provide-only**：此处只“提供”能力，不要读取别的 plugin 提供的 bus 能力。
    /// 即使先注册的 plugin 的结果在实现上可能可见，也**不保证**装配顺序——
    /// 需要读别家能力的逻辑请放到运行期（middleware/tool 执行时）或
    /// [`finalize`](Self::finalize)。
    async fn register(&self, r: &Registrar);

    /// **晚期钩子** — 在所有 `register()` 调用结束、完整 tool 集合与 model
    /// 都已就绪后调用。用于动态 tool 发现 + 跨插件晚期接线（读别家在
    /// `register` 阶段提供的能力）。默认实现返回空 vec（无晚期 tool）。
    ///
    /// 返回值：晚期发现的 tool 以 `Arc<dyn Tool>` 返回（运行期 registry
    /// 持共享所有权，故为 `Arc` 而非 `register` 阶段的 `Box`）。此阶段没有
    /// `Registrar`，无法调 `r.tool()`，晚期 tool 只能从本方法返回。
    async fn finalize(&self, _cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        vec![]
    }
}
