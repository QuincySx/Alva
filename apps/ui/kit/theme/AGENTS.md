# Kit theme
> 设计 token 的 Go 单一来源：生成的 Palette 与常量，加到 MyGo ui.Theme 的映射。

## 地位
Alva Kit 的数据层。`tokens.json` 由设计源导出，`tokens_gen.go` 由生成器产出，二者一并提交；`apply_desktop.go` 是唯一允许碰 MyGo 的文件。
## 逻辑
`Light`/`Dark` 两套 `Palette`（20 个颜色角色加 `Popup` 阴影），无类型常量承载字体、字号、间距、圆角、控件尺寸与动效；Narrow 视口的值是 `SizePanel(narrow)`、`SizeViewportWidth(narrow)` 这类函数（侧栏 220、设置导航 208、侧栏标签在 Narrow 下不变，没有 72 图标栏；页头 `SizeHeader` 36，资源面板头 `SizePanelHeader` 52）。`Of(c)` 按窗口外观取色，`Apply` 映射到 `ui.Theme`，`ReducedMotion(c)` 汇总系统与应用偏好，`RegisterFonts` 注册内嵌字体（与设计字体目录逐字节相同）。
## 约束
不手改 `tokens_gen.go`；改 token 走导出脚本、`go generate`、测试三步。`go test ./apps/ui/kit/theme/...` 不依赖 Python（`TestTokensJSONMatchesDesignSource` 找不到 python3 或 smallpen CLI 时 Skip 并写明原因）。
## 业务域清单
| 文件 | 职责 |
|---|---|
| theme.go / tokens.json / tokens_gen.go | Color、Shadow、`For`、内嵌字体；导出的 token JSON；生成的 Palette、常量与 Narrow 函数 |
| apply_desktop.go | `Of`、`Apply`、`UI`、`ReducedMotion`、`RegisterFonts`（desktop tag） |
| design_sync_test.go | 不依赖 CLI、CI 必跑：tokens.json 与设计包 tokens.css 逐项一致（亮暗、Narrow、number、阴影），设计包与 `build_next.py --check` 一致 |
| theme_test.go | JSON 与设计包一致（需 smallpen）、Palette 覆盖全部设计颜色、Narrow 值与密度、字体与设计字体一致 |
| fonts/ | Alva UI Sans SC 400/600、JetBrains Mono 与其 OFL 声明，来自 docs/design/alva/fonts |
