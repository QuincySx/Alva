# L7 · Shared native Hub assembly
> Desktop与HTTP使用同一实际装配；不依赖MyGo，不创建第二runtime或facts store。
## 地位
L7产品配方：组合apps/hub/product、共享存储profile、integration/engine/alva、外部engine和可选插件。
## 逻辑
Open仅接显式存储工厂/目录/provider/approved executable snapshots，验证但不联网、不起CLI/MCP/任务。每次Alva装配绑定实际Hub owner/nativeID、同一审批Guard/ToolLockRegistry、native refs、History/Team/Skills/MCP；Close先Team pumps与owner执行再adapter再store。真实demo echo实例来自Builder.DeclaredTools，不用同名假对象或放宽unknown。
Native SubAgent必须显式opt-in，其Plugin也进入exact attempt资源registry；递归child的setup失败owner和physical cleanup持续保留，绑定actual parent identity ceiling及子会话独立facts/审批，不冒充Hub Worker。AEP仅已检查pin+独立host批准后下一TurnStage/Open；未知native风险先耐久commit，失败构建仍绑定已知native日志供Inspector；factory/SDK终态均owned Close，cleanup强失败不降为取消。
持久memory默认关闭，仅显式opt-in后lazy调用程序选择的MemoryFactory；opaque namespace由saved canonical workspace path派生，模型不能选择其他project namespace；无远程embedding/自动scan/extract，不自动委派给child。Hooks/diagnostics同样显式disabled edits+独立host批准，下一Turn装配实际Plugin；Hook pre-input仍重新HITL，diagnostic exit0不冒充零问题。
媒体默认off，启用仅下一Alva Turn注册actual media Tool；可选decoder只消费明确两exe+独立host批准的snapshot，不发现程序。模型视觉支持仅由operator Config.ImageCapable声明并绑定exact provider/model，未知拒绝select，demo不能自称支持；原始上传不自动投递，实际JPEG选择只进入一个请求副本。
Browser只消费显式批准snapshot并注册actual7个Tools，同Guard/先耐久risk；每run fresh private profile，不读用户浏览器、登录、keychain或已有配置。构造/保存不启动，正常结束/取消/early失败都由exact attempt资源registry收尾，清理未完成不关facts。未委派给native child。
## 约束
无全局配置/凭据读取。持久route必须等于实际构建的model/provider/account/billing。Close超时不能关掉活跃执行下的事实存储，可显式重试。UI与HTTP仅消费同一Service，不复制recipe。
Open只构造AutomationCoordinator不Start；native UI/HTTP入口显式Start其本地timer，Application.Close在Teams/Runtime之前关admission owner，保留已认领普通Turn的runtime所有权。
独立Shell Sessions资源owner显式延长actual attempt的Hub workspace lease及SDK global lease到reap；每轮Lease只拥有generation reservation，不把observer退出当Stop。正常Turn可跨轮读/写；fresh Guard/reporter只在idle重绑，实际exe/env/cwd/authority不可变；Application.Close在Runtime之后、facts之前取消/join所有Shell owner，strong journal error和physical completion分开。Hooks同样注册actual attempt资源，独立Notify/finalizer未退出不得关facts，run-end join超时保留强失败且owner Close不重放final hooks。MCP同样注册actual attempt资源，不仅靠OnRunEnd；审批/工具回调、worker/process和native journal均完成后才允许关facts。Native resource registry保留真实未完成physical cleanup的factory/runner，阻止同product新Turn，并让Application.Close先重试精确owned资源（包含SubAgent）再关facts；journal强错误和物理cleanup完成是两个事实，不能互相覆盖。
受管Worker worktree仅显式GitExecutable+GitHostAccessApproved构造（不执行），首Turn前用户独立批准才创建新private目标；实际session.Workspace与native tool根一致，已有native历史不得retarget。Runtime.Close先取消/join双方workspace租约的文件操作，再Close Git manager；未知mutation不重放/回滚、不reset/delete旧worktree。Git cwd是canonical+前后identity检查，Darwin未有进程级fchdir，不声称敌对sameUID下原子cwd或OS隔离。
OpenStores为必传插口，OpenMemory/OpenBindings只在相应组件启用时需要；不存在或失败不选默认后端。stock Local*配方集中storage.go，普通assembly/tools只看同一共享契约。ProductStorage显式声明原生路由resolver，防止换实现后审批能力丢失。工厂部分资源、adapter未接管binding和Close失败均保留真实owner；构造清理未完成时返回Application及error供Close重试。

## 业务域清单
| 文件 | 职责 |
|---|---|
| assembly.go | 显式Config、Open/Application和真实执行资源owned shutdown；live仅构造cold Gateway，Close先取消/join其真实handler |
| tools.go | 六类可选Plugin真实装配（planning/interaction/utility 显式导入 native 适配，普通 Tool 根包不再绑定原生产品）、same-session问题broker、actual tool信任与Shell跨轮Session owner/attempt Lease与Web物理收尾 |
| assembly_test.go | no-start、真实demo跨重启native resume、route绑定与未批准零副作用回归 |
| options_test.go | actual registered factory→local HTTP system role；opt-in请求轮数/系统prompt实际传给harness，不只改Descriptor |
| subagent_test.go | 真实四次local model/root-child HITL、typed关系、引用隔离与冷Inspector闭环 |
| mcp_test.go | 明确本地真实MCP子进程、lazy启动、actual attempt资源注册及SDK/应用物理收尾 |
| aep_test.go | 实际本地AEP进程下一Turn启动、pin拒绝、握手失败真实日志/终态和snapshot清理 |
| memory.go / memory_test.go | lazy workspace-bound持久memory配方、真实HITL/跨重启/引用投递/不扫描/不同workspace隔离 |
| commands_test.go | 实际本地命令/SDK/HITL、pre-input新权限、诊断来源/exit/reap证据和prior Activity |
| resources.go / resources_test.go | exact per-attempt资源注册、early-observer失败/physical cleanup重试、strong错误与facts关闭顺序，以及actual Hooks.Notify取消/Close超时重试 |
| media_test.go | 真实HTTP/SDK/SQLite原件保留、JPEG派生、未知route拒绝、单请求图、重启预览和无历史重放 |
| browser_test.go | 显式真实Chrome/local HTTP、actual SDK/HITL、prior risk、文本/截图refs与实际reap/profile removal |
| worktree_test.go | 显式真实Git+HTTP/SDK write_file/HITL Worker隔离、显式原字节采纳、持久冷证据与重开不重放 |
| storage.go | 共享产品profile与显式Stores/Memory/Bindings工厂；仅Local配方选择具体provider |
| storage_test.go | opaque组件贯穿实际Turn、缺失/失败零fallback、部分资源与failed-close所有权 |
