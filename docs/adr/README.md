# Architecture Decision Records

当前实现的技术栈与边界决策见 [ADR 0012](./0012-go-native-stack.md)：
Go、MyGo 自绘、独立 SDK/扩展/harness/产品、无历史兼容桥。
[ADR 0013](./0013-module-freeze-and-v1-scope.md) 修订其中两处，并记录 v1 范围、中间窗口内容块、
长会话验收线以及经批准的内核/SDK 与 Hub interface 收窄；落地顺序见 [ROADMAP](../ROADMAP.md)。
[ADR 0014](./0014-alva-kit-component-library.md) 决定在 `apps/ui/kit` 建立自有组件库 Alva Kit（设计 token 单一来源、
画廊与覆盖闸、分层与采用闸），修订 COMPONENTS.md 中“不再发布独立 UI 包”的说法。
2026-10-18 用户明确授权先提交 Rust 完整快照，再删除旧实现并将 Go 转正。

Rust 时代的 ADR、产品规范和实现记录已归档到 `archive/rust-final-2026-10-18`，
不再列作当前源码地图。可用 `git show archive/rust-final-2026-10-18:docs/adr/README.md`
查看完整旧索引；领域/所有权约束继续体现在当前 Go 接口与目录文档中。

当前架构见 [ARCHITECTURE](../ARCHITECTURE.md)，实际能力/平台验收见
[MIGRATION.md](../../MIGRATION.md)。归档与转正不自动宣称所有目标平台或产品场景已验收。
