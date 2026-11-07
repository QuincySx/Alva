# 横向支撑 · Single-response measurement
> 注入模型、版本化用例与评分函数，测量一次完整模型响应。

## 地位
SDK外部的可复用评测库，只依赖标准库、abi与wire；不选择Provider、存储或UI。

## 逻辑
Plan校验完整用例/调用预算并冻结请求，返回planned回执，不连接模型。
Run顺序执行明确trial，复用既有StreamAccumulator校验完整终态，区分passed/failed/error。
Case.Grade是宿主可信代码，只返回固定有界finding code；模型输出只供内存中的评分函数检查。

## 约束
只测量single_response，不决定接入测试/任务评估的产品语义，不执行广告工具、不产生宿主权限或另一个Agent runtime。
最多64个用例/每例20次、256次调用；默认32次预算、30秒/请求，最多2分钟；请求128KiB、响应256KiB、4096chunks。
先校验全部用例再调用；每trial深拷贝，按ID/版本/请求SHA归因，无自动retry。
取消返回已发生的部分结果，未运行不算失败/成功；错误保留已观察usage，不记录原始错误/正文/参数/思考签名。
Model需响应ctx并同步join，不能用未退出goroutine伪装硬超时。Target为配置声明，不证明服务实际模型身份。
Grade必须避免把敏感内容编码成finding；库只校验code边界，不承诺识别任意秘密。

## 业务域清单
| 文件 | 职责 |
|---|---|
| eval.go | Case/Target/Options、无网络Plan、注入Run、完整流与有界报告 |
| eval_test.go | 预算预检、版本/指纹/隔离、行为与执行故障区分、取消/超限/截断与隐私回归 |
