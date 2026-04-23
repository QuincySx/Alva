# Amp 反编译分析 —— 组织索引

> Amp (Sourcegraph) 是闭源的 CLI coding agent。我们对 `~/.amp/bin/amp` (Bun 编译的 70 MB Mach-O)
> 做了静态分析，把所有能还原出来的内容按模块分类在这里。

**版本信息**：二进制内置 version `0.0.1776760235-g65b009`。

**定位**：这是 **参考资料库**，不是规范。Alva 的设计决策应该基于自身需求，把这里当作"别人怎么做过"的资料。

---

## 两种入口：给人看 vs 给 agent 看

这套文档提供**两种入口**，分别面向人类阅读和 LLM / agent 加载：

| 文件类型 | 目的 | 特征 |
|---|---|---|
| `README.md`（本文件 + 各子目录）| 人类阅读 / 导览 | 目录树、快速查询表、完整 context |
| `SKILL.md`（每个子目录 + 根）| LLM 懒加载 / agent 检索 | frontmatter + trigger_words + 精简索引 + 常见速答 |

### 设计思想（借鉴 Amp 本身）

Amp 处理 skills 的思路是"**只挂名字和描述**，内容按需 `load_skill`"。这份文档采用同样的模式：

```
docs/amp/SKILL.md                    ← 顶层 skill，只列子 skill 名字
  │
  ├── prompts/SKILL.md                ← 子 skill："想看 prompt 来这"
  │   └── executor-modes.md / subagents.md / ...  ← 真正的内容
  │
  ├── tools/SKILL.md                  ← 子 skill："想查工具来这"
  │   └── architecture.md / catalog.md / ...
  │
  ├── context/SKILL.md                ← ... 以此类推
  └── ...
```

### 使用建议

**人类阅读**：从 `README.md` 开始，顺目录结构深入具体 `.md` 文件。

**Agent / LLM 加载**：
1. 先加载顶级 `SKILL.md`（几 KB）了解有哪些模块
2. 根据 trigger_words 匹配，加载需要的 `<module>/SKILL.md`（各自 2-3 KB）
3. 还不够再读具体 markdown 文件

**把这套接入 Amp 自己的 skill 系统**：配置 `additionalSkillPaths` 指向 `docs/amp/`，Amp 会自动扫描到所有 SKILL.md。`disable-model-invocation` 没设，全部模型可触发。

**把这套接入 Alva 的 skill 系统**（等 `alva-protocol-skill` ready）：同理配置 skill search path。

**总 tokens 开销对比**：
- 一次性加载全部 45 个 md：**~10K 行 / ~400 KB**
- 只加载顶级 SKILL.md：**61 行 / ~2.5 KB**
- 加载顶级 + 1 个子模块 SKILL：**~160 行 / ~6 KB**
- 按 trigger_words 精准命中的实际文件：**几百行 / ~15-40 KB**

节省 **90%+** context 占用。

---

## 快速入口

| 想了解 | 看这里 |
|---|---|
| 怎么反编译出来的？ | [`00-methodology.md`](./00-methodology.md) |
| Amp 整体长什么样？ | [`01-architecture.md`](./01-architecture.md) |
| Amp 的系统提示词长什么样？ | [`prompts/`](./prompts/) |
| Amp 有哪些工具？怎么注册的？ | [`tools/`](./tools/) |
| 为什么 Amp 上下文看起来很干净？ | [`context/`](./context/) |
| Amp 的会话 / 消息 怎么存？ | [`storage/`](./storage/) |
| Amp 的 Skills 是什么？ | [`skills/`](./skills/) |
| Amp 有插件系统吗？ | [`plugins/`](./plugins/) |
| Amp 跑在云上是怎么跑的？ | [`remote-runtime/`](./remote-runtime/) |
| Amp 怎么编排多个 agent？ | [`orchestration/`](./orchestration/) |
| **对 Alva 有什么启发？** | [`alva-learnings/`](./alva-learnings/) |

---

## 目录树

