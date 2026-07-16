# 11 — jobs 体系集成 + 工具调用粒度 job 日志

**What to build:** 沙箱档位与既有 jobs 体系（submit/wait/status/result/list）无缝组合：submit 时声明档位与授权目录，全生命周期语义不变。job 日志按工具调用粒度记录 worker 行为（每次 CRUD/run_script/fetch/升级请求一条），批任务无需 token 流即有进度与审计可见性——这是 v1 不做流式 LLM 代理的补偿设计。

**Blocked by:** 05 — CLI 接通 wasm 档。

**Status:** ready-for-agent

- [ ] jobs submit 带沙箱参数的全生命周期 golden（submit→status running→result done），沿用无 daemon 的文件+pid 探活推导
- [ ] job 目录下有工具调用粒度日志，含时间戳、工具名、结果摘要
- [ ] 升级请求及其审批结果在日志中可审计
- [ ] wait 超时、worker crash 等异常路径语义与现有 jobs 一致
