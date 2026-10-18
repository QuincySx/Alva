# Review evidence
> Round 6 的可复现探针与命令输出。

## 地位
只读审查产物。
## 逻辑
UI 探针需临时放到 web/src/components 下执行 Vitest，server 探针需临时放到 desktop/tests 下执行 Cargo；运行后移回本目录。
## 约束
不得将复制的 server projector 当成真正 HTTP 集成；core 自带独立 workspace，禁止部署。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 核心 | core/ | 广播和代际 probe、常规 suite 输出 |
| UI | ui-round6.test.tsx / ui-probes.txt | 三项组件反例 |
| 基础验证 | web-tests.txt / browser-tests.txt | 常规前端结果 |
| HTTP seam | server-projection.rs / server-projector-source.txt / server-probe.txt | 正式 run 与投影接缝 |
| Scanner | scanner-results.txt | 六项契约变异输出 |
| 审批 UI | ui-r57.test.tsx / ui-r57-probes.txt | 中文丢失效果的中英对照 |
