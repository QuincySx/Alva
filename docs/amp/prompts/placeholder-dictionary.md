# 占位符字典 —— `${XX}` 变量解码表

Amp 二进制里所有 prompt 的工具名都是混淆后的 ES module 标识符。这张表用于阅读其他 prompt 文档。

---

## 工具类（在 prompt 里用 `${varname}` 引用）

| 变量 | 工具名 | 功能 |
|---|---|---|
| `${Y8}` | `Bash` | shell 命令，**执行上 serial 独占** |
| `${P8}` | `Read` | 读文件（绝对路径）|
| `${ee}` | `Grep` | 内容搜索（ripgrep）|
| `${vt}` | `codebase_search_agent` (finder) | 语义级代码搜索，内含 LLM 子 agent |
| `${dr}` | `edit_file` | str-replace 风格编辑 |
| `${_d}` | `edit_file` (别名) | 同上，在 $wR/EwR 套 prompt 用这个符号 |
| `${we}` | `create_file` | 新建文件 |
| `${q0T}` | `undo_edit` | 回滚最近一次编辑（依赖 fileChangeTracker）|
| `${he}` | `Task` | 子 agent 执行器（fire-and-forget）|
| `${wr}` | `oracle` | 高推理模型顾问（GPT-5.4 级）|
| `${ly}` | `read_web_page` | WebFetch（支持 objective 聚焦）|
| `${r$}` | `web_search` | WebSearch（支持 objective + queries）|
| `${ui}` | `read_github` / Librarian | 跨仓库 GitHub 代码理解 |
| `${U7}` | `chart` | 跑 shell 输出 JSON 渲染图表 |
| `${VW}` | `image_generation` | Nano Banana / Gemini 3 图生 |
| `${W0T}` | `mermaid` | 渲染 mermaid 图 |
| `${lET}` | `walkthrough_diagram` | 渲染交互式 walkthrough |
| `${is}` | `load_skill` | 按需加载 skill 内容 |
| `${H0T}` | `todo_write` | 任务列表（CRUD + 依赖 + 父子）|
| `${t$}` | `diagnostics` | 诊断/typecheck 工具（具体名未还原）|
| `${N$}` | 文件输出到 Artifacts | 生成用户可下载的文件（截图/导出）|

---

## Agg Man（Orchestrator）工具类

这些只在 Agg Man 模式 prompt 里出现：

| 变量 | 工具名 | 功能 |
|---|---|---|
| `${$iT}` | `create_execution_thread` | 创建全新 clean-slate execution thread |
| `${Yg}` | `send_to_execution_thread` | 给已有 execution thread 发消息（关键参数 `workflow`）|
| `${Qg}` | `callback` | execution thread 完成后回调 orchestrator |
| `${Pv}` | `read_thread` | 读取线程内容 / 当前状态 |
| `${bd}` | `search_threads` | 搜索线程（DSL：`file:` `repo:` `ref:` `author:`）|
| `${JCR}` | `create_project` | 创建 v2 project |
| `${TAR}` | `archive_thread` | 归档 |
| `${RAR}` | `restore_thread` | 恢复归档 |
| `${NCR}` | `workspace_doc_read` / `list` | workspace docs 读取 |
| `${UCR}` | `workspace_doc_write` | workspace docs 写入 |
| `${HCR}` | 类似 | workspace notes |
| `${ZCR}` | Agg Man UI 预览 | "what would Agg Man look like with visual changes" |
| `${tAR}` | `github_*` | GitHub 工具集合 |
| `${XW}` | `slack_read` | Slack 读（查 user / channel / thread / 加 reaction）|
| `${jiT}` | `slack_post` | Slack 发消息 |
| `${OET}` | `analyze_file` | 用 Gemini 3 Flash 分析单个文件 |

---

## Canonical Prompt 变量（workflow 参数内容）

这些是"按钮化"的 workflow，Orchestrator 调用 `${Yg}` 时把 `workflow: "xxx"` 作为参数，服务器把对应的 canonical prompt verbatim 发给 execution thread：

| 变量 | workflow 名 | 内容 |
|---|---|---|
| `${aqT}` | `"merge_changes"` | 标准化的 merge prompt，详见 [`../orchestration/canonical-workflows.md`](../orchestration/canonical-workflows.md) |
| `${uwR}` | `"code_review"` | 标准化的代码审查 prompt |

---

## 其他常见符号

| 变量 | 含义 |
|---|---|
| `${Ot}` | **AGENTS.md** —— 工作区向导文件（自动注入到 prompt）|
| `${nDT}` | AGENTS.md 的备用/等价文件（可能是 CLAUDE.md 或 rules 文件）|

---

## 主 Prompt 生成函数

这些在 [`executor-modes.md`](./executor-modes.md) 详解：

| 变量 | prompt 名 |
|---|---|
| `fwR()` | Pair programming 默认 prompt (You are Amp, workspace sharing) |
| `kwR({enableTask, enableOracle, enableDiagnostics, enableChart})` | 参数化 executor prompt |
| `$wR()` | Hardcore Executor (Guardrails + Parallel Execution Policy) |
| `EwR()` | $wR 的精简变体 |
| `MwR()` | Pair programming with XML tags |
| `DwR()` | Speed mode ("optimized for speed and efficiency") |
| `wwR({enableDiagnostics})` | Rush Mode (1-3 词回答) |
| `UwR({specialAgentName})` | 自定义 agent 骨架 |

## 子 Agent Prompt 变量

这些在 [`subagents.md`](./subagents.md) 详解：

| 变量 | 子 agent 名 | 基础模型 |
|---|---|---|
| `yFT(workingDir, workspaceRoot)` | Oracle | 主模型（高 reasoning）|
| `iuT` | Code Reviewer | 主模型 |
| `aGR` | Code Reviewer XML 输出格式 | — |
| `LGR` | Diff Explainer | 主模型 |
| `VVR` | Librarian（跨仓库代码理解）| 主模型 |
| `BXR` | File Analyzer | `gemini-3-flash-preview` |
| `DXR` | File Analyzer 的模型常量 | `"gemini-3-flash-preview"` |
| `F1R / G1R / K1R` | Walkthrough 三阶段 prompt | 主模型 |

---

## 其他关键常量

| 变量 | 值 | 含义 |
|---|---|---|
| `pG` | `500` | Read 默认行数上限 |
| `XLT` | `4096` | Read 单行字节上限 |
| `eD` | `~5 MB` | Read 单文件绝对上限 |
| `cpR` | `32768` | AGENTS.md 总预算（32 KiB）|
| `aXR` | `256000` | context window size 常量之一 |
| `vz` | `262144` | 类似 |
| `f4` | `4100` | 某种 chunk 大小 |
| `cO` | `50000` | 某种 buffer 大小 |
| `$h` | 默认 `AMP_URL`（`https://ampcode.com`）| |
