# Granted workspace filesystem
> 持有真实 pinned roots 的受限宿主文件能力，与模型工具和 Controller 分开。

## 地位
独立 host 叶子；只消费 SDK、安全路径原语及自身实现，不依赖 extension、harness、storage 或 host 父包。
## 逻辑
Workspace 打开显式目录 grants 并保留 os.Root；Read/List/Search 返回有界纯数据，Update 在同一锁内读取、调用变换/备份回调并原子写入。Snapshot 对已存在文件执行逐级 pin、完整 hash/mode 检查和受检替换。
## 约束
不导出根句柄或让调用方重新打开绝对路径。Guard.CheckPath 保护每次操作，实际模型 effect/HITL 仍由 security Plugin 负责。Update 回调不重入同一 Workspace；回调失败不写入，文件变化/取消错误不能冒充成功。既有 rename 检查和 snapshot 都不是与不合作 OS 写者的 hash-CAS；Close 由实际 owner 在执行收尾后调用。
## 业务域清单
| 文件 | 职责 |
|---|---|
| workspace.go | grants、Resolve、单层目录、受限 read/atomic write 和根生命周期 |
| operations.go | Guard-bound ReadFile/ListDirectory/Update 与无根句柄的纯数据结果 |
| search.go | 字面搜索、路径 Guard、节点/字节/结果预算和实际扫描计数 |
| snapshot.go / snapshot_test.go | 已存在文件的 hash/inode/mode、逐级 pin 和不确定替换证据 |
| workspace_test.go | 非递归目录及授权/真实宿主操作回归 |
