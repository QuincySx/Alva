# Screenshot and Wireframe Map

## Evidence inventory

- Cindy source screenshots: **67/67**, `.scratch/cindy-ui-reference/screenshots/`
- Cindy state lineworks: **67/67**, `.scratch/cindy-ui-reference/linework/`
- Preserved Cindy aggregate wireframes: **7/7**, `.scratch/cindy-ui-reference/wireframes/`
- Redrawn ALVA Tauri target wireframes: **7/7**, `.scratch/alva-hub-tauri-ui-system/wireframes/`
- Browser visual baselines: **9**, `crates/alva-hub-desktop/web/tests/visual.spec.ts-snapshots/`

The numeric names connect target SVGs and Playwright screenshots. The ninth
browser baseline is the narrow Workbench overlay, required because its region
relationship differs from wide desktop.

## 页面族

| 页面族 | 总线稿 | 逐状态线稿 | 需要抽取的稳定区域 |
|---|---|---|---|
| Chat Shell | `wireframes/01-chat-shell.svg` | 01、31–34、49–52 | Sidebar、Topbar、ConversationScroll、ConversationDock、Composer |
| New Conversation | `wireframes/02-new-conversation.svg` | 02–07 | EmptyConversation、QuickStart、Composer、Context/Agent/Model Popover |
| Right Workbench | `wireframes/03-right-workbench.svg` | 35–48 | WorkbenchRail、WorkbenchTabs、Files、Agent、Inspector、Tasks、Usage、Browser、Terminal |
| Automations | `wireframes/04-automation.svg` | 08–10、62–64 | PageHeader、TemplateGrid、AutomationList、AutomationDialog |
| Plugins and Skills | `wireframes/05-plugins-skills.svg` | 11–13、59–61 | PageTabs、SearchBar、ExtensionGrid、ExtensionCard、ExtensionDetail |
| Settings | `wireframes/06-settings.svg` | 15–28 | SettingsNav、SettingsPage、SettingsGroup、SettingsRow |
| Context Menus | `wireframes/07-context-menus.svg` | 53–58 | ProjectMenu、SessionMenu、FolderPicker、DangerConfirmation |

搜索状态 14、65 使用全局 `SearchSurface`，不另建第八套页面骨架。

## 线稿使用方法

1. 总线稿只决定区域归属和阅读顺序，不决定最终颜色。
2. 逐状态线稿决定弹出方向、触发关系、列表密度和关闭路径。
3. 原始截图决定字体、对比度、圆角、阴影和实际空间占比。
4. 实现截图必须与总线稿和对应逐状态线稿组成三联图验收。

## 布局断点

| 视口 | 区域行为 |
|---|---|
| ≥ 1360 | Sidebar + Main + 可调整 Workbench 三栏 |
| 1024–1359 | Sidebar 收窄；Workbench 打开时 Main 整体收缩 |
| 760–1023 | Workbench 变成覆盖式 Sheet；Main 宽度不二次压缩 |
| < 760 | Sidebar 与 Workbench 均为覆盖层；Composer 控件分两行 |

断点只改变区域关系，不允许页面组件各自发明响应式规则。
