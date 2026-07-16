# 10 — wasm-env 环境说明 skill（Explicit 常驻注入）

**What to build:** wasm 档 worker 的环境说明作为 bundled skill 维护，以 Explicit 注入策略常驻 worker 的 system prompt——worker 睁眼即知：自己在 wasm 沙箱里、授权目录挂载点、可用工具就三样（文件 CRUD / run_script / fetch 视授权）、授权外路径不存在别重试、没有 shell 重活走升级请求、批量活写脚本别逐文件、结果如何交付。

**Blocked by:** 09 — skill 触发四步（注入机制）；05 — CLI 接通 wasm 档（有 worker 才有环境可说明）。

**Status:** ready-for-agent

- [ ] recording-mock 断言：wasm 档 worker 的请求体 system prompt 含环境说明全文
- [ ] 说明内容与实际工具集一致性有测试钉住（工具增减时说明不至于漂移）
- [ ] 非 wasm 档 worker 不注入此 skill
- [ ] 环境说明覆盖：挂载点、三工具、无 shell、升级通道用法、批量脚本建议、结果交付方式
