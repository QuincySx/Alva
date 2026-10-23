# Hub workspace resources
> 固定设计字体与单一图标家族，随原生产品嵌入。

## 地位
仅为呈现资源，不属于SDK、执行owner或设计工具运行依赖。
## 逻辑
字体400/600与Lucide原始SVG从现有正式设计逐字节复制；workspace_assets_desktop.go一次注册字体并缓存解析图标，任意聊天文字保留平台字形回退。
## 约束
保留provenance.json实际SHA与完整OFL/ISC/MIT许可；不逐图标改画布/笔画中心，不逐标签改基线。没有全量CJK字体覆盖或跨平台像素一致性承诺。
## 业务域清单
| 路径 | 职责 |
|---|---|
| fonts/ | Alva UI Sans SC400/600、完整OFL及原设计度量来源 |
| lucide/ | 固定Lucide1.61.0原始SVG与完整ISC/Feather MIT许可 |
| provenance.json | 上游、版本、来源路径、许可与实际嵌入文件SHA |
