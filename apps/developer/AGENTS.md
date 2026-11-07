# L6 · Shared developer options
> CLI与Hub共同消费的开发者接入测试、独立模型评估及执行owner。

## 地位
产品外围能力，不是SDK、Agent runtime或第九个命令程序；不依赖Hub、EngineRuntime、MyGo或具体存储。

## 逻辑
test使用真实interaction声明检查单响应工具选择/参数，不输出模型质量分。
eval使用独立版本化任务集、exact_text/exact_json和1..100权重；默认五个可审阅基础任务，可显式替换。
二者复用注入模型的integration/llm/eval执行/预算，不共用用例或成功含义。eval按重复trial出加权分、全部trial通过的任务数、延迟和实际usage；数据集含评分/权重的SHA与请求SHA共同归因。
RunCLI由alva developer test/eval调用，默认冷计划，--run才执行；测试失败exit1，评估低分正常出报告，执行故障/中断仍失败。
Service绑定程序提供的明确模型route及可选revision目标源，Plan/Targets/Snapshot冷读；Start原子准入后立即返回，由唯一异步job在首请求前冻结全部选中配方，跨窗口共享观察/Stop，不创建聊天session或执行广告工具。目标编辑/禁用不改变已冻结job；解析失败给failed/canceled快照，解析阶段也由同一owner取消/join。

## 约束
配置32KiB、数据集64KiB/最多64任务、prompt/期望各8KiB；8 routes/8名称、每例20trial、总调用默认32/最多256；默认输出2048/最多16384token、30秒/最多2分钟。
秘密仅显式CLI stdin映射或程序Provider配置，不发现环境/账户，不返回URL/key/prompt/原始响应/工具参数或思考。
工具名称使用ASCII字母/下划线开头、后续字母/数字/下划线/连字符的64字节子集，get_weather保留；CLI/UI共用校验。
先验证完整矩阵/全部route与凭据再调用，无retry/fallback。修改UI预览内容须重新预览；打开界面不调用模型。
API错误不混入质量分母，未执行无分，报告必须同时显示执行覆盖/错误；小型单响应任务分不证明多轮Agent、权限、Other回填、视觉或持久恢复能力。
Service取消/Close等待实际HTTP退出；超时仍保留owner可再Close，不用未退出goroutine假装硬超时。最新报告仅进程内保留，存储政策由产品另行选择。

## 业务域清单
| 文件 | 职责 |
|---|---|
| cli.go | 产品CLI前端、严格配置/凭据、完整调用预算及分开的报告/exit语义 |
| contracts.go | 五类真实问答工具契约检查与单名称变体，无工具执行 |
| dataset.go | 独立Task/Dataset、任务grader、评分与覆盖/版本指纹 |
| service.go | UI无关共享job owner、静态route/可选动态目标源、全矩阵冻结、冷计划、进度/结果观察与解析/HTTP真实Stop/Close |
| dynamic_test.go | 动态目标冷预览、后续配方解析/构造失败零HTTP，解析中Stop/Close超时保留实际owner |
| cli_test.go | 四协议实际HTTP、工具名称矩阵、行为/接入故障、预算/隐私/取消回归 |
| service_test.go | 冷UI服务、测试无质量分/评估独立任务、加权重复评分/失败语义、深拷贝与实际HTTP取消join |
