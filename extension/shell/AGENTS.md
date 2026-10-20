# L4 · Shell Extension
> 显式宿主 shell、真实 owned job、同会话脱敏输入输出引用。

## 地位

可选 Plugin，默认不装配；不借 shell 能力宣称 OS sandbox。

## 逻辑

Guard/HITL、实际工具身份/会话/cwd/run 和持久 risk/intent 先于启动。Coordinator 使用同一个宿主 ToolLockRegistry 自持 global lease；前台真实退出、后台真实进程组清理之后才释放。standalone New仍是run owner；Hub显式Sessions是应用内native-session owner，成功Turn只释放当轮Lease reservation，后台继续归Sessions监管。读/写/kill管理工具不重入global lease。Hub另从actual admitted attempt延长既有product workspace lease，直到physical reap，避免与external engine/worktree mutation并发；不在待排他writer后再排一次reader。platform/system/process 清空环境、真实进程组回收；行 framing 后脱敏完整 I/O 留事实与 artifact，小输出仅为状态与分页 refs。

## 约束

显式批准绝对 shell exe/cwd/环境，无 ambient credentials、PID 操作或自动恢复重放。standalone在run end清理；显式Sessions在正常成功轮后保留后台，当前enabled run取消/失败、独立Stop、timeout/应用Close实际回收。最多64个session owners、8 jobs/轮/64 retained jobs每owner、32 MiB每owner总 I/O、8 MiB/2048 帧每 job、1 MiB 行、256 KiB 累计 stdin、10 分钟硬 timeout；溢出/记录失败停止且不返回成功前缀。已知字段和显式配置Env值（含无字段回显）在完整行/声明base64正文内脱敏；截断正文或存在显式Env值的未结束行整体省略，不保存凭据前缀。已知凭据脱敏不等于任意 secret 检测。Lease generation防旧runner重入/late Close偷释放新reservation；新Guard/reporter仅在无active attempt重绑，实际exe/env/cwd/authority在live jobs间不可变。显式Reset可清理owner，保留耐久records/artifacts；重启只将缺失outcome记unknown，不读PID、不执行旧命令。Sessions必须在facts store之前Close，timeout保留真实资源，可重试。物理 CleanupComplete 与 journal error 分开，不以 EOF/取消确认代替 reap。实际Tool在审批前登记in-flight调用，owner/run取消到达审批与管理回调；Close同时等待调用/事实写入结束，不能只等进程退出。管理调用不额外取得global/job lease，后台仍由原owner持有。参数拒绝重复键/非法Unicode，CallID必须非空。

Sessions.List与私有job snapshot均在锁内复制Task开始/结束时间和ExitCode指针，返回状态不授予owner元数据写入权。

存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 装配 | `plugin.go` | 显式配方、同会话生命周期、Close 与持久事实 |
| 应用内会话owner | `sessions.go` / `sessions_test.go` | 跨Turn后台资源、generation Lease、scoped分离状态/Stop/Reset、真实Close与重启unknown不重放 |
| 任务 | `job.go` | 实际 Task/进程、完整脱敏帧/输入、真实收尾 |
| 工具 | `tools.go` | 启动/输出分页/写 stdin/终止的真实消费者 |
| 调用收尾回归 | `cleanup_test.go` | 审批/后台stdin管理的取消、超时保留及真实join，歧义参数拒绝 |
| 回归 | `plugin_test.go` | 实际前后台输入输出/终止/超时、跨Agent全局lease、晚清理与装配顺序/身份门禁 |
