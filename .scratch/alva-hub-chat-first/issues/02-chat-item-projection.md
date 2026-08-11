# 02 — 建立统一 ChatItem 投影模型

**Status:** planned

## Goal

把持久化记录和实时 engine 事件统一投影为聊天流项目，先解决数据模型，再改界面。

## Scope

- 定义 user message、assistant message、work group、permission、fallback、switch、readiness、worker、error 等 `ChatItem`；
- 合并 StoredTurn、StoredTimelineEvent 和 live TimelineEvent；
- 保持稳定排序、去重、流式 assistant upsert 和崩溃恢复语义；
- 为投影函数添加纯单元测试。

## Acceptance

- 同一 session 重启前后生成相同顺序的 ChatItem；
- streaming delta 不产生重复 assistant message；
- permission/fallback/worker 事件保留原 ID，可继续执行操作；
- 现有 backend DTO 无破坏性修改。

**Blocked by:** 00 — 冻结旧 Tauri 对照并建立迁移清单.
