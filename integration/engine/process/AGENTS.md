# 横向支撑 · JSONL engine adapters
> Claude/Codex/Pi 完整外部 harness 的原生子进程协议适配，不是 Provider 包装。
## 地位
原生外部 Agent 适配器：消费 integration/engine 根契约、程序必传的storage.BindingStore、owned platform/system/process 与私有文件能力；不依赖 Hub 产品状态或 Rust 桥。Claude/Codex/Pi 当前在本 package 按 Vendor 分派共享实现，不宣称一对一迁成三个独立 adapter 包，也不是 SDK 层。
## 逻辑
显式批准宿主/账户访问后启动指定程序，使用私有HOME与显式env；stdout/stderr/stdin经完整行framing、显式环境秘密及共享known-field脱敏落私有有界journal，产品只收事件/引用。stderr单行超1MiB正文省略并失败，不能保存截断秘密。原生result与真实进程结束共同决定终态，取消kill/reap，Unknown vendor帧不触发执行。Pi只恢复匹配header的确切文件，无关启动中断的空/残缺header不阻断其他已绑定会话；目标缺失仍失败。
## 约束
不发现/启动用户真实CLI或读取凭据作为测试。Claude默认safe-mode/dontAsk，Codex只读sandbox，Pi禁用extension/MCP/skills/templates/theme/contextfiles自动发现并只读工具、offline启动；CLI安全标志不是OS沙箱。没有permission callback就明确unsupported。三vendor native身份均绑定product owner/canonical cwd；未知/foreign resume拒绝。Pi仅读取私有目录JSONL精确header/cwd，传确切文件路径，不让session-id缺失时新建冒充恢复。日志编码/脱敏失败锁存，后续帧或取消不能把不完整journal恢复成成功。日志/帧/总时限超限明确失败，不截断成成功。
Adapter.New不创建engine.sqlite，缺失Bindings在IO前拒绝；上层成功装配后移交Close所有权。原SQL实现已归platform/storage/bindings，stock程序显式选择；测试全部经opaque接口句柄跑真实子进程和重开。供应者Close失败保留原owner供显式重试。

## 业务域清单
| 文件 | 职责 |
|---|---|
| adapter.go | 显式程序/argv/env、owned I/O生命周期、私有日志与终态 |
| descriptor.go | 独立保守能力声明；不把CLI安全flag冒充等价权限profile，Claude真实append-system-prompt/max-turns，Codex/Pi明确拒绝，不声明native Inspector/provider headers/fast mode |
| mapping.go | 各vendor真实JSONL方言、tool配对、可恢复错误与结果证据 |
| sessions.go | 经共享BindingStore核对owner/cwd，Pi私有原生header与真实join后关闭所传组件 |
| private_other.go / private_windows.go | Pi文件的Unix权限/Windows DACL与SameFile检查；Windows USERPROFILE/AppData/TEMP显式绑定私有home，拒绝大小写profile override |
| adapter_test.go | 三vendor真实本地fake子进程、配对/错误/取消、日志和跨实例Pi恢复/foreign拒绝 |

Pi argv/恢复行为对照官方源码：`badlogic/pi-mono/packages/coding-agent/src/cli/args.ts`、`src/main.ts`、`src/core/session-manager.ts`（2026-10-16核对main）。只用fixture验证，不声称已运行任意安装版本的真实用户CLI。

Claude选项对照[官方CLI reference](https://code.claude.com/docs/en/cli-reference)（2026-10-17）：--append-system-prompt与--max-turns仅作为显式argv；原safe-mode/dontAsk保持，helper子进程验证真实argv和error_max_turns失败终态。无付费账户或已安装用户CLI验收。
