# 03 — wasm 跑道预备（prefactor）

**What to build:** 让 wasm 档的地基处于"随时可开工"状态：修复内核主循环中已证实的 wasm 目标下 panic 路径（此前跨环境调研已定位）；文件工具的条件编译从"wasm 全部排除"放宽为"wasm 且非 wasi 才排除"，使其在 wasip2 下可用；CI 的 wasm 防火墙在现有 wasm32-unknown-unknown 检查之外增加 wasip2 目标。纯预备性重构，不引入新功能。

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] 内核主循环在 wasm 目标下无 panic 路径（以测试或编译期证据钉住）
- [ ] 文件工具组在 wasip2 目标下编译并注册，非 wasi 的 wasm 目标行为不变
- [ ] CI 防火墙脚本对既有 wasm-clean crate 集增加 wasip2 检查且全绿
- [ ] 本地与 CI 均验证通过，对 native 构建零行为变化
