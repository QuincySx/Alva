# 第三轮实际验证与证据

审查HEAD：`f9d0e5744a16a546ff2577f6f583a2ab520afb3a`，diff基线`134b8b3`。生产代码未修改；临时复现测试最终随本目录交接，不作为已实施修复。

## 已完成验证

| 范围 | 实际结果 | 日志 |
|---|---|---|
| kernel-core / agent-core / agent-context / app-core，lib + tests | **735 passed / 0 failed / 4 ignored**，28个测试二进制，命令exit0 | [core-tests](./alva-sep11-core-tests.txt) |
| Hub前端Vitest | **70 passed / 0 failed**，6个文件 | [web-tests](./alva-sep11-web-tests.txt) |
| Hub前端TypeScript + Vite生产构建 | **PASS**；有chunk尺寸warning | [web-build](./alva-sep11-web-build.txt) |
| Hub Playwright视觉/交互 | **7 passed / 5 failed**；均为截图断言，未更新expected | [visual-tests](./alva-sep11-visual-tests.txt) |
| public-surface快照 | **3 crates PASS** | [surface](./alva-sep11-surface.txt) |

```sh
cargo test -p alva-kernel-core -p alva-agent-core -p alva-agent-context -p alva-app-core --lib --tests --offline --locked
# 在 crates/alva-hub-desktop/web 中：
npm test
npm run build
npm run test:visual -- --update-snapshots=none
# 仓库根：
./scripts/public-surface.sh --check
```

4个ignored来自现有真实模型eval测试，未强行运行。核心suite启动时未纳入后建的review_round3_probes目标；735通过不包含专项复现，也不能消除静态发现。

## 专项复现

本次实际编译运行的源码：[review_round3_baseline_probes.rs](./review_round3_baseline_probes.rs)。**0 passed / 4 failed**，exit101，确认以下四个缺口：

- 动态ToolExecutionContext能读取已配置store cell。
- clear后新订阅收到下一条实时事件。
- 第一个append在异步监听回调阻塞时也能返回。
- 确认回调已进入后实际abort追加任务，既有实时订阅不漏已提交事件。

另提供修复验收种子 [review_round3_probes.rs](./review_round3_probes.rs)：取消用例增加 append 返回后的调用者存活 gate，避免正确的非阻塞实现返回后无法再取消外层任务。此调整版尚未编译运行；上述4失败对应原始 baseline 源码。复制种子到 `crates/alva-agent-core/tests/review_round3_probes.rs` 后可运行：

```sh
cargo test -p alva-kernel-core -p alva-agent-core -p alva-agent-context -p alva-app-core --test review_round3_probes --offline --locked
```

实际运行：[编译日志](./alva-sep11-probes-build.txt)、[四项失败日志](./alva-sep11-probes-final.txt)。先以核心suite相同 feature graph 执行 `cargo test -p alva-kernel-core -p alva-agent-core -p alva-agent-context -p alva-app-core --lib --tests --no-run --offline --locked`，再直接执行生成的 `target/debug/deps/review_round3_probes-31621e6519bfb155`。100ms timeout用于已建立状态/gate之后的完成或漏消息检查。临时测试已移出生产源码目录，保留于本证据包。

## Hub 原生实测

使用 `./target/debug/alva-hub-desktop` 打开真实Tauri窗口，前端来自当前Vite `127.0.0.1:1421`。已有二进制mtime为2026-09-10 05:24:54；本次没有把它当作最新HEAD的Rust构建。

- [自动化创建后](./native/native-auto-created.png)显示1个且启用；[切页返回](./native/native-auto-returned.png)变为0。提示词为无副作用review no-op，仅局部state，未安排真实计划任务。
- [设置选择Plan](./native/native-plan-selected.png)后，[新会话](./native/native-plan-new-session.png)仍为允许写入工作区；源码证实局部state未接入策略。
- [插件页](./native/native-plugins.png)显示硬编码“已安装”；注册/安装处理器的无持久化行为为静态审查，未进行网络安装。
- 用户本人随后在该窗口发出“你好”并收到真实模型回答。[用户会话截图](./native/user-hello.png)和[完整界面树](./native/user-hello.txt)显示执行图挤占聊天、Markdown原样、Middleware巨列表、重复Inspector内容与技术ID。该记录是UI重整样本，不是根agent额外发起的模型任务。

原生Tauri会话保持打开，用户可继续使用；不清理用户“你好”历史。UI问题中纯前端行为来自当前源码；递归持久化和原子切换仍需新卡上的真实端到端验收。

## 视觉差异

`visual/` 保存5个失败的expected/actual/diff和错误上下文。人工查看chat对照，差异集中顶部状态“操作已完成”→“已完成”及相邻控件位移（411/509像素），不是五个独立功能bug。
Inspector测试的截图失败发生在Back断言前，因此本轮不能声称整个Back流程通过。按用户新要求重整UI后由R23重新审阅基线，不直接放宽阈值。

## 未完成／未运行

- Hub Rust `cargo test -p alva-hub-core -p alva-hub-desktop --lib --tests --offline --locked` 已启动，长时间等待共享构建目录后编译依赖；为优先收尾专项复现，本轮主动中止，**无测试结论**。[日志](./alva-sep11-hub-tests.txt)
- 最新桌面/server `cargo build -p alva-hub-desktop -p alva-hub-server --offline --locked` 等待构建锁时主动中止，**无最新构建成功结论**。[日志](./alva-sep11-hub-build.txt)
- 未运行全workspace测试、全部engine实际会话、完整dependency/wasm闸门、clippy；不宣称release-ready或全量通过。
