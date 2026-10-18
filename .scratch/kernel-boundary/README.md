# 内核边界重构

> **ARCHIVED：原 01–19 卡保留作历史资料，不再执行。**
> 当前唯一执行入口：[2026-09-11 当前新卡 R14–R23 + UI01–UI10](../kernel-boundary-review-round3/TASKS.md)。
> 2026-09-07 定案的 19 张历史工单在 [issues/](./issues/)，原依赖图见 [TASKS.md](./TASKS.md)。

## 目标

把 `alva-kernel-abi` / `alva-kernel-core` 收敛成一个**极其通用的 loop + 工具调用内核**，
不与任何具体工具耦合；Hub 补上"第五个引擎能进来"的最后一步。

## 两条判据（仓库根 `CONTEXT.md` 的正式定义）

> **一个只会翻译文本的 agent，如果不需要某个概念也能跑起来，那个概念就不在内核里。**
>
> **长期没有第二个实现者、也没有真实消费者的 trait 方法或模块，不是"给未来留的扩展点"，是死表面。**

动手前先读 `CONTEXT.md`。它定义了 Kernel / Agent Contract / Harness / Tool / Tool Policy /
Conversation Log / Session Store / Engine Descriptor / Orchestration 的准确含义，
工单正文一律使用这套词汇。

## 顺序为什么是这个

**拆 `kernel-abi` 排在最后，不是最先。** 依赖它的有 29 个 crate，先拆就等于把七个死模态、
四张死名单、六个死方法一起搬过新边界再删——搬运一遍死代码，而且那次 diff 大到没人能 review。
先立闸（01）、先删干净（02-04）、先把不该留在内核的搬走（09/10），`kernel-abi` 自己会缩到
一个拆起来很轻的体积。

## 分支

**一条巨型分支**（本次经用户明确决定，临时破例）。每张工单一个 commit，
**工单 01 之后每个 commit 都必须带公开符号快照 diff**——快照文件是这条分支的进度条，
也是它不会失控的唯一保险。每完成一组（见 TASKS.md 的分组）打一个全绿 checkpoint。

## 明确不做

这些是**已经讨论过并否决**的方向，不要在实现中顺手"改进"：

| 不做 | 理由 |
|---|---|
| Hub UI 插件化 | 成本远超产出，且与既有视觉规范冲突 |
| Hub 产品能力插件化（就绪/provider/权限/修复） | 等引擎与编排两条 seam 落地后自然显形 |
| 声明式 TOML 引擎（零 Rust 接入第五引擎） | **下一阶段目标**，不在本分支 |
| 抽编排策略 trait | 只有一种编排存在过，现在抽会抽错 |
| 循环的阶段级 seam | 会把循环内部结构变成公开契约，那才真正锁死内核演化 |
| 引擎 AEP 插件协议 | 引擎本来就是子进程，包一层 RPC 无新能力 |
| 终端 agent 改走 EngineRuntime | 内核会失去唯一一个必须自己跑通的消费者 |
| trait 方法数上限 lint | 凑数指标，合并成返回结构体即可规避；符号快照已覆盖 |
| 2026-07-18 review 的 #4、#12–20 | 与本次结构无关，隔离处理 |

## 验收硬指标

- `alva-kernel-abi` 6001 行 → 约 2500
- `alva-kernel-core` 8624 行 → 约 3500
- `Tool` trait 20 个方法 → 7 个（`name` / `description` / `parameters_schema` /
  `execute` / `scheduling` / `describe_effect` / `is_read_only`）
- `alva-hub-desktop` 的权限、就绪、装配三个模块中，引擎名字面量出现次数为 0
