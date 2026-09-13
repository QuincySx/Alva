# 第四轮复现证据
> 用公开接口和受控React bridge复现缺陷，保留日志用于返工验收。

## 地位
非生产源码；复制到对应测试目录后运行，执行者须避免覆盖他人文件。
## 逻辑
README给出命令和局限，源码对应core/chat日志，既有suite日志提供对照。
## 约束
不得将mock bridge称作真实Tauri端到端；探针失败是复现，不是修复交付。
## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| 验证说明 | README.md | 命令、结果与限制 |
| 会话探针 | review_round4_probes.rs | 7项装配/广播API断言 |
| 聊天探针 | review-round4.test.tsx | 历史可读控制及live终态正文断言 |
| 新增结果 | core-probes.txt、chat-probe.txt、chat-live-probe.txt | 本轮专项结果 |
| 原有结果 | existing-core-regressions.txt、existing-web-tests.txt | 原套件复跑结果 |
