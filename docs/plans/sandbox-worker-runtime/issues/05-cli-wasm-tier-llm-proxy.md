# 05 — CLI 接通 wasm 档 + 阻塞式 LLM 宿主代理

**What to build:** 用户可用派活参数声明 wasm 档与授权目录（形如 --sandbox wasm --grant <dir>），headless 跑一个真实的文件加工任务端到端。worker 的 LLM 调用由宿主代理：guest 通过 import 函数递出 messages，宿主贴 API key 转发 provider，阻塞式返回完整响应——key 从头到尾不进沙箱。授权外访问的失败以明确错误对模型可见，任务失败时带原因返回。

**Blocked by:** 04 — wasm 宿主 runner + preopen 圈禁。

**Status:** ready-for-agent

- [ ] headless 派活端到端：授权目录内读→分析→写，任务成功返回（recording-mock 与真 provider 各验一次）
- [ ] recording-mock 断言：请求由宿主发出、带 key；guest 侧不可能拿到 key（以接口形状证明）
- [ ] 授权外访问：错误进入 worker 上下文（模型可调整策略），最终失败带原因返回调用方
- [ ] 未知/非法 --sandbox、--grant 参数响亮报错
