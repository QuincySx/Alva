# L4 · Native Interaction Adapter
> 真实人类提问/回答，与 HITL 权限审批严格分离。

## 地位

原生 Plugin；复用父包的解析、问答与回执，真实 Handler 缺失不能注册假等待或返回虚构答案。

## 逻辑

实际工具/会话/run/cwd、Guard 通过后，问题完整脱敏 artifact/intent 先保存，再等待显式 Handler。回答解析实际选项/自由文字，全文留同会话引用；模型仅小 receipt 后按需读取。宿主 QuestionBroker 独立于 ApprovalBroker，产品按实际根/typed 子关系筛选。

## 约束

Handler所有nil-capable typed-nil形式在构造时拒绝；输入严格校验重复键/Unicode/精确字段，call ID非空。
不读 ambient stdin，不把回答当 shell/file/network 授权；没有自动重放。问题8KiB/标题1KiB/metadata4KiB、2..4选项、答案64KiB、每run最多4并发，等待最多30分钟；取消/过期/旧run晚答拒绝。Handler 需真实响应 ctx；不以遗留 goroutine 假造完成。产品 UI/HTTP 接线由产品 owner 消费，未接时不可广告可用。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 父包能力 | `../question.go`、`../tool.go` | 有界问答、共享解析/等待/回执；本包只加原生政策 |
| Plugin | `plugin.go` | 实际 ask_human、handler等待和完整事实/引用 |
| 回归 | `plugin_test.go` | 实际宿主broker长回答落引用、选择解析、无handler/身份拒绝 |

通用 Tool 的构造不进入本包；只有需要原生会话、权限与耐久证据的产品装配才导入本 `native` 适配。Plugin.Name、工具名与既有记录 namespace 保持。
