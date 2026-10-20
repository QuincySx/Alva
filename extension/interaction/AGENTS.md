# L4 · Portable Interaction Tools
> 真实人类问答，可选择有界完整答案或引用回执。

## 地位
普通可选工具与问答契约；只依赖标准库、SDK 和纯 strictjson。Handler 由宿主显式绑定；引用存储是可选输出政策。

## 逻辑
NewTool(ToolConfig) 可直接注册到 SDK Builder.Tool；严格解析问题，身份仅来自执行上下文，只需 Handler，真正同步等待并校验答案；默认返回完整有界 answer_value，配置 References 才保存完整引用。ParseQuestion/Ask/Receipt 由普通 Tool 与 native 适配共同消费。普通 Tool 不自报管理超时，遵循宿主 timeout middleware 与调用 context；原生适配才自报管理超时，继续应用自身配置与 run 生命周期。

## 约束
不是权限审批，不读取 ambient stdin；没有自动重放。Handler/显式引用后端 typed-nil 拒绝，取消后晚答不接受，不用遗留 goroutine 假造完成。问题8KiB/标题1KiB/metadata4KiB、2..4选项、答案64KiB；输入拒绝重复键、错误 Unicode、未知或错误大小写字段与模型伪造身份。JSON回执最多16KiB；无存储超界明确失败，已配置存储失败不退回全文。AnswerReceivedError 保留 question ID 并说明已收到有效答案，原始原因可 Unwrap；不自动再问。引用后端负责宿主存储范围与脱敏；native/ 保留原生 authority、Guard、并发限额、intent/outcome 与期限。

## 业务域清单
| 文件/子目录 | 职责 |
|---|---|
| question.go | Question/Choice/Answer/Handler、有界验证与选项解析 |
| tool.go | ToolConfig、共享问题解析/真实等待与可选引用输出 |
| answer.go | 已收答案但回执失败的 typed error 与有界恢复信息 |
| tool_test.go | 真内存 SDK、大回答引用、同步取消/晚答拒绝、严格解析与实际宿主超时 |
| output_test.go | 无存储真SDK问答、已答保存失败不回退、超界不截断或再问 |
| native/ | 会话/Guard/并发/期限与耐久问答 journal 适配，复用父包能力 |

AnswerReceivedError.Recovery()与RecoveryRecord()是结构化恢复通道；Error文本在Join/timeout后不保证是JSON。默认harness保存schema1恢复事实，普通SDK可用errors.As读typed数据，不自动再次提问。
