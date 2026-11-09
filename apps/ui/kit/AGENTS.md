# Alva Kit 组件库
> MyGo `*Base` 控件之上的 Alva 薄皮肤：产品无关的设计系统组件，视觉全部来自设计 token。

## 地位
ADR 0014 的实现，位于共享 UI 层；留在本 module，不对外独立发布。消费者是 `apps/ui/chat|modules` 与两个桌面产品（K4 迁移），反向依赖被架构测试禁止。控件文件受 `desktop` build tag 保护，`theme`/`icon` 的数据与生成器无需 MyGo 即可测试。

## 逻辑
即时模式：`Name(c, 数据, NameProps) ui.Element`。Props 零值就是设计默认；设计的配置轴是枚举（`ButtonKind` 等）；状态轴只有 Disabled／Busy／Invalid／ReadOnly 进 Props，Hover／Pressed／Focus 来自运行时（画廊与测试经 `internal/force` 强制）。受控值用指针；动作用受门控的 `OnClick`（Busy／Disabled 时不触发，MyGo 的 `Clicked()` 无法门控）；图标按钮的可访问名称是必填位置参数；`Field` 把标题与状态注入其内的 Input／TextArea／Select。一套密度（控件 32、图标按钮 28、正文 13），无尺寸档。减弱动效：`theme.Apply` 在应用偏好为真时 `DisableAnimations`，自绘 Loader 读 `theme.ReducedMotion(c)`（系统偏好或本帧 Instant）。Menu 在 `PopoverBase` 上自写 roving focus（按 rune 做首字母匹配；Esc、Tab 或选中后焦点回触发器，外部点击关闭不动焦点；状态挂在触发器元素上；竖向 FocusGroup、Home/End、首字母、Esc、关闭后焦点回触发器），因为 MyGo 0.7.4 没有 `MenuBase`。
token 单一来源是设计源的 `token_definitions()`：`python3 -I docs/design/alva/source/export_tokens.py` → `go generate ./apps/ui/kit/theme ./apps/ui/kit/icon` → `go test ./apps/ui/kit/...`，失败项就是要对齐的组件，JSON 与生成文件同一个 commit。

## 约束
只依赖标准库、MyGo 与自身（`boundaries_test` 与 `kit_test` 守护）；组件内不出现颜色与尺寸字面量（`TestKitHasNoLiterals`）、不出现工具名或引擎名（`TestKitHasNoProductVocabulary`）。不加 `ALVA_KIT_SHOTS` 之外的文件输出；不提交截图 PNG。新增设计组件必须进画廊，或进 `gallery` 测试里只减不增的 deferred 清单。桌面代码新增颜色字面量、原生按钮、自制浮层壳会被 `kit_adoption_test` 拒绝。

## 业务域清单
| 路径 | 职责 |
|---|---|
| doc.go / kit_desktop.go | 包说明；Tone、焦点环、SVG 缓存、Loader、浮层表面等共享件 |
| text / icon / button / iconbutton _desktop.go | Text（字阶）、Icon（Lucide 16/14）、Button（4 Kind、Busy、IconOnly、门控 OnClick）、IconButton（28、Selected、必填名称） |
| inline_desktop.go | InlineAction：输入框内的 14px 轻量动作（无底色、hover 只变色、同高命中区、框内焦点描边），Password 的显隐与以后的清除按钮共用 |
| input / field _desktop.go | Input、Password（掩码、尾部显隐切换自带名称）、TextArea（Invalid、ReadOnly、Busy、Disabled）与 Field（标题、提示、错误、状态注入） |
| select / toggle / tooltip / menu _desktop.go | 泛型 Select、Checkbox（含 Mixed）与 Switch、Tooltip、Menu（roving focus） |
| examples_desktop.go 与 *_examples_desktop.go | `Page`/`Axis` 与各组件按设计轴展开的示例页，画廊与覆盖闸共用 |
| *_desktop_test.go | 交互、几何/像素、强制态与名称审计测试 |
| theme/ | token 数据（tokens.json → 生成的 Palette 与常量）、`Apply`/`Of`/`ReducedMotion`、内嵌字体 |
| icon/ | 设计图标清单、嵌入的 Lucide 1.61.0 SVG 与名称表 |
| internal/force/ · internal/audit/ | 强制 Hover/Pressed/Focus 的作用域；可访问名称审计出口（仅 kit 内可导入） |
| kittest/ | 像素读取、居中 harness、截图与名称审计测试辅助 |
| gallery/ | 画廊 App：组件列表、亮暗与 Narrow 开关、变体矩阵、覆盖闸与全页渲染测试 |
| testdata/design-components.json | 设计组件目录（名称、轴、变体数），由导出脚本生成 |
