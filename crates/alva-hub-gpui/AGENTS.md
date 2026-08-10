# alva-hub-gpui — GPUI 开发规范（AGENTS）

> ALVA Hub Desktop 的 Rust 原生 GPUI 实现（迁移自 Tauri+React）。本文件是
> **唯一权威的 GPUI 开发规范**：初始化、组件写法、状态管理、测试、陷阱。
> 技术栈较新（GPUI = Zed 的 Rust 原生 UI 框架），先读本规范再动手。
>
> 依赖（git 锁定）：`gpui`（zed-industries/zed）+ `gpui-component`
> （longbridge/gpui-component ⭐12.5k，60+ 组件，Longbridge Pro 生产验证）。
> 迁移评估见 `docs/GPUI-MIGRATION.md`。

## 0. 硬性前提（必须先做）

1. **独立 workspace**：`alva-hub-gpui` 自带 `[workspace]` 表，**不进主 workspace**
   ——gpui 需要 wasm-bindgen ≥0.2.120，与主 workspace 的 alva-host-wasm（锁 0.2.114）
   冲突。命令：`cd crates/alva-hub-gpui && cargo build`（在 crate 内跑）。
2. **初始化顺序**（入口必须如此）：
   ```rust
   fn main() {
       gpui_platform::application()
           .with_assets(gpui_component_assets::Assets) // 图标必需
           .run(move |cx| {
               gpui_component::init(cx);               // MUST 第一个
               cx.spawn(async move |cx| {
                   cx.open_window(WindowOptions::default(), |window, cx| {
                       let view = cx.new(|_| MyApp::new());
                       // 窗口第一层必须是 Root（启用 dialog/sheet/notification/键盘导航）
                       cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
                   }).expect("open window");
               }).detach();
           });
   }
   ```
3. **叠加层**：要用 Dialog/Sheet/Notification，第一层 render 里必须挂三个层：
   ```rust
   div().size_full()
       .child(main_content(window, cx))
       .children(Root::render_dialog_layer(cx))
       .children(Root::render_sheet_layer(cx))
       .children(Root::render_notification_layer(cx))
   ```

## 1. 组件写法规范

### 1.1 无状态组件（Stateless，大多数）

直接在 render 里构建，builder 模式 `Component::new("id").method().method()`：

```rust
Button::new("submit").primary().label("Run").on_click(|_, window, cx| { /* ... */ })
```

变体：`.primary()` `.danger()` `.warning()` `.success()` `.ghost()` `.link()`
状态：`.disabled(true)` `.loading(true)` `.selected(true)`
尺寸：`.xsmall()` `.small()` `.medium()`（默认）`.large()`

### 1.2 有状态组件（Stateful：Input/Select/Combobox/List/Table/Tree…）

**必须**在 `new()` 里建 `Entity<State>` 存到结构体，render 传引用：

```rust
struct MyView {
    name: Entity<InputState>,          // 状态持有
    engine: Entity<SelectState>,
}

impl MyView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Your name")),
            engine: cx.new(|cx| SelectState::new(
                vec!["alva", "claude", "codex"], Some(IndexPath::default()), window, cx)),
        }
    }
}

impl Render for MyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().gap_2()
            .child(Input::new(&self.name))
            .child(Select::new(&self.engine))
    }
}
```

**读值/订阅事件**（在 `new()` 或初始化里）：
```rust
cx.subscribe_in(&self.name, window, |view, state, event, window, cx| {
    match event {
        InputEvent::Change => { let v = state.read(cx).value(); /* 更新 snapshot */ }
        InputEvent::PressEnter { .. } => { /* 提交 */ }
        _ => {}
    }
});
```

### 1.3 常用组件速查

