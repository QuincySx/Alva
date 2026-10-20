# L7 · Go native Hub desktop entry
> 独立产品入口，真实装配产品 SQLite 与原生会话 SQLite，再经 Alva EngineRuntime 运行完整 harness。
## 地位
位于 Hub 产品层；不是直接 alva-desktop 的替代桥或历史兼容层。
## 逻辑
显式 --data-dir，默认本地 demo；--live 必须显式 endpoint/model，可从可取消pipe/regular文件stdin提供≤8KiB key。flags之后委托apps/hub/bootstrap.Open唯一配方，入口不复制引擎/插件装配。
可选 --debug-listen 数字loopback与 --startup-secrets-stdin 精确JSON独立token；默认off/只读，controls/execution分别授权。prepare只构造冷owner，进入UI前显式监听；诊断先join再关产品，诊断独立shutdown不Stop Agent。
## 约束
无 WebView/React/npm。窗口关闭后由 owner 先关automation admission，再有界关闭执行/external adapter绑定库及独立存储。外部引擎必须显式 executable+host确认；managed MCP按会话enabled/approved snapshot和同一Guard按需启动，模型只拿引用。项目Skill只从明确启用的saved cwd受管根加载metadata；AEP检查pin与独立host批准下一Turn生效，进程/代码snapshot owned cleanup。四provider走canonical Build，持久route必须匹配实际model/provider/account/billing。--image-capable仅operator明确当前live route支持图片，未知默认拒绝选图；媒体工具/decoder另外独立opt-in，原件不自动投递。已有typed递归Inspector、真实独立Worker FIFO/生命周期及opt-in SDK child、引用Team handoff、paused-first自动化和显式静音派生视频自绘播放与有界PCM音频voice；实机窗口可观察验收尚未通过。
受管工作树额外需--git-executable+--git-host-approved；新Worker首Turn前显式隔离，SHA/path diff与二次确认literal采纳。不执行repo filters、不自动commit/merge/reset，未知不重放/回滚；不是OS原子hash-CAS/敌对sameUID隔离。HTTP已复用同product服务，多轮rebase/原件恢复UI为原Rust尚未实现的后续增强。

程序入口负责选择实际存储组件：direct显式打开后传Config.Storage；Hub明确传LocalStores/LocalMemory/LocalBindings配方。下层消费者只看共享插口，错误不触发默认后端fallback。

## 业务域清单

| 文件 | 职责 |
|---|---|
| main.go | 可取消的冷启动/显式配置、唯一bootstrap装配与资源关闭；进入窗口前还原启动signal handler |
| production_desktop.go | 常规构建必须显式data-dir的生产入口默认值 |
| smoke_desktop.go | 仅desktop&&smoke临时隔离目录/本地demo和own-window readiness诊断 |
| Startup regressions | `main_test.go` | 实际open pipe无EOF取消/不关闭借用stdin、拒绝超限key且零模型/窗口/Store、启动context结束后真正Send仍可执行 |
