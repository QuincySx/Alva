# alva-agent-tools

> Built-in tool implementations for the agent framework (file ops, shell, search, browser automation).

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | src/lib.rs | Declares tool modules and provides `register_builtin_tools` / `register_all_tools` registration functions |
| AskHuman | src/ask_human.rs | Requests input from the human user via stdin in CLI mode |
| CreateFile | src/create_file.rs | Creates or overwrites a file with auto-creation of parent directories |
| ExecuteShell | src/execute_shell.rs | Executes shell commands via tokio process with configurable timeout and working directory |
| FileEdit | src/file_edit.rs | Performs string-replace-based file editing requiring unique match of old_str |
| GrepSearch | src/grep_search.rs | Searches for regex patterns across workspace files with glob filtering and line-level results |
| InternetSearch | src/internet_search.rs | Searches the internet using DuckDuckGo Instant Answer API |
| ListFiles | src/list_files.rs | Lists directory contents with recursive traversal and hidden file filtering |
| ReadUrl | src/read_url.rs | Fetches a web page and returns plain-text content with HTML tags stripped |
| ViewImage | src/view_image.rs | Reads image files and returns base64-encoded content with MIME type detection |
| Browser (module) | src/browser/mod.rs | Module declaration and re-exports for the 7 browser automation tools and their shared manager |
| BrowserManager | src/browser/browser_manager.rs | Manages Chrome instance lifecycle, CDP connections, tab navigation, and provides shared access via Arc<Mutex> |
| BrowserStart | src/browser/browser_start.rs | Launches a Chrome browser instance with configurable headless mode, profile, and proxy |
| BrowserStop | src/browser/browser_stop.rs | Shuts down a running Chrome browser instance and releases all resources |
| BrowserNavigate | src/browser/browser_navigate.rs | Navigates the browser to a URL, waits for load, and returns the final URL and title |
| BrowserAction | src/browser/browser_action.rs | Performs page interactions (click/type/press/scroll) via CSS selectors or coordinates |
| BrowserSnapshot | src/browser/browser_snapshot.rs | Extracts page content in text, HTML, or readability (article-extraction) mode |
| BrowserScreenshot | src/browser/browser_screenshot.rs | Captures page screenshots (viewport, full-page, or element) and saves to file |
| BrowserStatus | src/browser/browser_status.rs | Queries browser instance status including running state, current URL, title, and open tabs |
