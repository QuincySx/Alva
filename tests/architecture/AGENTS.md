# Go architecture checks
> 源码依赖防火墙和迁移约束的可执行验收。

## 地位

仅测试基础设施，不参与 Agent 或界面运行，不是另一个插件系统。

## 逻辑

解析非测试 Go 源码 imports，含 `desktop` 条件文件，验证直接及传递依赖。
`sdk` 保留 L0/L1/L2/L2.3/L2.5/L3 细分职责；`extension` 为 L4、`harness` 为 L5、
`apps` 为 L6/L7；`integration` 与 `platform` 是横向协议/原生支撑，不是绕过边界的容器。
公开快照扫描 `sdk` 中wire/bus/kernel ABI/kernel core/agent ABI/agent core和纯sandbox ABI；路径迁移只更新其 import 身份，不建立旧路径别名。

## 约束

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

共享storage根契约仅可依赖SDK/纯blob词汇，普通消费者不能直接/传递进入具体session/memory/bindings/Hub SQLite；SQL错误不作为消费者协议。bootstrap只有storage.go选择stock provider，含同包assembly绕过的负例。存储根与blob公开面一并显式快照。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `boundaries_test.go` | import 图扫描、边界和回归反例 |
| `layout_test.go` | 分层目录、纯容器地图存在与旧平铺目录不得复生的回归 |
| `paths_test.go` | module 根定位、程序入口/桌面/验证角色路径；负例仍以独立字面路径验证精确边界 |
| `conversion_test.go` | 吸收 Magpie 的固定来源/许可/本地哈希与 RelayKit 已移除的离线闸 |
| `fractal_test.go` | 所有源码（含tagged/helper）三行头与目录分形文档强制检查 |
| `surface_test.go` / `testdata` | SDK公开面AST快照与扫描器反例，须显式审核更新 |
| storage_test.go | 统一存储插口、具体provider/SQL泄漏与显式配方位置的正反例 |
