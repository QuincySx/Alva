# 横向支撑 · Windows line input
> 借用stdin文件/pipe的可取消逐行OS原语，不拥有REPL、审批或secret政策。
## 地位
Windows native-only，消费标准库和已有x/sys；CLI负责UTF-8/累计预算、交互与权限，secretinput继续独立持有密钥输入政策。
## 逻辑
GetFileType分派；pipe仅PeekNamedPipe已观察字节才ReadFile，单字节避免吃掉下一行；regular file同步有界读。行内容最多maximum bytes，尾随LF不占内容额度，与既有CLI reader一致。无后台reader/借用handle Close/flags/deadline修改。
## 约束
调用方必须是sole reader；取消只在本次同步read真实返回后结束。Console cooked read的取消语义未验明，明确拒绝，不以CancelSynchronousIo或关闭borrowed handle冒充安全取消。不是终端编辑器或可强制中断的磁盘IO沙箱。
## 业务域清单
| 文件 | 职责 |
|---|---|
| line_windows.go | bounded context-aware ReadLine、原样EOF partial与无readahead |
| line_windows_test.go | 真实继承fd0 pipe、多行保留/未EOF取消/精确行上限与超限/EOF原生helper回归 |