| 需求 | 组件 | 形态 |
|---|---|---|
| 主/次/危险操作 | `button::{Button, ButtonGroup}` | 无状态 |
| 文本输入/密码/数字 | `input::{Input, InputState}` | 有状态 |
| 下拉/可搜索选择 | `select::{Select, SelectState}` / `combobox::Combobox` | 有状态 |
| 勾选/开关/单选 | `checkbox::Checkbox` / `switch::Switch` / `radio::Radio` | 无状态（受控） |
| 图标（Lucide） | `{Icon, IconName}` | 无状态（**图标来自 gpui-component-assets**，名见 `IconName` 枚举） |
| 状态徽章 | `badge::Badge` / `tag::Tag` | 无状态 |
| 弹窗/确认 | `dialog::Dialog` + `WindowExt`（`window.open_modal`） | 叠加层 |
| 侧栏抽屉 | `sheet::Sheet`（`window.open_sheet`） | 叠加层 |
| 通知 | `window.push_notification(...)` | 叠加层 |
| 悬浮菜单 | `menu::{PopupMenu, DropdownMenu}` / `popover::Popover` | 叠加层 |
| 侧栏导航 | `sidebar::{Sidebar, SidebarMenu, SidebarMenuItem, ...}` | **Hub 主壳用它** |
| 折叠面板 | `collapsible::Collapsible` / `accordion::Accordion` | Hub 的 fallback/team |
| 标签页 | `tab::{Tab, TabBar}` | 主区视图切换 |
| 大数据列表 | `{v_virtual_list, h_virtual_list}` / `list::{List, ListState, ListDelegate}` | 会话/任务列表 |
| 树 | `tree::{Tree, TreeState, TreeDelegate}` | 会话→分支层级 |
| 键值展示 | `description_list::DescriptionList` | handoff 信封字段 |
| Markdown | gpui-component 内置 markdown 渲染 | workspace_summary |
| 分组容器 | `group_box::GroupBox` | 面板标题容器 |
| 标题栏 | `title_bar::TitleBar` | 窗口 |
| 表单 | `form::{v_form, h_form, field}` | 设置/权限表单 |

## 2. 状态管理规范（GPUI 核心陷阱）

### 2.1 重入（Re-entrancy）——最容易 panic 的坑

GPUI 在 render/update 期间**锁定实体**。对**同一个实体**重入 read/update → panic：

```rust
// ❌ panic: entity_a 在自身 update 里又 update 自身
entity_a.update(cx, |_, cx| {
    entity_a.update(cx, |_, _| {});   // PANIC
});
// ✅ 不同实体嵌套 update 是安全的（不同锁）
entity_a.update(cx, |_, cx| entity_b.update(cx, |_, _| {}));
```

### 2.2 render 回调禁止访问外部实体

`render_item` 等渲染钩子内 `entity.read(cx)` 外部实体 → panic（该实体可能在渲染）。
**修法：维护纯 snapshot 字段，渲染只读 snapshot**：

```rust
// ❌ render 里读外部实体
fn render_item(&mut self, ix: IndexPath, ...) -> ... {
    let checked = parent.read(cx).contains(&ix);  // PANIC
}
// ✅ 读 snapshot（每次外部变更后同步）
fn render_item(&mut self, ix: IndexPath, ...) -> ... {
    let checked = self.selection_snapshot.contains(&ix);
}
```

### 2.3 闭包用弱引用（防 retain cycle）

```rust
// ❌ 强引用 → 循环引用泄漏
let entity = cx.entity();
some_callback(move || entity.update(cx, ...));
// ✅ 弱引用
let weak = cx.entity().downgrade();
some_callback(move || { let _ = weak.update(cx, |_, cx| cx.notify()); });
```

### 2.4 闭包内用 inner cx，别用外层

```rust
// ❌ 外层 cx 在嵌套回调里 borrow 冲突
entity.update(cx, |state, inner_cx| {
    cx.notify();   // 错
    cx.spawn(...); // 错
});
// ✅ 用 inner_cx
entity.update(cx, |state, inner_cx| inner_cx.notify());
```

### 2.5 `defer_in` 会重新锁实体——同重入规则

```rust
cx.defer_in(window, |state, window, cx| {
    // state 已被锁；直接 &mut 用，不要再 state.update(cx, ...)
    state.delegate_mut().apply(...);
    parent.update(cx, |p, cx| ...);  // 不同实体，安全
});
```

## 3. 布局规范

- `h_flex()` = 水平 flex；`v_flex()` = 垂直 flex（gpui-component 扩展）
- 常用：`v_flex().gap_2().p_4()`、`h_flex().items_center().justify_between()`
- 间距用 gap（`gap_1`=4px … `gap_4`=16px），**不用 margin 堆**
- 全屏骨架：`div().size_full().flex_row().child(sidebar).child(div().flex_1().size_full())`
- 复杂面板布局用 **Dock**（`dock::DockArea` + `PanelView`）——拖拽/缩放/持久化
- 窗口主壳：`title_bar::TitleBar` + `status_bar::StatusBar`
- 颜色/尺寸全部走主题，**不硬编码色值**（见 §4）

