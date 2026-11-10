# Kit gallery
> 按设计轴展开每个组件全部变体的视觉审阅页与覆盖闸。

## 地位
`cmd/alva-kit-gallery` 的内容。它不是产品界面，只为人工审阅与测试截图服务。
## 逻辑
左侧按类别列出 `kit.Pages()`；顶栏有暗色与窄视口开关；右侧是所选页的变体矩阵，每格标注轴值。`Flat` 只渲染矩阵，供整页截图。`TestGalleryCoversDesignCatalog` 要求设计目录里每个 Alva 组件要么有页（轴与变体数相同），要么在只减不增的 deferred 清单里写明归属期。
## 约束
依赖 kit 与 MyGo；不接产品类型、存储、网络或模型。截图经 `ALVA_KIT_SHOTS=<目录>` 生成，不提交。设置 `ALVA_KIT_DESIGN_DIR=<目录>`（`source/export_design.py` 导出的 `<组件>.png`）时，每页左侧显示设计稿、右侧是实现；缺文件时给出导出提示，不提交 PNG。
## 业务域清单
| 文件 | 职责 |
|---|---|
| gallery_desktop.go | App、列表、工具栏、矩阵与单元；设计稿对照列（DesignDir） |
| gallery_desktop_test.go | 覆盖闸、全页亮暗渲染与截图、矩阵无缺漏、工具栏与导航交互 |
