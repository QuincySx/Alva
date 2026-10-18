# Cindy 产品文档包

这是 Cindy Desktop `0.1.45` 与开源提交 `240247e52e938710f409c921a7a393097dcef52b` 的产品参考包。两者是并列证据基线；安装包 build 与提交的精确映射尚待动态验证。

## 从这里开始

- [全量产品参考与任务蓝图](/Users/smallraw/.cache/cindy-deep-dive/product-docs/CINDY-PRODUCT-REFERENCE.md)：主入口、跨文档裁决口径、核心对象、旅程、不变量、路线图、任务模板和 DoD。
- [01 产品战略与信息架构](/Users/smallraw/.cache/cindy-deep-dive/product-docs/01-product-strategy-and-ia.md)
- [02 UX/UI 产品规格](/Users/smallraw/.cache/cindy-deep-dive/product-docs/02-ux-and-ui-spec.md)
- [03 用户流程与状态矩阵](/Users/smallraw/.cache/cindy-deep-dive/product-docs/03-user-flows-and-state-matrix.md)
- [04 全功能产品需求](/Users/smallraw/.cache/cindy-deep-dive/product-docs/04-feature-requirements.md)
- [05 领域状态、一致性与安全策略](/Users/smallraw/.cache/cindy-deep-dive/product-docs/05-domain-state-and-policy.md)
- [06 系统与集成蓝图](/Users/smallraw/.cache/cindy-deep-dive/product-docs/06-system-and-integration-blueprints.md)
- [07 产品缺口、路线图与验收体系](/Users/smallraw/.cache/cindy-deep-dive/product-docs/07-gap-roadmap-and-acceptance.md)

## 建议用法

- 做产品规划：先读主参考、01、07。
- 做交互设计：读 02、03，再用 07 建验收矩阵。
- 做功能开发：从 04 的领域需求进入，使用 05 的状态/策略和 06 的系统蓝图约束实现。
- 做测试发布：从 03 的流程矩阵和 07 的风险 ID、测试环境、发布门禁出发。
- 拆任务：直接复制主参考第 15 节任务模板，不按“页面/IPC/表”把同一端到端不变量拆散。

## 证据资产

- [审计总入口](/Users/smallraw/.cache/cindy-deep-dive/README.md)
- [43 张实机状态 manifest](/Users/smallraw/.cache/cindy-deep-dive/ui/manifest.jsonl)
- [43 张原始截图](/Users/smallraw/.cache/cindy-deep-dive/screenshots/original)
- [43 张同名线稿](/Users/smallraw/.cache/cindy-deep-dive/screenshots/lineart)
- [源码分析目录](/Users/smallraw/.cache/cindy-deep-dive/source-analysis)

## 完整性快照

- 8 份正文，共 4,894 行；另有本索引。
- 共 90 张 Mermaid 产品/流程/状态/系统蓝图。
- 43/43 manifest、原始 PNG、原始 JPEG、线稿一一对应。
- Markdown 围栏闭合，Mermaid 块类型头可识别，本地绝对链接缺失数为 0。
- Cindy 源码仓库保持 clean；本次没有修改应用源码。

事实、建议和动态待验严格分开。遇到文档冲突时，遵循主参考第 3 节的证据与裁决规则。
