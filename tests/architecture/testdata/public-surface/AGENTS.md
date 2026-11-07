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
| AEP protocol | integration-protocol-aep.txt | 实际Plugin协议、独占Process/Peer选择与物理owner契约，不进入SDK |
| Hub product | apps-hub.txt | 产品事实、next-turn Plan设置端口与既有六方法执行依赖 |
| Developer jobs | apps-developer.txt | 静态/动态目标端口、显式job配置冻结与可取消owner；不包含账号数据 |
| Local Gateway | apps-gateway.txt | 独立监听owner的generation/closed/stopping、显式一次Credentials与绑定停止；状态快照不含Bearer |
| Hub shared services | apps-hub-product.txt | 共享草稿/引用/命令/Skills准备与实际产品准入，不是SDK内核契约 |
| Hub assembly | apps-hub-bootstrap.txt | 程序显式选择存储、provider和外部CLI配置来源的装配入口，不下沉产品策略到SDK |
| Shared native chat | apps-ui-chat.txt | 共享有界消息/输入、可选产品行呈现与行距；零值保留既有布局，不导出可变列表状态 |
| External CLI adapter | integration-engine-process.txt | 显式程序/profile配置、只读账号来源事实与实际执行生命周期，不含账号数据 |
| WASM execution | platform-sandbox-plugin.txt | 独立WASIp1模块配置和stdio执行owner，不含协议或存储 |
| Engine input/interaction contract | integration-engine.txt / integration-engine-alva.txt | stdlib engine事件、typed文件/Skill与实际factory能力声明，不导入产品政策 |
| Native selected media | extension-media.txt | 原件/派生/选择/单请求receipt与inputs-only可选Plugin，不启用全套媒体Tool |
| Physical process/document owner | platform-system-process.txt / platform-system-document.txt | 自有PID内存观察、固定PDF处理器及取消/真实join；不冒充OS硬内存沙箱 |

2026-10-18：Alva module 提升到仓库根目录；六份快照仅同步 github.com/QuincySx/Alva 的 import 身份，保留原类型/字段/方法契约，不留旧 module alias。

2026-10-20：新增kernel core显式公开面快照，审阅DiagnosticObserver与纯观察值。原有L1/Plugin/Registrar契约没有为耐久等待新增字段或hook。

2026-10-26：Codex环境/输出两个私有helper加入process快照的import记录；审阅确认本次无导出类型/字段/签名变化，环境继承政策与真实来源验证分别记录在process地图与根MIGRATION。
