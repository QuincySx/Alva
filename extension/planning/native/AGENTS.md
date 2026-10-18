# L4 · Native Planning Adapter
> SDK typed Task 驱动的真实手动工作流与持久 Plan 笔记。

## 地位

原生 Plugin 与默认持久 Service；复用父包普通 Tool，状态只住同 native typed-record authority。

## 逻辑

实际 Tool/session/cwd/run + Guard/HITL 后消费同一Service；Config.Service显式替换、Service()与typed bus均返回已接受实例，普通nil才选默认nativeService，typed-nil拒绝。替换时不读取/写入默认planning snapshot或迁移旧状态，外部Service的生命周期、并发、校验和业务持久化由提供者负责。
默认local_workflow Task严格转换、依赖DAG、owner/metadata/notes真正保存；128 KiB状态快照用单SQLite事务compare-and-append防旧cache覆盖。恢复只取exact namespace/type最新metadata与有界正文，不加载无关conversation。大详情/备注仅refs。

## 约束

纯任务记录不启动/停止进程或 agent、不虚构 output path。task_stop 只取消 workflow；task_output 只读显式 append 的备注。todo_write 保存会话笔记，不偷偷写 CLAUDE.md/任意 path。旧 Rust enter/exit plan tool 仅文案 stub，未照搬：本插件不注册虚假的模式切换工具，也不调用共享 PM 全局 SetMode。正确的独立 policy-change 准入、可信宿主批准与直接父 ceiling 尚待接线；未声称权限收紧/解除已完成。工作区 worktree 是另一能力，未冒充已迁。

默认后端128 tasks、每项20 deps/32 notes、description4KiB、metadata4KiB、notes64KiB、状态128KiB；超界/记录失败明确报错。恢复不可重放执行。Service是已绑定的可信host capability，不接受用户owner/session授权输入。
此Plugin的实际native log/Guard与会话引用authority保持：替换业务Service不替换执行身份，也不把session字符串当授权。外部Service的输出引用必须能由本实际会话reader读取；详情投影严格编码、≤128KiB并保留到本会话，不将marshal失败当空成功，也不宣称外部业务状态天然耐久。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Native Service | `service.go` | 消费父包 Service/Task 契约，真实 DAG/更新/Plan 引用、冷恢复与乐观原子快照 |
| Plugin | `plugin.go` | 同会话权限与生命周期、bus 能力装配 |
| Tools | `tools.go` | 父包七 Tool 的实际身份/Guard 适配与原生详情保留 |
| 回归 | `plugin_test.go` | 真实恢复/DAG/状态/元数据/备注、旧缓存冲突与身份/坏快照失败 |
| 替换回归 | `replacement_test.go` | 真SDK消费自有Service、bus/getter同实例、原生详情证据、默认状态隔离及身份/坏投影/取消拒绝 |

通用 Tool 的构造不进入本包；只有需要原生会话、权限与耐久证据的产品装配才导入本 `native` 适配。Plugin.Name、工具名与既有记录 namespace 保持。