```
docs/amp/
├── README.md                          ← 你在这里
├── 00-methodology.md                  反编译方法论
├── 01-architecture.md                 整体架构
│
├── prompts/                           所有系统提示词
│   ├── README.md
│   ├── executor-modes.md              7 个 executor 模式 (fwR/kwR/$wR/EwR/MwR/DwR/wwR)
│   ├── orchestrator-aggman.md         Agg Man 编排模式
│   ├── subagents.md                   Oracle / Librarian / Reviewer / File Analyzer / Walkthrough
│   ├── compaction-recap.md            /compact 和 handoff 两套 recap 模板
│   ├── assembly-pipeline.md           system prompt 装配 + SHA 分片
│   └── placeholder-dictionary.md      ${Y8} 等变量符号解码表
│
├── tools/                             工具系统
│   ├── README.md
│   ├── architecture.md                工具数据结构 + Observable fn
│   ├── execution-scheduler.md         resource lock + 并发策略
│   ├── catalog.md                     40+ 工具清单
│   └── custom-agents.md               .agents/agents/*.md 自定义子 agent
│
├── context/                           上下文管理
│   ├── README.md
│   ├── strategy.md                    四层策略总览
│   ├── file-change-tracker.md         edit 历史追踪
│   ├── in-thread-compact.md           /compact slash command
│   ├── handoff.md                     跨线程 handoff（取代自动压缩）
│   └── diagnostics.md                 amp context 命令 + 缓存监控
│
├── storage/                           会话与同步
│   ├── README.md
│   ├── thread-model.md                Thread / Message 数据结构
│   └── sync-protocol.md               version vector + flushVersion
│
├── skills/                            Skills 系统
│   ├── README.md
│   ├── design.md                      懒加载 + 两种渲染模式
│   ├── file-format.md                 SKILL.md frontmatter
│   └── builtin-skills.md              code-tour / code-review / walkthrough
│
├── plugins/                           .amp/plugins/ 插件系统
│   ├── README.md
│   ├── hooks.md                       agent / tool / configuration 生命周期
│   ├── rpc-api.md                     Plugin ↔ Host RPC 表面
│   └── debugging.md                   amp plugins exec
│
├── remote-runtime/                    远程执行环境
│   ├── README.md
│   ├── dtw.md                         Distributed Thread Worker (Cloudflare)
│   └── stream-json.md                 --execute --stream-json subprocess 协议
│
├── orchestration/                     编排与协作
│   ├── README.md
│   ├── execution-threads.md           $iT / Yg / Qg 指挥链
│   └── canonical-workflows.md         merge_changes / code_review 固化 prompt
│
└── alva-learnings/                    对 Alva 的启发
    ├── README.md
    ├── comparison.md                  已有 vs 缺失对照
    ├── resource-lock-scheduler.md     资源锁调度器
    ├── handoff-tool.md                handoff 工具设计方案
    ├── workflow-skill.md              WorkflowSkill 类型
    ├── plugin-exec.md                 alva plugins exec 命令
    └── context-diagnostics.md         alva context CLI
```

---

## 核心结论（TL;DR）

如果只看一段话：

> Amp 的"干净上下文"不来自某个聪明的算法，而来自**系统性的工程约束**：
> 工具输出端就截断、Skills 只挂名字不挂内容、AGENTS.md 32 KiB 硬上限、
> 子 agent 全部推给 Gemini Flash 做总结、主模型永远看不到子 agent 细节。
> 当本地线程真撑不下时，不做 in-place 压缩，而是**让 LLM 自己调 handoff 工具开新线程**。
> 这种"永远不假设用户愿意等压缩"的设计哲学比任何具体技术都值得抄。

Amp 还有一层大多数人没意识到的设计：它是**双态 agent** —— CLI 模式下是 executor，
ampcode.com web 上是 orchestrator（"Agg Man"），指挥一组跑在 Cloudflare Workers 上的
"execution threads"。Handoff 不只是压缩替代品，是这套分布式编排的关键同步点。

---

## 使用这些文档的建议

**读顺序**：如果你第一次看，按目录编号读：先 `00-methodology.md` 再 `01-architecture.md`，
然后挑自己关心的模块深入。

**引用格式**：文档里的所有 "${变量名}" 都是二进制里的混淆后标识符，真实名称见
[`prompts/placeholder-dictionary.md`](./prompts/placeholder-dictionary.md)。

**原始产物**：反编译中间产物在 `/tmp/amp-decompile/`（`strings.txt` + 分类拆分文件）。
这份文档是对它的结构化提炼。
