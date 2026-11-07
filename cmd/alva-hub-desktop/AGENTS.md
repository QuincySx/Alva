# L7 · Go native Hub desktop entry
> 独立产品入口，真实装配产品 SQLite 与原生会话 SQLite，再经 Alva EngineRuntime 运行完整 harness。
## 地位
位于 Hub 产品层；不是直接 alva-desktop 的替代桥或历史兼容层。
## 逻辑
普通CLI必须显式 --data-dir，默认本地 demo；--live 必须显式 endpoint/model，可从可取消pipe/regular文件stdin提供≤8KiB key。flags之后委托apps/hub/bootstrap.Open唯一配方，入口不复制引擎/插件装配。
可选 --codex-config-dir/--claude-config-dir/--pi-config-dir必须配对应--*-command及外部host批准，指定absolute已存在的完整CLI配置目录；仅CLI读取其内容。已批准Codex app-server按Cindy继承本机环境/HOME、单独设置CODEX_HOME；Claude及显式Codex exec仍保留私有HOME。canonical目录SHA256绑定实际产品账户route，换目录后须明确选择新route；不宣称只复用登录，不编辑CLI配置文件，rules/hooks仍由CLI负责；app-server本次sandbox/approval由下述显式参数决定。
显式--codex-command默认app-server双向stdio；--codex-transport exec明确选择旧打印协议，不按失败回退/重放。app-server默认--codex-sandbox workspace-write与--codex-approval-policy on-request，可明确选read-only、untrusted/never；这些原生策略不冒充Alva Ask/Auto/Full。写根仅saved canonical cwd，关闭network与默认tmp写根；策略flag只接受对应显式Codex/app-server，非法组合在storage I/O前拒绝。不增加登录、凭据读取或账户发现。
Pi默认--pi-transport rpc经原生双向协议与产品审批处理；显式exec保留只读print，非法transport或没有对应command在存储I/O前拒绝；不按失败切换协议。
正常build仅当无参数且实际executable位于*.app/Contents/MacOS时，读取同bundle唯一固定Resources/alva-launch.json；显式终端flags完全替代profile，普通CLI不搜索安装、HOME或旧配置。严格version=1 JSON包含arguments数组和可选external_environment；后者只接受已选择且host-approved引擎的PATH绝对目录列表。profile不可请求stdin秘密，不包含API key；实际CLI配置只交给所选CLI。普通demo仍明确是离线演示，已存Connections目标由下一Turn按原有产品服务解析为真实Alva route。QA smoke不读取安装profile。
安装manifest示例（路径由安装者明确填写，不作机器扫描）：
```json
{"version":1,"arguments":["--data-dir","/absolute/private/alva","--demo","--codex-command","/absolute/bin/codex","--codex-config-dir","/absolute/codex-config","--pi-command","/absolute/bin/pi","--pi-config-dir","/absolute/pi-agent-config","--pi-transport","rpc","--external-host-approved"],"external_environment":{"pi":{"PATH":"/absolute/node/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"}}}
```
可选 --debug-listen 数字loopback与 --startup-secrets-stdin 精确JSON独立token；默认off/只读，controls/execution分别授权。prepare只构造冷owner，进入UI前显式监听；诊断先join再关产品，诊断独立shutdown不Stop Agent。
## 约束
无 WebView/React/npm。窗口关闭后由 owner 先关automation admission，再有界关闭执行/external adapter绑定库及独立存储。外部引擎必须显式 executable+host确认；managed MCP按会话enabled/approved snapshot和同一Guard按需启动，模型只拿引用。项目Skill只从明确启用的saved cwd受管根加载metadata；AEP检查pin与独立host批准下一Turn生效，进程/代码snapshot owned cleanup。四provider走canonical Build，持久route必须匹配实际model/provider/account/billing。--image-capable仅operator明确当前live route支持图片，未知默认拒绝选图；媒体工具/decoder另外独立opt-in，原件不自动投递。已有typed递归Inspector、真实独立Worker FIFO/生命周期及opt-in SDK child、引用Team handoff、paused-first自动化和显式静音派生视频自绘播放与有界PCM音频voice；实机窗口可观察验收尚未通过。
受管工作树额外需--git-executable+--git-host-approved；新Worker首Turn前显式隔离，SHA/path diff与二次确认literal采纳。不执行repo filters、不自动commit/merge/reset，未知不重放/回滚；不是OS原子hash-CAS/敌对sameUID隔离。HTTP已复用同product服务，多轮rebase/原件恢复UI为原Rust尚未实现的后续增强。

程序入口负责选择实际存储组件：direct显式打开后传Config.Storage；Hub明确传LocalStores/LocalMemory/LocalBindings配方。下层消费者只看共享插口，错误不触发默认后端fallback。

## 业务域清单

| 文件 | 职责 |
|---|---|
| main.go | 可取消的冷启动/显式配置、唯一bootstrap装配与资源关闭；进入窗口前还原启动signal handler |
| production_desktop.go | 常规构建显式flags或无参数已安装profile的启动入口与普通窗口runner |
| launch_profile_desktop.go / launch_profile_desktop_test.go | 固定bundle Resources严格manifest、明确外部PATH、三engine冷注册与非法来源零存储回归 |
| automation_e2e_desktop.go | 仅desktop&&e2e导入MyGo官方automation插件；只有显式MYGO_AUTOMATION私有Unix socket才监听，普通发布无入口 |
| smoke_desktop.go | 仅desktop&&smoke临时隔离目录/本地demo和own-window readiness诊断 |
| Startup regressions | `main_test.go` | 实际open pipe无EOF取消/不关闭借用stdin、拒绝超限key且零模型/窗口/Store、启动context结束后真正Send仍可执行；Codex app-server默认/显式exec冷装配与独立native policy映射，不启动用户CLI |
