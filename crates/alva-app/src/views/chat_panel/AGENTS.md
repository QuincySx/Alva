# chat_panel
> 中央对话面板：消息列表 + 输入框

## 地位
在 `views` 中作为主交互区域，占据三栏布局的中央弹性列，负责展示和发送聊天消息。

## 逻辑
- `ChatPanel` 组合 `MessageList` 和 `InputBox` 两个子 Entity，顶部带 "Chat" 标题栏。
- `MessageList` 订阅 `WorkspaceModel`（session 切换）和 `ChatModel`（新消息/流式增量），根据当前 session 读取已有消息、streaming buffer 和 thinking buffer 并渲染气泡。
- `InputBox` 捕获键盘事件构建 draft 文本，Enter 时调用 `EngineBridge::send_message` 启动引擎；Shift+Enter 换行。
- `InputBox` 在 agent 运行中禁用发送按钮。

## 约束
- 键盘输入基于 `on_key_down` 手动拼装字符，无系统输入法/IME 支持。
- 消息气泡为纯文本渲染，未支持 Markdown。
- 工具调用输出截断为 300 字符。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| ChatPanel | `chat_panel.rs` | 组合 MessageList + InputBox 并渲染 header |
| MessageList | `message_list.rs` | 滚动展示已完成消息、thinking 指示器、streaming 增量和 tool call 卡片 |
| InputBox | `input_box.rs` | 可聚焦输入控件，处理键盘事件、draft 管理、发送消息 |
| MessageBubble | `message_bubble.rs` | 无状态消息气泡渲染（user/assistant/system），支持 Markdown |
| AgentBlock | `agent_block.rs` | 已完成/运行中 Agent 块渲染，支持点击展开 |
| ThinkingBlock | `thinking_block.rs` | 可折叠推理/思考块，展开状态由父组件持有 |
| ToolCallBlock | `tool_call_block.rs` | 工具调用块渲染（placeholder） |
| RunningAgentsZone | `running_agents_zone.rs` | 输入框上方运行中 Agent 状态条 |
| Markdown | `markdown.rs` | Markdown → GPUI 元素转换：标题、列表、代码、加粗/斜体、链接 |
| CodeBlock | `code_block.rs` | 语法高亮代码块渲染，带语言标签和复制按钮 |
| mod | `mod.rs` | 桶模块，re-export ChatPanel |
