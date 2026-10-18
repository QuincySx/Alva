# L4 · Notebook Extension
> nbformat4.5+ 实际单元编辑，不启动/执行 kernel。

## 地位

可选 Plugin；复用同 builtin.Workspace 的 pinned os.Root 安全能力。

## 逻辑

格式依据为 [Jupyter 官方 nbformat 文档](https://nbformat.readthedocs.io/en/latest/format_description.html)，本工具明确只支持有唯一 cell IDs 的4.5+，非完整 schema validator。

actual Tool/session/run/cwd 与 Guard/HITL 后，Coordinator 自持实际 shared global lease，覆盖 read→transform→原字节/准备结果引用→risk/intent→checked replace→真实outcome。唯一 cell ID、insert_before 真正位置、严格 source string/array 读取，源码换行原样保留；编辑code清旧outputs/execution_count，不伪称执行结果。未知其它字段保留。

## 约束

Tool参数及原notebook均拒绝重复键，避免editable/deletable保护字段被最后值覆盖；参数拒绝大小写别名。
仅现有 .ipynb、格式4.5+、20MiB/10000cells、源码128KiB；不自动upgrade/create、执行code或媒体理解。metadata.editable/deletable false 拒绝。原bytes/准备结果未脱敏按原件保留供恢复，只引用不自动入模，intent/outcome仍规范脱敏。Snapshot复用原root逐级pin父目录、祖先/target symlink和特权mode拒绝、保留mode+原子rename；同Workspace mutex/实际共享lease保护合作写者，旧hash拒绝已观察竞争。明确存在不合作外部writer最后check→rename窗口，不声称OS hash-CAS；失败可能已写时不得自动重放。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 编辑 | `edit.go` | 有界标准JSON/ID/保护字段、replace/insert/delete实际转换 |
| Plugin | `plugin.go` | 实际文件消费者、审批/真实globallease、原件/准备引用与事实 |
| 边界回归 | `boundary_test.go` | 重复cell/source参数与原notebook重复保护字段拒绝 |
| 回归 | `plugin_test.go` | 真文件/字节备份/模式/精确换行、outputs清理/metadata保护/位置与risk先行 |
