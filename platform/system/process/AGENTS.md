# 横向支撑 · Owned Process
> stdio 子进程的宿主生命周期原语，不是权限沙箱。

## 地位

Native-only 机制；启动授权在外层 Plugin/产品。

## 逻辑

显式配置直接 argv；Unix保持leader未回收直到清理group，Windows以创建时JOB_LIST绑定非继承KILL_ON_JOB_CLOSE job。Close幂等，Windows还确认ActiveProcesses归零与真实leader退出后才报告物理完成。

## 约束

不继承凭据、不执行隐式shell、不自动重启。Windows仅接受显式本地.exe或系统目录裸名，HANDLE_LIST只含三条stdio，可显式将regular file作为只读seekable stdin，拒绝Unix fd3 ABI；需要Windows10/Server2016及以上的创建时JobList，失败不回退到先运行后Assign或PID树扫描。原生Windows测试/本机交叉编译分别记录。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Lifecycle | `process.go` | 配置校验、stdio、显式关闭与错误 |
| Shared configuration | `config.go` | 保留独立跨平台配置，Unix FD与Windows HANDLE能力不冒充彼此 |
| Windows owner | `process_windows.go` | 原子JobList+准确stdio HANDLE_LIST、clean SystemRoot/env、job kill/active-zero/真实exitcode与异常owner退出保护 |
| Windows acceptance | `process_windows_test.go` | 真实子孙job归属/自然leader退出、owner不Close直接退出、ambient inheritable file拒绝、大小写env与cmd脚本反例 |
| Read descriptors | `readfiles_unix.go` / `readfiles_other.go` | 合计最多8个显式regular/目录FD、Unix实际O_RDONLY检查；目录句柄在regular后，仅pinned cwd不是只读树沙箱；不继承ambient FD，未实现平台拒绝 |
| Seekable input regression | `input_file_test.go` | actual child从文件stdin seek/read、写失败、借用file不关闭、非regular拒绝；不创建copy goroutine |
| Unix | `process_unix.go` | 自有 process group，先 kill 再 reap |
| Darwin | `process_darwin.go` | killpg EPERM后最多100ms只重观测，不重发signal/不reap；仅waitid真退出且全组无live时确认XNU退出过渡，真实live/inspection失败保留原错误 |
| Non-reaping wait | `wait_darwin.go` / `wait_linux.go` / `wait_other.go` | libc waitid / Linux WNOWAIT 观察自然退出，保留 PID 所有权直到组清理 |
| Other | `process_other.go` | 未实现平台拒绝启动，不冒充完整回收 |
| Regression | `process_test.go` | 环境隔离、真实非零退出码、EOF 不是退出与取消回收 |
| Darwin exit regressions | `process_darwin_test.go` | 首次未waitable→真实自然退出、禁止reap/额外signal、live leader/live descendant/inspection失败保留EPERM及128个partial-prefix自然退出压力 |
| Stream framing | `lines.go` / `lines_test.go` | arbitrary Read/UTF8边界合为有界原字节完整行，EOF partial/超限明确，不持有脱敏政策 |
