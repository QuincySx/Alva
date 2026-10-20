# 横向支撑 · Storage contracts and adapters
> 同一套存储契约贯穿消费者与实现，由最外层程序装配选择组件。

## 地位
根 package 仅有契约、共享DTO/错误和SDK类型；不打开数据库、文件、网络或选择默认后端。
SDK不依赖本目录。会话事实、Hub产品事实、memory与不可变字节各有独立职责，不合成God Store。
## 逻辑
SessionReader/Records/Conversations/References/Children等小端口按消费者组合；Repository只是应用完整profile。
会话、分页、引用、因果关系的返回类型始终是同一共享契约，不能在下一层退回具体Repository/Session。
MemoryStore.Bind返回同一SDK memory.Backend；BindingStore保存外部runtime的owner/workspace create-or-verify事实。

契约的语义与方法签名一起约束替换组件：

| 插口 | 所有实现必须保持的语义 |
|---|---|
| Session | SDK原子AppendRecords/Flush与实际authority；AppendExtensionSnapshot按exact namespace/type最新revision比较，附加records全有或全无 |
| Records/RecordLookup | Page/Search按记录sequence向前、after exclusive；highWater=0取得当前快照，后续沿返回highWater读取；LatestRecord的false只表示已授权会话内该stream尚无记录 |
| ConversationTail | 固定session/highWater；Metadata返回独立副本、从新到旧；Read取最新count条并按对话时序返回，受maxBytes限制；不选择模型窗口或拼省略提示 |
| Artifacts/References | 全程owner+session绑定；URI/hash不是权限。metadata页不取正文；bounded读/完整读校验对应hash，返回独立buffer；引用继续使用SDK ReferenceBackend |
| Children | 实际父run/call的已提交intent为先决条件，child与typed因果link原子创建；关系查询不自动授予父/子正文权限 |
| Runs | 执行前独占claim与旧未知intent结算；release幂等，Recover不重放工具或替实际owner取消执行 |
| BindingStore | 已分namespace的native ID，create-or-verify；重复相同值幂等、owner/workspace冲突零改写；不存在与provider失败分别报告 |

实现位于子目录；上层程序可选择现有Local配方或自己的组件，普通Plugin/harness/UI不实例化后端。
## 约束
原子append/CAS、独占执行claim、真实authority、会话归属、固定高水位及有界读写语义必须保持。
消费者不得downcast实现、识别SQL错误、失败后换默认库或按字符串ID伪造权威。
Close由装配owner在真实执行join后调用；成功幂等，失败保留组件供显式清理重试。普通工具/Inspector不获得Close。
RecordRetention与artifact retention分别如实声明；临时字节不冒充持久记录，普通网盘不冒充日志事务。
外部不可变上传先于metadata提交；不确定结果不自动重试/删除，不自动获得宿主执行权限。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Shared ports | contracts.go | 会话/记录/因果关系/引用/记忆/身份绑定的组合契约与共享错误 |
| Shared values | values.go | 与实现无关的DTO、游标、引用与检查结果 |
| Immutable bytes | blob/ | Put/bounded Read契约及显式目录/易失内存适配 |
| Native sessions | session/ | SQLite typed facts/catalog与授权/hash，不拥有模型历史选择政策 |
| Persistent memory | memory/ | 可选namespaced SQLite FTS/vector实现，Bind交回SDK接口 |
| Runtime identity | bindings/ | 已有外部引擎身份SQL实现；不拥有进程或装配选择 |
| Pure redaction | redaction/ | journal共用纯数据脱敏，不带存储实现依赖 |
| SQLite host | sqlite/ | 私有数据库配置及进程锁；文件系统检查下沉system/privatefs |
