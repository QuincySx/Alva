# ALVA Hub Chat-First Roadmap

> 重新梳理日期：2026-08-11。这里取代旧的 `alva-hub-desktop-v1` 任务集。

## 已确认的产品决策

1. 桌面产品主入口是 `alva-hub-gpui`。
2. `alva-app-gpui` 不再继续扩展产品功能，只作为单引擎参考实现和迁移期间的代码来源。
3. Tauri 入口已经退出 Cargo workspace，但旧目录在 GPUI 功能与视觉验收完成前保留为只读对照；最终验收通过后再删除。
4. Hub 主界面改为 chat-first：会话、运行过程、审批、fallback、worker 和结果都出现在同一聊天流。
5. 标准控件必须优先使用 `gpui-component`，不再用裸 `div()` 重造 Button、Tag、Tab、Sidebar、Sheet、Collapsible 等组件。
6. Claude/Codex 仍由现有 engine adapter 驱动；Hub 在其上提供模型目录、连接管理、协议代理和兼容性判断。

## 已有能力，不重写

下面这些能力已经存在于 `alva-hub-gpui` 后端，任务只负责重新投影和接入新的聊天界面：

- HubSession / Turn / EngineAttempt / EngineBranch 持久化与崩溃恢复；
- 手动切换、native resume、handoff、fallback chain、watchdog stall；
- readiness、repair、PermissionProfile、敏感信息过滤；
- TeamRun、WorkerSession、ResourceClaims、worktree adoption、hibernate/archive。

能力规范仍以 `docs/ALVA-HUB-V1.md`、`docs/ALVA-HUB-GLOSSARY.md` 和现有测试为准。

## 里程碑

| 里程碑 | Tickets | 交付结果 |
|---|---|---|
| M0 建立迁移基线 | 00–01 | 冻结旧 Tauri 对照，建立统一 GPUI 组件规范 |
| M1 Chat Core | 02–03 | 持久化与实时事件统一投影成可流式聊天界面 |
| M2 In-Chat Operations | 04 | 审批、fallback、切换、readiness 都在聊天流完成 |
| M3 Model Platform | 05–08 | 模型选择、供应商连接、兼容性判断和协议代理闭环 |
| M4 Collaboration & Ship | 09–10 | Worker 协作内联，完成桌面打包与三引擎验收 |

## 推荐执行顺序

`00 → 01 → 02 → 03 → 04 → 05 → 06 → 07 → 08 → 09 → 10`

其中 06–08 可以在 03 完成后并行预研，但不得跳过 07 的兼容性判定直接把未知模型标记为可用。

## 非目标

- 不重写 `alva-hub-core` 或已有 fallback/team runtime；
- 不把 `alva-app-gpui` 和 `alva-hub-gpui` 合并成一个 crate；
- 不为追求视觉效果自建第二套设计系统；
- 不在 Hub SQLite 中明文保存 API key；
- 不假设任意 OpenAI-compatible 模型一定兼容 Claude Code 或 Codex。
- 不在 GPUI 功能、视觉和打包验收完成前删除旧 Tauri 对照目录。

## 当前状态

- 旧 23 张能力建设 tickets：作废，后端能力已基本落地；
- 新任务：11 张；ticket 00 已完成审计，其余为 `planned`；
- 功能代码：尚未按本任务集开始实施。
