# 07 — fetch + wasi-http 域名白名单

**What to build:** job 配置增加域名白名单（默认空=全拒）。run_script 的 JS 世界里提供 fetch 绑定，经 wasi-http 出网：白名单内域名成功、名单外域名以明确错误拒绝——与文件 preopen 同构的能力授权。任务级联网（抓参考资料）与基础设施级 LLM 调用（宿主代理）保持两条通道、互不共享授权。

**Blocked by:** 06 — run_script（fetch 绑定活在其 JS 世界里）。

**Status:** ready-for-agent

- [ ] 默认（未授权任何域名）下 fetch 一律拒绝
- [ ] 白名单域名 fetch 成功（本地 mock http 服务验证），名单外拒绝且错误对脚本可捕获
- [ ] 白名单不影响宿主 LLM 代理通道（两通道独立断言）
- [ ] 重定向到名单外域名被拒（防白名单穿透）
