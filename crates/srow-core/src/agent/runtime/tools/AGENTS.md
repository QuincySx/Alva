# agent/runtime/tools
> Agent 运行时内置工具集

## 地位
提供 Agent 可调用的全部内置工具，包括文件操作、Shell 执行、代码搜索、网络访问和浏览器自动化。

## 逻辑
`register_builtin_tools` 注册 9 个基础工具，`register_all_tools` 额外注册 7 个浏览器工具。所有工具实现 `Tool` trait，通过 `ToolRegistry` 按名查找。

## 约束
- 所有工具必须实现 `Tool` trait
- tool_call_id 由 engine 层填充，工具返回时留空

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | register_builtin_tools、register_all_tools |
| execute_shell | `execute_shell.rs` | ExecuteShellTool：Shell 命令执行 |
| create_file | `create_file.rs` | CreateFileTool：文件创建/覆写 |
| file_edit | `file_edit.rs` | FileEditTool：字符串替换式编辑 |
| grep_search | `grep_search.rs` | GrepSearchTool：正则跨文件搜索 |
| list_files | `list_files.rs` | ListFilesTool：目录列表 |
| view_image | `view_image.rs` | ViewImageTool：图片 base64 编码 |
| ask_human | `ask_human.rs` | AskHumanTool：请求用户输入 |
| internet_search | `internet_search.rs` | InternetSearchTool：DuckDuckGo 搜索 |
| read_url | `read_url.rs` | ReadUrlTool：抓取网页纯文本 |
| browser/ | `browser/` | 浏览器自动化工具子集 |
