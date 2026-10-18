# 横向支撑 · Native secret input
> 共享命令启动边界，有界读取显式stdin，不发现或保存凭据。

## 地位
仅native入口消费，不进入SDK、codec或协议转换层。

## 逻辑
独占借用文件read；Darwin/Linux通过受SyscallConn.Control保护的≤50ms poll/read取消真实pipe（含继承的blocking fd0），Windows通过PeekNamedPipe只读实际available bytes并≤20ms检查取消；不修改flags/deadline或关闭borrowed descriptor。其他平台仅在deadline支持时读取并join取消回调，否则明确拒绝。
显式regular文件和有限内存buffer同步有界读，前后检查取消；无后台reader与正文日志。

## 约束
交互终端/socket及未知blocking Reader明确拒绝，要求pipe或regular文件输入。
1..1MiB硬上限由调用者进一步收紧；不关闭caller文件、不修改stdio写端或环境。
regular磁盘读不是可抢占OS I/O，不宣称硬实时取消。

## 业务域清单
| 文件 | 职责 |
|---|---|
| read.go | file类型/有限buffer的有界read与实际平台pipe分派 |
| read_test.go | 真实open pipe取消、文件归还与边界回归 |
| pipe_unix.go / pipe_other.go | inherited fd0真实poll/read与其他平台可中断deadline fallback，无后台reader |
| pipe_windows.go | SyscallConn保护的PeekNamedPipe/ReadFile，只读真实available bytes并可取消，不改变HANDLE flags |
| pipe_inherited_test.go | Unix/Windows实际test-binary fd0继承，open producer取消/精确EOF/超限不等待EOF；旧实现真实失败 |
| privatefile_unix.go / privatefile_windows.go / privatefile_other.go | 显式已有文件的只读打开；Unix验证UID/mode/regular/nlink且不跟随symlink，Windows复用同handle DACL/reparse/hardlink检查；无发现/创建/修改 |
| privatefile_unix_test.go | 实际只读/缺失不创建、宽权限不变、symlink/hardlink/FIFO拒绝；Windows同原语的OpenRead反例在privatefs |
