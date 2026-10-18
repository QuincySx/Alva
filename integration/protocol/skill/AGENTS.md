# 横向支撑 · Skill Protocol
> SKILL.md 解析、显式来源仓库与三级渐进加载。

## 地位

协议能力，不执行 Skill 脚本，不拥有 Agent 授权或系统提示装配。

## 逻辑

先列元数据，点名再加载正文，资源单独读取。auto 目录隐藏 explicit 项，点名仍可读取。
多个显式根按传入顺序覆盖同名，不读取环境/全局目录；安装只写唯一 writable 根，删除是归档。

## 约束
Product project roots use `OpenWithin(savedWorkspace, relative, writable)` to pin the existing child directory via the approved workspace descriptor, reject symlink ancestors and never resolve outside it. FS owns its descriptor; creating the project directory remains an explicit product action.

严格名称、UTF-8、frontmatter 和字节预算；拒绝 symlink/特殊文件及路径逃逸。
资源只是字节，不执行；Strict 的工具声明不能被当作真正授权。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Types and parser | `skill.go` | 元数据、正文、注入策略与完整 YAML 解析 |
| Filesystem | `fs.go` | pinned root 仓库、有界读取/安装与可恢复归档 |
| Regression | `skill_test.go` | 渐进加载、YAML、边界、路径与可恢复归档回归 |
