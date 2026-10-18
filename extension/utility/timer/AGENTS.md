# L4 · Portable Timer Tool
> 同一普通Tool供独立SDK与原生Utility Plugin使用。

## 地位
可选纯能力叶子；只依赖标准库和kernel/wire，不依赖SQLite、native session、Guard、host或integration。

## 逻辑
显式NewTool创建sleep；实际tool/call匹配后严格解析单个4KiB UTF-8 JSON，使用真正timer等待0..300000ms，ctx取消返回错误且不声称完成。

## 约束
不访问任何会话状态、文件、网络或权限，不需要原生会话装配；不是第二种Plugin或loop。Coordinator/ManagesOwnTimeout避免持有宿主global执行锁，执行仍同步等待真实完成或取消。

## 业务域清单
| 文件 | 职责 |
|---|---|
| timer.go | NewTool与无宿主依赖的实际等待 |
| timer_test.go | 真正内存SDK model/tool/model、取消和无harness参数校验 |
