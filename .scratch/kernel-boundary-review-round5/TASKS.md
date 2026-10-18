# 第五轮返工清单

覆盖八套清单106张ticket，最新diff为 `07b75f8...5c64af1`。本清单24张卡R38–R61承接确认缺陷与明确未完成验收；保留已完成部分。

| 卡 | 优先级 | 轴 | 标题 | 状态 |
|---|---|---|---|---|
| [R38](issues/R38.md) | P1 | Spec | 修复非 Tokio 广播回调重入死锁 | open |
| [R39](issues/R39.md) | P2 | Spec | 让弃置会话唤醒并回收空闲广播 driver | open |
| [R40](issues/R40.md) | P1 | Spec | 仅对助手正文保留终态合并内容 | open |
| [R41](issues/R41.md) | P2 | Spec | 修复 Markdown 文本双转义并补阅读行为验证 | open |
| [R42](issues/R42.md) | P2 | Spec | 把执行概览迁入 Inspector 并接通工具定位 | open |
| [R43](issues/R43.md) | P2 | Spec | 按真实引擎能力描述敏感路径保护 | open |
| [R44](issues/R44.md) | P2 | Spec | 移除个人资料假保存与取消不回滚 | open |
| [R45](issues/R45.md) | P2 | Spec | 补齐真实产品与审批验收并纠正关闭口径 | open |
| [R46](issues/R46.md) | P3 | Standards | 同步真实职责文档与逐卡证据映射 | open |
| [R47](issues/R47.md) | P3 | Standards | 将新增聊天间距和圆角归入设计 tokens | open |

顺序：R38/R39 协调同一广播实现；R40/R41/R42 为不同 UI 行为修复；R43/R44 独立。R45 在相关修复后补验收，R46 最后收口文档与证据，R47 只收口新增样式。不要重做已通过的递归持久化、typed i18n 与模型搜索。

另有 [R48](issues/R48.md)，P2 / Spec / open：保留 R24 主实现，补运行中 child 与 swap、重启 Inspector 逐级查询的组合验收。

## 全历史扩审新增

| 卡 | 优先级 | 标题 | 状态 |
|---|---|---|---|
| [R49](issues/R49.md) | P1 | 在转发终态前闭合未结束工具 | open |
| [R50](issues/R50.md) | P1 | 无引擎 cancel 能力时仍释放 Hub claim | open |
| [R51](issues/R51.md) | P1 | HTTP Stop 在真实终止后再记取消终态 | open |
| [R52](issues/R52.md) | P2 | 补齐五适配器契约剧本与违约处置完成口径 | open |
| [R53](issues/R53.md) | P2 | 移除 Workbench 终端浏览器文本预览的伪连接 | open |
| [R54](issues/R54.md) | P2 | 区分工具记录与真实 Git 修改及 Review 状态 | open |
| [R55](issues/R55.md) | P2 | 用量费用区分未知、已知零和部分统计 | open |
| [R56](issues/R56.md) | P2 | 让公开表面快照覆盖真实 Rust 公共契约 | open |
| [R57](issues/R57.md) | P2 | 将 ToolEffect 审批说明传到真实请求界面 | open |
| [R58](issues/R58.md) | P1 | 按运行代际登记和清理 RunHandle | open |
| [R59](issues/R59.md) | P2 | 让全局搜索范围与真实结果一致 | open |

R49/R50/R58协调HubRuntime同文件改动；R51消费统一控制面；R52保留已存在契约测试，只补空洞；R53–R55、R59为局部truthful UI修复；R56/R57分别是快照闸与审批说明链。最终完成口径均由R46统一整理。

## 最终补齐的专项验收卡

- [R60](issues/R60.md)（P2，open）：让中间件错误回归真正等待并执行断言。
- [R61](issues/R61.md)（P2，open）：通过正式产品装配入口验收第五引擎。
