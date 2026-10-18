# L4 · File tools
> 把明确授予的宿主文件能力接入模型调用。

## 地位
消费 platform/host/workspace 与 SDK Guard/reference；不拥有根句柄，不要求其他能力依赖整个工具集合。
## 逻辑
FileTools 提供 read_file/write_file/edit_file/list_directory/search_files。读先持久化为引用，写在 Workspace.Update 的串行变换回调中先保存旧文本备份再返回新字节。工具保持实际 session/workspace 绑定、显式调度/effect 与 1 MiB 写入预算。
## 约束
参数拒绝 null、重复键、大小写别名、非法 Unicode 和未知字段。批准/引用存储/真实 IO 失败必须返回执行错误；仅既有模型可恢复的参数/二进制/歧义错误成为 IsError 结果。保留原 JSON 输出、搜索预算、备份和审批政策；不提供同名工具的隐式信任。
## 业务域清单
| 文件 | 职责 |
|---|---|
| files.go | 工具目录、严格参数、调度/effect 与结果编码 |
| execute.go | 实际宿主调用、引用投递、备份/编辑与错误分类 |
| files_test.go | 真实引用/备份、会话/路径/symlink 和搜索回归 |
| boundary_test.go | 含糊写入参数拒绝及原文件无损 |
