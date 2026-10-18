# 横向支撑 · System OS 原语
> 不进入可移植 SDK 的显式宿主能力。

## 地位

协议、工具与资源 owner 可显式消费；与 platform/host 的 Agent 执行/审批所有权分开，不决定产品授权或读取用户全局配置。

## 逻辑

process 提供自有进程组/真实回收，media 提供显式字节派生与播放，loopback/httpjson/secretinput 提供有界本地传输与输入原语。调用方继续拥有执行、审批、存储和关闭顺序。

## 约束

子进程清空继承环境，仅允许安全 PATH/locale 与显式配置；未实现的平台能力必须明确拒绝。共享原语不隐式启动模型/进程，也不把 low-level I/O 当成产品执行权威。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Owned process | `process/` | 明确argv、清洁环境、自有进程组/真实cleanup与有界原字节流framing |
| Explicit media | `media/` | 原字节有界真实派生和完整来源，不自动向模型投递 |
| Loopback HTTP gate | `loopback/` | 共享numeric Host/Origin/256-bit Bearer，不引入Hub/协议/执行政策 |
| Strict JSON | `httpjson/` | 共用Unicode/重复键/深度/DTO精确字段校验，不属于协议转换或授权 |
| Secret input | `secretinput/` | 命令stdin有界读与真实管道取消，无后台secret reader |
| Windows file line input | `lineinput/` | sole-reader文件/pipe逐行可取消、无readahead，不决定REPL/审批或secret政策；console明确unsupported |
| Terminal text | `terminaltext/` | direct/Hub CLI共用的纯C0/C1显示转义，不更改模型/执行参数或原始历史 |
| Windows private filesystem | `privatefs/` | 仅Windows原生DACL/owner/reparse/hardlink与原子私有创建，供独立storage/browser/snapshot适配消费 |