## 4. 主题规范

```rust
use gpui_component::ActiveTheme as _;
// 取色（唯一来源）
cx.theme().background / .foreground / .border / .surface / .muted
cx.theme().primary / .destructive
// 样式里用
div().bg(cx.theme().surface).text_color(cx.theme().foreground).border_color(cx.theme().border)
// 明暗切换
cx.update_global::<Theme, _>(|t, cx| t.toggle_mode(cx));
```

**规则**：任何颜色必须来自 `cx.theme()` 或 Theme 变量——不硬编码 hex；
状态色（healthy/failed/fallback）用 `success/danger/warning/primary` 语义色。

## 5. 测试规范

- 纯逻辑（无窗口）：普通 `#[test]` 即可
- 需要窗口/渲染：`#[gpui::test]` + `TestAppContext` / `VisualTestContext`
  ```rust
  #[gpui::test]
  fn my_test(cx: &mut TestAppContext) { /* cx.new/read/update */ }

  #[gpui::test]
  async fn my_async_test(cx: &mut TestAppContext) { /* async */ }
  ```
- 单线程确定性执行器；异步用 `cx.executor()` / `cx.background_executor()`
- **原则**：测复杂逻辑与核心状态转换，不堆简单测试；builder 模式组件补一个
  `test_*_builder` 覆盖构建参数
- 测试目录：`crates/alva-hub-gpui/tests/`（集成）+ `#[cfg(test)]`（单元）

## 6. 错误处理与异步

- 错误类型：自定义 `HubError`（参考 `crates/alva-hub-core/src/error.rs`），
  组件层用 `Result<_, HubError>` 传导，UI 层转换成人话 + `window.push_notification`
- 异步：GPUI 的 `cx.spawn(async move |cx| ...)`（主线程）+ `cx.background_executor().spawn`
  跑重活（store 查询/引擎调用），完成后 `cx.notify()`
- 引擎运行：P2 起复用 `alva-hub-core::HubRuntime` 的 `execute()` 事件流，UI 订阅转发

## 7. 迁移映射（React → GPUI 对应）

| React 概念 | GPUI 对应 |
|---|---|
| state/useState | `Entity<State>` + `cx.new` + `cx.notify()` |
| props | builder 方法 / 结构体字段 |
| useEffect | `cx.subscribe` / `cx.spawn` + `defer_in` |
| context | `Global`（`cx.new_global`/`update_global`） |
| JSX 树 | `div().child(...).children(...)` 链 |
| CSS | `Styled` trait（`.bg() .p_2() .gap_2()`）|
| Tailwind 类 | theme + styled 链 |
| 事件冒泡 | `EventEmitter` + `cx.subscribe` |
| Vite/TS | 无——纯 Rust 编译 |

## 8. 常见陷阱清单（写完代码自查）

- [ ] `init(cx)` 是否入口第一个调用？
- [ ] 窗口第一层是 `Root`？
- [ ] 需要 dialog/sheet/notification 时挂了三个 render 层？
- [ ] 有状态组件是否 `Entity<State>` 持有 + `new()` 构建？
- [ ] render 回调里是否读了外部实体？（→ snapshot）
- [ ] 同一实体嵌套 update？（→ 不同实体/直接 &mut）
- [ ] 闭包持强引用？（→ downgrade）
- [ ] 闭包里用了外层 cx？（→ inner_cx）
- [ ] 颜色是否来自 theme？（→ 无硬编码 hex）
- [ ] 每个交互元素有 `ElementId`/可访问名？
- [ ] `cargo check` + `cargo clippy -- -D warnings` + `cargo fmt` 过？

## 9. 验证命令（在 crate 内）

```bash
cd crates/alva-hub-gpui
cargo check          # 快查
cargo build          # 完整构建
cargo clippy -- -D warnings
cargo fmt --check
cargo test           # gpui::test
```

## 10. 替换目标与 Web 策略

- **最终形态**：`alva-hub-gpui` 是唯一产品入口。功能对齐后，旧 Tauri 版
  （alva-hub-desktop / alva-app-tauri）**整体删除**，不并存（用户决策）。
- **Web 场景**：gpui 支持 wasm32 目标（gpui-component 的 `story-web` 是生产先例）。
  未来要 Web 端，编译 GPUI 到 wasm——不保留任何 web 前端代码。
- 迁移期间旧版留在仓库作功能参考；**不向旧版添加新功能**，新功能只进 GPUI 版。
