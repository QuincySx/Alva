# environment
> 嵌入式运行时环境管理（Sub-8）

## 地位
管理 srow-agent 内嵌的运行时组件（Bun、Node.js、Python、uv、Chromium、Qwen），负责版本检测、安装、更新和路径解析。

## 逻辑
```
EnvironmentManager
  ├── ResourceManifest   (manifest.rs)  — 期望版本与工件配置
  ├── InstalledVersions  (versions.rs)  — 已安装版本追踪
  ├── Installer          (installer.rs) — 提取/下载/校验
  ├── RuntimeResolver    (resolver.rs)  — 可执行文件路径查找
  └── EnvironmentConfig  (config.rs)    — 基础目录与平台检测
```
启动时 `ensure_ready()` 对比 manifest 和 versions.json，对缺失/过期组件执行安装。

## 约束
- 下载功能当前为占位（TODO），仅支持本地 packages/ 目录提取
- 支持 zip-flat、tar.gz、qwen-zip 三种归档格式
- 路径解析按平台自适应（macOS/Windows/Linux）

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | EnvironmentManager 主体、EnvironmentError、ensure_ready 入口 |
| config | `config.rs` | EnvironmentConfig、平台检测 detect_platform |
| manifest | `manifest.rs` | ResourceManifest、ComponentVersion、ArtifactConfig、ArchiveFormat |
| versions | `versions.rs` | InstalledVersions、VersionStatus、版本对比逻辑 |
| resolver | `resolver.rs` | RuntimeResolver、按组件/平台查找可执行文件 |
| installer | `installer.rs` | Installer、zip/tar.gz 提取、版本记录更新 |
