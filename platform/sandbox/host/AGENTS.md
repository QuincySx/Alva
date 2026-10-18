# 横向支撑 · WASIp1 native host
> 每 job 隔离的 wazero runner 与显式文件/网络/升级能力。

## 地位

native 叶子包，仅依赖标准库、wazero、wire、kernel 的模型最小合约和 sandbox ABI；provider/key/PermissionMode 与持久化均由 caller 保留。

## 逻辑

`Run` 同步持有 fresh runtime、模块和 os.Root；目录桥不经默认 DirMount。exchange/read 仅保留一个有界响应；模型代理完整保留 chunks，fetch 手动逐跳授权，escalation 需显式 opt-in + Approval + Executor 三项。
`Runner`显式拥有最多16种模块/燃料配置的编译缓存（不是实例/授权缓存），每job仍fresh runtime；Close拒新任务并等待实际活跃任务退出才释放cache。首次native编译时间属于总deadline，不能用它冒充已验证guest指令中断。
QuickJS原始/计量代码冷编译也属于job总deadline；成功编译且确认job未取消后，才启动独立10秒的实例初始化/脚本执行期限。编译失败不执行脚本，不能把编译耗时当作脚本已运行；wazero同步编译仍可能延迟响应取消，Run等待真实退出。
真实SDK/fuel与SDK/QuickJS/fuel集成测试给fresh Runner三分钟冷启动预算；tiny-module指令耗尽和明确取消测试仍用各自原有限额，不通过放大那些执行期限掩盖执行错误。

Fuel为显式opt-in（Limits.Fuel/CLI --wasm-fuel，1..1e12，0关闭）。版本1在每条直线基本块执行前预扣：除unreachable/nop/block/loop/else/end/return/drop为0外，每个原operator为1；calls/control结束基本块，循环back-edge必经新检查。耗尽不执行当前块，剩余计数不是CPU时间/已经执行的精确指令数。原模块必须先以相同Core V2 validator验证，新计数器不得使原越界index合法；追加global不重排原索引，保留函数/table/data，原start先于_start。重写后重新验证，最多128MiB；debug/source-map offsets失效故丢弃对应custom sections。没有换wazero fork或给guest ABI加政策。

## 约束

- 不继承 env/stdin/网络代理，不开放 WASI socket；默认无 grant、无 fetch domain、无 escalation。
- os.Root 拦截路径与 symlink 逃逸，但不隔离已授权树内 hardlink、设备/mount 或其他宿主进程；只允许 regular file/dir，拒绝 guest 创建 link。
- 目录模式只读在 host syscall 桥上执行，guest policy 不是授权；文件每个 20 MiB、每 job 写入 64 MiB、最多 256 open handles。
- 必须实际等待同步 callback 退出；不响应 context 的可信 host callback 可拖住 Run，不能报告已终止。deadline与指令fuel独立；host callback/批量内存字节不按指令数计费。
- observer 为 raw bytes、guest audit 标注不可信；caller 负责脱敏、持久化与引用索引，失败即真实失败。
- 全部exchange默认8192次、host/guest IO总量64MiB，超限真实停交互，不宣称仍有完整超限正文；输出超限prefix明确标注`*_prefix_limit`。默认host单响应output token ceiling 16384，guest不能放大；extra_body默认不授权。
- References/History是caller实际固定会话的只读能力；host验证请求和结果的页/搜索/UTF8/cursor预算，不接收guest指定owner/session、不提供写存储或数据库挂载。全部请求/结果同样计入exchange/IO与完整observer。
- Windows普通文件路径复用Go1.27 Root；NOFOLLOW先Root.OpenRoot固定parent再NtCreateFile单leaf/OPEN_REPARSE_POINT，验证exact handle无reparse才truncate；不依赖新Windows才支持的OBJ_DONT_REPARSE。不允许device/ADS/out-of-root path；Windows原生回归已写但本机仅交叉编译，实际运行仍未验收。其他未实现平台继续明确拒绝。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `types.go` | 显式 job/config/observer/升级契约 |
| `runner.go` | Runner编译缓存生命周期、fresh runtime、限额、内存 exchange 与模型桥 |
| `fuel.go` / `fuel_test.go` | 原模块先验→Core V2基本块计量→wazero再验；private per-instance counters/原start顺序、双backend语义对照、越界global不能借meter变合法及实际Go guest耗尽反例 |
| `script.go` / `script_files.go` / `script_test.go` / `script_unix_test.go` | host opt-in的fresh QuickJS core WASM实例；同job pinned guest roots/write budget/fetch grants、32MiB tracked heap/512KiB soft C stack/10秒独立deadline与reference-only返回；script指令共享实际job fuel、64MiB child reservation在整体linear-memory budget内；实际现代JS/Resizable TypedArray及同实例栈溢出恢复回归 |
| `scriptwasm/` | 固定QuickJS-NG源码checksum、可重建C reactor、无libc模块/loader的trusted bootstrap、二进制hash/MIT许可与显式maintainer构建脚本 |
| `filesystem.go` / `filesystem_test.go` | os.Root-backed WASI读写；目录mutation intent不扩大返回的RDONLY file data权限，append Pwrite拒绝忽略offset |
| `open_unix.go` / `open_windows.go` / `open_other.go` | Unix nonblock/NOFOLLOW、Windows普通Root与atomic no-follow叶子打开；不支持平台fail-closed |
| `open_windows_test.go` | 实际Windows创建/append/truncate、device/ADS/escape/in-root symlink nofollow不截断；Windows机器运行与cross-build分开 |
| `fetch.go` | 逐跳域名闸、无 ambient proxy 的 bounded HTTP |
| `runner_test.go` / `capabilities_test.go` / `budgets_test.go` / `testdata` | 真Go guest编译/执行、SDK文件引用、取消实际退出、越权/逐跳fetch/HITL/限额/runner所有权反例 |
