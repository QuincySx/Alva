# H02 · SPEC：产品方向冻结、术语与规范迁移

- 日期：2026-09-20；依赖：H01（INVENTORY.md / LEGACY-MAP.md）
- 词汇表权威：`docs/ALVA-HUB-GLOSSARY.md`（本次已增量维护，见 §5）。禁止新造平行领域模型。

## 1. 产品定义（冻结）

ALVA Hub 是**默认自研 ALVA 引擎的多引擎 Agent 工作台**（体验目标类似 Codex，不复制其全部功能）：

- 引擎：**Alva（默认）、Codex、Claude Code SDK、Pi**。Engine ≠ Model ≠ Provider。
- Provider connection 管理不同协议端点与认证/额度来源；官方订阅与第三方 API 分开呈现；发任务时只能选真实兼容的 Engine/Model/来源组合。
- 中央阅读区**优先简单、完整**：全部用户可见的 AI 消息、工具调用/结果、审批、执行状态先完整展示在主阅读轴；不做信息折叠、摘要替代、不把工具信息移出主轴。"全部"不含密钥、隐藏推理、内部日志、安全策略禁止展示的数据；"完整"不要求所有历史同时挂载 DOM。
- 未来任何收拢只是展示策略，权威记录不丢。

## 2. 两模式主流程

1. **对话模式（Conversation mode）**：启动即可进入，不选项目/仓库。会话用受管独立目录（沿用 `session_workspace.rs` 的 marker 契约），cwd/权限契约不变。
2. **项目模式（Project mode）**：左侧按 Project 聚合工作目录（workspace）与其会话；项目可关联多个 Git worktree（H05 只读发现）。
3. 两模式都是正式入口，对话不是项目模式的例外；切换模式不搬迁会话、不扩大权限、不改变已有会话的实际 cwd。
4. 主流程：加项目→发现目录→选来源/模型→发任务→审批/停止→看文件/diff→继续→重启恢复（H18 验收路径）。

## 3. 保留 / 调整 / 暂缓清单

**保留（不得默认取消）**
- 自动 fallback 与 FallbackPolicy、EngineBranch/Handoff 全套（词汇表既有语义）。
- TeamRun/Worker（Cindy Lead/Worker）与受管 worktree 采纳流。
- PermissionProfile 三档与审批分组；SessionRecord 权威记录；R42 之外的现有诚实禁用占位（未接线工具不得假装完成）。

**调整**
- 旧规则"ChatTranscript 仅 user/assistant、工具只归 Workbench"（R42）→ **迁移**：中央阅读轴合并 user/assistant 消息、工具调用/结果、审批与状态（H09 实施）；Workbench 工具区保留为辅助操作面。工具活动条目不再从主时间轴滤除。
- 旧会话中心导航（无项目聚合）→ 增加项目中心导航；会话中心导航在对话模式保留。
- 工具区位置：用户两次说"左边"，现有规范是右侧。**未定前沿用右侧**（词汇表 Tool Workspace 条目不动），H03 出可逆方案（布局 token 化，左右可切），未经用户确认不当作已接受决策。

**暂缓（不阻塞主流程）**
- Interactive 模式的 claude/codex/pi（当前 engine_wiring 拒绝，保持）。
- 主题浅色/系统、密度开关、全局快捷键、reduced-motion 设置项（现禁用占位，H17 统一处理）。
- hub-server 窄端点面与无鉴权（H18 单独声明限制）。

## 4. 受影响规范映射

| 规范文件 | 影响 | 动作 |
|---|---|---|
| `docs/ALVA-HUB-GLOSSARY.md` | 增 Project / Conversation mode / Project mode；Engine 补 Pi | 本次已更新（见 §5） |
| `docs/ALVA-HUB-UI-GLOSSARY.md` | 两模式入口命名、中央阅读轴区域命名 | H03 落地时更新 |
| `docs/ALVA-HUB-UI-WIREFRAMES.md` | 启动页/左侧导航两态 | H03 出方案后更新 |
| `docs/ALVA-HUB-UI-MOTION.md` | 无新增动效要求 | 不动 |
| `crates/alva-hub-desktop/web/src/AGENTS.md` | 中央展示迁移说明 | H09 同 commit 更新 |

## 5. 词汇表增量（已提交到 glossary）

- **Project**：用户声明的产品聚合——一个工作目录（Workspace）及其会话集合；可含多个 Git worktree（H05 发现）。Project 不是 HubSession，也不是 Workspace 本身；非 Git 目录允许成为 Project，不擅自 git init。
- **Conversation mode**：启动入口之一，会话不强制关联 Project，使用受管独立目录。
- **Project mode**：启动入口之一，左侧按 Project 聚合工作树与会话。
- **Engine**：V1 引擎清单改为 Alva（默认）、Claude、Codex、Pi。
