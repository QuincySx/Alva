# Go architecture checks
> 源码依赖防火墙和迁移约束的可执行验收。

## 地位

仅测试基础设施，不参与 Agent 或界面运行，不是另一个插件系统。

## 逻辑

解析非测试 Go 源码 imports，含 `desktop` 条件文件，验证直接及传递依赖。
`sdk` 保留 L0/L1/L2/L2.3/L2.5/L3 细分职责；`extension` 为 L4、`harness` 为 L5、
`apps` 为 L6/L7；`integration` 与 `platform` 是横向协议/原生支撑，不是绕过边界的容器。
公开快照扫描 `sdk` 中原有五个契约包和纯sandbox ABI；路径迁移只更新其 import 身份，不建立旧路径别名。

## 约束

根目录静态闸不联网、不执行被扫描包、不依赖 go list 或外部 lint 安装；失败给出完整依赖路径。
`extensibility/`是独立执行的消费者实验，显式调用SDK与原生临时存储；不参与静态扫描实现，不被生产依赖，不声称实验等于完整产品支持。
MyGo 只允许 `apps/desktop`、`apps/hub/desktop`、`apps/cmd`，不允许整个 apps 容器。
`apps/hub` 核心只看标准库与 `integration/engine` 根契约，不能回引自身 product/bootstrap/sqlite/server/desktop。
引擎根契约只依赖标准库；direct CLI/Desktop 不得传递进入 engine 或 Hub。
SDK、harness、integration、platform、extension 不得向上依赖 apps；L0 检查含未知新增 workspace 包。
extension/utility/timer是已抽出的portable Tool叶子，直接/传递只可标准库与SDK，不能重新拉入父native Plugin、SQLite/host或第三方依赖；负例持续验证而非仅凭包名宣称轻量。
显式 module import 即使没有被扫描的源节点也保留 workspace 身份，不能伪装成标准库依赖。
旧目录和 import 路径不保留，布局回归拒绝旧名或 symlink 兼容桥。
纯容器目录即使没有 Go 源文件也必须保留含分形四节的地图文档。
manual native_window_qa.py不由Go测试或CI隐式运行；仅显式--run会编译/打开自己的临时demo窗口、验证实际owner回调，并分别记录current Build与compositor/AX未验收，finally回收自建进程。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `boundaries_test.go` | import 图扫描、边界和回归反例 |
| `layout_test.go` | 分层目录、纯容器地图存在与旧平铺目录不得复生的回归 |
| `conversion_test.go` | 吸收 Magpie 的固定来源/许可/本地哈希与 RelayKit 已移除的离线闸 |
| `fractal_test.go` | 所有源码（含tagged/helper）三行头与目录分形文档强制检查 |
| `surface_test.go` / `testdata` | SDK公开面AST快照与扫描器反例，须显式审核更新 |
| `native_window_qa.py` | 仅显式--run的macOS临时own-app/demo/私有export/独立HTTP窗口线程探针；current Build前拒绝旧PNG，--launchservices仅给自建bundle ad-hoc签名/open，真实回调/渲染/compositor/AX分开记录，正常Close或严格own PID回收 |
| `extensibility/` | 第三方消费者式的上下文替换、多Agent调度、双向transport/替换loop与跨进程业务阶段恢复实验；无生产契约新增 |
