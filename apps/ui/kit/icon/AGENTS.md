# Kit icons
> 设计的 68 个图标名加 PanelLeft，以及内嵌的 Lucide 1.61.0 SVG。

## 地位
Alva Kit 的图标数据层，不导入 MyGo；解析与绘制在 kit 包的 `Icon`。
## 逻辑
`icons.json` 是设计清单 `docs/design/alva/fonts/icons.json` 的副本；生成器产出 `Name` 常量与 Lucide 文件名表；`lucide/` 内嵌设计目录里同名 SVG；`PanelLeft` 是 Panel 的水平镜像（`FlipX`）。
## 约束
只有一套 Lucide 1.61.0；不加第二套图标，不改 SVG 几何。更新清单后重新 `go generate ./apps/ui/kit/icon`，测试会比对清单副本、嵌入文件与设计目录。
## 业务域清单
| 文件 | 职责 |
|---|---|
| icon.go / names_gen.go | `Name`、`Data`、`Names`、`FlipX`、`Files`；生成的名称常量与映射 |
| icons.json / lucide/ | 设计清单副本；内嵌 SVG 与 LICENSE |
| icon_test.go | 清单、嵌入、设计 SVG、设计 Icon 轴一致，每个图标是 24 单位 Lucide 1.61.0 |
