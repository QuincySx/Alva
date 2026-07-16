# 06 — run_script（QuickJS，无模块系统）

**What to build:** worker 获得 run_script 工具：提交一段 JS，在沙箱内的 QuickJS 引擎里执行，脚本通过绑定的文件函数批量操作授权目录——一次工具调用完成 N 个文件的修改，token 成本与脚本源码同量级。scope 锁死：无模块系统、无 npm、无 Node API，只有 ES 内置 + 十余个文件绑定函数。带执行超时与内存上限。

**Blocked by:** 04 — wasm 宿主 runner + preopen 圈禁。

**Status:** ready-for-agent

- [ ] 一次 run_script 调用批量修改多个文件，宿主侧验证全部落盘
- [ ] 脚本内文件访问同样被 preopen 圈禁（越狱断言复用缝 2 模式）
- [ ] 超时脚本被终止、超内存脚本被终止，错误信息对 worker 可读
- [ ] 绑定函数清单作为契约文档随代码维护（一页纸），无 import/require 能力的断言
