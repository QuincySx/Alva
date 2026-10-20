# Go SDK public surface
> 稳定SDK接口变化的显式审核快照。

## 地位
不编译、不运行；由architecture测试读取。

## 逻辑
包含导入身份、导出类型/字段/tag、泛型约束、接口封口、函数/receiver与常量组。

## 约束
仅显式 ALVA_UPDATE_GO_SURFACE=1 的机械更新；同一改动保留测试与职责审查。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| wire | llm-wire.txt | 消息/媒体/模型配置/流累积值契约 |
| bus | kernel-bus.txt | typed能力/独立有界通知，不包含另一套装配生命周期 |
| kernel ABI | kernel-abi.txt | 最小循环词汇、运行事实与执行上下文 |
| kernel core | kernel-core.txt | 可替换循环/执行配置与可选通用诊断观察契约 |
| agent ABI | agent-abi.txt | 可选Task/Scope/Spawn/phase/provider/analytics |
| agent core | agent-core.txt | Plugin/Registrar/Finalizer/Builder与快照 |
| sandbox ABI | sandbox-abi.txt | 纯宿主/guest能力数据 |
| storage ports | platform-storage.txt | 各层共用的组合插口、返回值与错误；不包含具体数据库 |
| immutable bytes | platform-storage-blob.txt | 两方法字节端口、retention与共享错误 |
| storage conformance | platform-storage-storagetest.txt | factory驱动的Repository/BindingStore公共测试入口，不是运行时能力 |
| byte conformance | platform-storage-blob-blobtest.txt | 不依赖具体adapter的不可变字节公共测试入口 |

2026-10-18：Alva module 提升到仓库根目录；六份快照仅同步 github.com/QuincySx/Alva 的 import 身份，保留原类型/字段/方法契约，不留旧 module alias。

2026-10-20：新增kernel core显式公开面快照，审阅DiagnosticObserver与纯观察值。原有L1/Plugin/Registrar契约没有为耐久等待新增字段或hook。
