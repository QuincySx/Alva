# Go architecture checks
> 源码依赖防火墙和迁移约束的可执行验收。

## 地位

仅测试基础设施，不参与 Agent 或界面运行，不是另一个插件系统。

## 逻辑

解析非测试 Go 源码 imports，含 `desktop` 条件文件，验证直接及传递依赖。
`sdk` 保留 L0/L1/L2/L2.3/L2.5/L3 细分职责；`extension` 为 L4、`harness` 为 L5、
`apps` 为 L6/L7；`integration` 与 `platform` 是横向协议/原生支撑，不是绕过边界的容器。
公开快照扫描 `sdk` 中wire/bus/kernel ABI/kernel core/agent ABI/agent core/agent context和纯sandbox ABI，以及 `harness` 默认配方（M4 起，稳定/暂定分档留待内核收窄第二批）；路径迁移只更新其 import 身份，不建立旧路径别名。
Hub根产品契约亦进入快照，显式审阅独立存储端口/DTO变化；不将产品类型移入SDK。
Engine根契约/Alva工厂、显式选图media与owned process/document亦有公开快照，防止输入/回调和物理owner能力漂移。
Hub bootstrap与外部process adapter的显式配置入口亦保留公开快照；CLI配置目录与只读来源事实不能通过新增字段默默改变账号政策。
Developer动态目标源与显式job配置冻结端口也进入快照，避免连接消费政策在UI/装配之间漂移。
Gateway独立Local owner的generation/closed/stopping、一次Credentials与绑定停止公开面亦进入快照，防止多窗控制替换owner的语义漂移。
共享chat公开呈现也进入快照，显式审阅消息列表/输入区拆装，不导出可变列表内部状态。

## 约束

platform/sandbox/plugin是标准库/wazero叶子，不依赖AEP、SDK、host授权、存储或产品；由实际消费者接入协议。AEP和WASM peer新增公开面快照，既有SDK快照不变。

根目录静态闸不联网、不执行被扫描包、不依赖 go list 或外部 lint 安装；失败给出完整依赖路径。
`../extensibility/`是独立执行的消费者实验，显式调用SDK与原生临时存储；不参与静态扫描实现，不被生产依赖，不声称实验等于完整产品支持。
MyGo 只允许 `apps/direct/desktop`、`apps/hub/desktop`、`apps/ui/chat`、`apps/ui/modules` 与 `cmd`，不允许整个 apps/ui 或 apps 容器。共享 UI 不得直接/传递依赖 direct、Hub、harness、host 或 engine。
`apps/hub` 核心只看标准库与 `integration/engine` 根契约，不能回引自身 product/bootstrap/sqlite/server/desktop。
引擎根契约只依赖标准库；direct CLI/Desktop 不得传递进入 engine 或 Hub。
SDK、harness、integration、platform、extension 不得向上依赖 apps；L0 检查含未知新增 workspace 包。
platform/host/workspace 是独立叶子，不得依赖 extension/harness/storage 或 host 父包；模型引用/备份政策留 extension/files。
extension/utility/timer是已抽出的portable Tool叶子，直接/传递只可标准库与SDK，不能重新拉入父native Plugin、SQLite/host或第三方依赖；负例持续验证而非仅凭包名宣称轻量。
planning/interaction/utility 根包与纯 strictjson helper 只能直接/传递依赖标准库、SDK 和该 helper，禁止回引 native/、具体存储/host/外部依赖；含同名前缀子包与 helper 偷带 SQLite 的反例。
harness/durable 根包保持纯编排，只可用标准库、SDK、普通interaction和strictjson；native存储适配不得反向进入根包，传递反例持续验证。
platform/storage/blob根契约和adapter仅可标准库与根契约；根不依赖adapter，内存不继承SQL/SDK/执行/产品；含直接/传递反例。
显式 module import 即使没有被扫描的源节点也保留 workspace 身份，不能伪装成标准库依赖。
外部examples消费者限标准库/公开SDK，不能作为生产依赖；sessiontest/storagetest/blobtest及testing不可由运行时包直接或传递引入。公共测试入口也有显式公开快照。
旧目录和 import 路径不保留，布局回归拒绝旧名或 symlink 兼容桥。
纯容器目录即使没有 Go 源文件也必须保留含分形四节的地图文档。
手工窗口工具在 ../../tools/qa/native_window.py，不由本包、Go测试或CI隐式运行。

长会话全量读取闸用AST扫描 apps/、extension/、harness/、integration/、platform/host/ 的非测试 Go 文件：任何名为`Messages`且只有一个参数的方法调用、或第二个参数为字面量0的`Records(x, 0)`都报错，提示改用`BoundConversationTail`、`Page`或`LatestRecord`。按名字匹配是刻意的：复用旧名的新全量读取必须经审阅。确属有界或只作用于小规模私有会话的调用进带一句理由的显式白名单（键为路径#外层函数#方法名），白名单条目不再命中即判过期；当前白名单为空。
工具名/引擎名闸用AST扫描 apps/hub/product 与 apps/hub/desktop 的非测试 Go 文件：整串等于工具名表（各adapter分类的Alva内置工具、Codex item类型、Claude/Pi工具名）或引擎ID（alva/codex/claude/pi）的字符串字面量即报错，工具呈现应由adapter的ToolDisplay给出（ADR 0013 §3）。既有非工具用途（Alva专属能力闸、作者/引擎选择显示名、连接错误动词、产品自有hub_history/hub_team工具的action、Agent选择器文案）进带理由白名单，条目不再命中即判过期。
共享storage根契约仅可依赖SDK/纯blob词汇，普通消费者不能直接/传递进入具体session/memory/bindings/Hub SQLite；SQL错误不作为消费者协议。bootstrap只有storage.go选择stock provider，含同包assembly绕过的负例。存储根与blob公开面一并显式快照。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `boundaries_test.go` | import 图扫描、边界和回归反例 |
| `layout_test.go` | 分层目录、纯容器地图存在与旧平铺目录不得复生的回归 |
| `paths_test.go` | module 根定位、程序入口/桌面/验证角色路径；负例仍以独立字面路径验证精确边界 |
| `conversion_test.go` | 吸收 Magpie 的固定来源/许可/本地哈希与 RelayKit 已移除的离线闸 |
| `fractal_test.go` | 所有源码（含tagged/helper）三行头与目录分形文档强制检查 |
| `surface_test.go` / `testdata` | SDK、共享存储与Hub根契约公开面AST快照与扫描器反例，须显式审核更新 |
| storage_test.go | 统一存储插口、具体provider/SQL泄漏与显式配方位置的正反例 |
| reads_test.go | 禁止新增全量会话读取（单参`Messages`、`Records(x, 0)`）的AST闸、带理由白名单过期检查与扫描器正反例 |
| tool_names_test.go | apps/hub/product与apps/hub/desktop非测试源码禁止整串等于已知工具名（Alva/Codex/Claude/Pi）或引擎ID的字符串字面量；带一句理由、键为路径#字面量#次数的显式白名单（次数为该文件内此字面量的精确出现数，只能减少，多于或少于登记值都报错）（既有Alva能力闸、引擎显示名、产品自有工具action等），过期条目报错；扫描器正反例 |
