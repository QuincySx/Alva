# alva-agent-tools/src
> Agent 内置工具实现层：文件操作、Shell 执行、搜索、浏览器自动化

## 地位
`alva-agent-tools` crate 的全部源码。每个工具实现 `alva_types::Tool` trait，通过 `lib.rs` 的 `register_builtin_tools` / `register_all_tools` 注册到 `ToolRegistry`。被 `alva-agent-runtime` 的 Builder 在构建时统一注册。

## 逻辑
1. `lib.rs` 按 feature flag 分三档注册工具：
   - 标准工具（始终可用）：ask_human, create_file, execute_shell, file_edit, grep_search, list_files, view_image
   - Native-only 工具（feature = "native"，wasm 下禁用）：internet_search, read_url
   - Browser 工具（feature = "browser"）：browser_start/stop/navigate/action/snapshot/screenshot/status
2. `local_fs.rs` 提供 `LocalToolFs`（基于本地 OS 的 `ToolFs` 实现）和 `walk_dir` 递归遍历辅助函数，是大部分文件工具的底层依赖。
3. `mock_fs.rs` 提供 `MockToolFs`（内存中的 `ToolFs` 实现），用于工具单元测试。
4. `browser/` 子目录封装基于 CDP (Chrome DevTools Protocol) 的浏览器自动化工具组。

## 约束
- 所有工具必须实现 `alva_types::Tool` trait（name / description / input_schema / execute）。
- 文件类工具通过 `ToolContext.tool_fs` 访问文件系统，禁止直接调用 `std::fs`，以保证可测试性和沙箱隔离。
- `internet_search` 和 `read_url` 需要网络访问，仅在 `native` feature 下编译。
- `browser/` 工具需要 `chromiumoxide` 依赖，仅在 `browser` feature 下编译。
- `execute_shell` 通过 `ToolFs::exec` 执行，受安全中间件的沙箱策略约束。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | lib.rs | 声明工具模块，按 feature 分档提供注册函数 |
| AskHumanTool | ask_human.rs | CLI 模式下通过 stdin 请求人工输入 |
| CreateFileTool | create_file.rs | 创建或覆写文件，自动创建父目录 |
| ExecuteShellTool | execute_shell.rs | 通过 ToolFs 执行 shell 命令，支持超时和工作目录配置 |
| FileEditTool | file_edit.rs | 基于字符串替换的文件编辑，要求 old_str 唯一匹配 |
| GrepSearchTool | grep_search.rs | 正则搜索工作区文件，支持 glob 过滤和行级结果 |
| InternetSearchTool | internet_search.rs | 通过 DuckDuckGo Instant Answer API 搜索互联网（native-only） |
| ListFilesTool | list_files.rs | 列出目录内容，支持递归遍历和隐藏文件过滤 |
| LocalToolFs | local_fs.rs | 本地文件系统 ToolFs 实现及 walk_dir 递归辅助函数 |
| MockToolFs | mock_fs.rs | 内存 ToolFs 测试实现，预置文件内容和 exec 响应 |
| ReadUrlTool | read_url.rs | 抓取网页并返回去 HTML 标签的纯文本（native-only） |
| ViewImageTool | view_image.rs | 读取图片文件，返回 base64 编码内容及 MIME 类型 |
| Browser Tools | browser/ | 基于 CDP 的浏览器自动化工具子模块（7 个工具 + 管理器） |
