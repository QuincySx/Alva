# 迭代日志
- iter1 | 完成: web 测试基建(vitest+RTL,11测试)+场景8 fallback 卡片三控件集成测试+纯函数测试 | 验证: npm test 11/11, tsc clean, commit 3736659 | 清理: 无 Rust 残留产生 | 下一步: iter2
- iter2 | 完成: 打包构建验证 web build + cargo release build(2m28s) + ci-check-deps 46 OK | 验证: 全过 | 清理: cargo clean 2.7GiB | 下一步: iter3
- iter3 | 完成: 真实机器 readiness 冒烟测试(SystemProbe vs 真实 claude/codex/alva, 独立 PATH 交叉校验) | 验证: 130/130 lib tests, commit 下一行 | 清理: cargo clean | 下一步: iter4
- iter4 | 完成: 打包真实 .app(tauri build, 修 CWD 与版本对齐, 启动冒烟: 存活11s+db初始化+干净退出) | 验证: bundle 启动通过 | 清理: cargo clean | 下一步: iter5 跨引擎 memory
- iter5 | 完成: 跨引擎 memory(schema14 hub_memory 表+HandoffPackage.session_memory+redact 过滤+drawer Memory 面板) | 验证: 132 lib + 12 web, tsc clean | 清理: cargo clean | 下一步: iter6 跨引擎 skills
- iter6 | 完成: 跨引擎 skills(扫描 .alva/skills/*/SKILL.md 进 HandoffPackage.available_skills+redact+handoff 视图) | 验证: 133 lib + 12 web, tsc clean, ci-deps exit0 | 清理: cargo clean | 下一步: iter7 engine model 选择器/提供商状态
- iter7 | 完成: 全量回归(hub-cli 24)+真实 .app 重建+真实库 13→14 迁移冒烟(hub_memory 创建) | 验证: 全绿+端到端迁移通过 | 清理: cargo clean | 下一步: 待用户决定是否继续 iter8+(候选: model 选择器/提供商标态, MCP 跨引擎)
- iter8 | 完成: 真实引擎打包运行(codex fresh+resume+alva+alva→codex handoff); 修2个真bug: codex resume -C/-s 参数+alva providers 解析 | 验证: hub-cli 24+codex-adapter 9, ci-deps 46 OK, 真实运行全通 | 清理: cargo clean | 下一步: claude 真实跑+desktop GUI 会话, fallback 真实触发观察
- iter8b | 完成: claude light 真实跑通(14.), 三引擎全部真实验证+alva→codex handoff | 验证: ACCEPTANCE 场景1 CLI 路径闭环 | 清理: 无新残留 | 下一步: desktop GUI 会话, 真实 fallback 触发(场景3)需人工/注入
- iter9 | 完成: 跨引擎 MCP(扫描 .mcp.json/mcp.json 两形状进 HandoffPackage.available_mcp, env/headers 绝不入快照) | 验证: 134 lib + 12 web, tsc clean | 清理: cargo clean | 下一步: iter10 model 选择器/提供商状态 UX
- iter10 | 完成: fallback chain model/provider 可编辑(纯前端, 抓到一个 React key 含可编辑字段致键入丢失的真 bug) | 验证: 13 web, tsc clean | 清理: 无 Rust 残留 | 下一步: iter11
- iter11 | 完成: 验证+固化 Hub Alva 会话长会话 middleware(compaction/checkpoint/tool_timeout 在栈中) | 验证: adapter-alva 12 tests | 清理: cargo clean | 下一步: iter12
- iter12 | 完成: details drawer handoff 列表显示跨引擎信封摘要(memory/skills/MCP 计数) | 验证: 13 web, tsc | 清理: 无 | 下一步: iter13
- iter13 | 完成: ACCEPTANCE Beyond-V1 补全(MCP/可编辑 policy/长会话 middleware)+最终质量门 | 验证: ci-deps 46 OK, 134+24+12+9 lib, 13 web, tsc | 清理: cargo clean | 下一步: 待续
- iter14 | 完成: 真实 fallback 目标观察(real_codex_runs + real_codex_receives_a_fallback, gate 模式) | 验证: 136 lib tests | 清理: cargo clean | 下一步: iter15
- iter15 | 完成: composer 显示当前 engine 生效 model/provider(优先 policy 编辑值) | 验证: 13 web, tsc | 清理: 无 | 下一步: iter16
- iter16 | 完成: 最终打包重建+.app 启动冒烟(schema14 无回归, 干净退出) | 验证: ci-deps 46 OK, 136 lib + 13 web 全绿 | 清理: cargo clean | 下一步: 恢复点已就绪
- iter17 | 完成: worker 队列→真实引擎链路测试(lead→create_team_run→enqueue→claim→real codex run→finish→turn 持久化) | 验证: 137 lib tests | 清理: cargo clean | 下一步: iter18
- iter18 | 完成: grouped approval 卡片前端测试(全员可见+exactly 显示的 request ids) | 验证: 14 web, tsc | 清理: 无 | 下一步: iter19
- iter19 | 完成: readiness/repair 面板前端测试(徽章+版本行+remedy+one-click repair 分发) | 验证: 15 web, tsc | 清理: 无 | 下一步: iter20
- iter20 | 完成: Team 面板前端测试(worker 卡片+lifecycle 徽章+isolated+worktree 提示+Interrupt 武装+Hibernate 分发) | 验证: 16 web, tsc | 清理: 无 | 下一步: iter21
- iter21 | 完成: worktree adoption 前端流程(Review→preview 渲染冲突/证据→Adopt 分发 run+worker ids) | 验证: 16 web, tsc | 清理: 无 | 下一步: iter22
- iter22 | 完成: worker 派发表单测试(scope+claims 解析+stale 选择回退) | 验证: 16 web, tsc | 清理: 无 | 下一步: iter23
- iter23 | 完成: 最终全量验证+打包冒烟(schema14 无回归)+恢复点加固 | 验证: 137+24+9+12 lib, ci-deps 46 OK, 16 web, .app 启动干净退出 | 清理: cargo clean | 下一步: 恢复点完整, 待续
- iter24 | 完成: drawer handoff 信封展开(Show envelope: memory/skills/MCP/transcript 全渲染) | 验证: 17 web, tsc | 清理: 无 | 下一步: iter25
- iter25 | 完成: queued task 编辑/取消前端测试(updateWorkerTask+cancelWorkerTask 精确分发) | 验证: 17 web, tsc | 清理: 无 | 下一步: iter26
- iter26 | 完成: composer 提交链路测试(choose→runHubPrompt 参数→fresh session reload)+修复 mock 泄漏(clearAllMocks 不清实现) | 验证: 18 web, tsc | 清理: 无 | 下一步: iter27
- iter27 | 完成: worker Interrupt/Restore 前端测试 | 验证: 18 web, tsc | 清理: 无 | 下一步: iter28
- iter28 | 完成: Switch back/Stop 点击分发测试(switchHubEngine 带 reason, stopHubAttempt) | 验证: 19 web, tsc | 清理: 无 | 下一步: iter29
- iter29 | 完成: hub-core 契约测试(未注册引擎切换/busy 拒绝/重复注册拒绝) | 验证: 21 hub-core tests | 清理: cargo clean | 下一步: iter30
- iter30 | 完成: alva shared active profile 装配测试(ALVA_CONFIG_DIR 隔离+静态锁防 env 竞争) | 验证: 25 hub-cli tests | 清理: cargo clean | 下一步: iter31
- iter31 | 完成: transcript sharing toggle 持久化测试 | 验证: 20 web, tsc | 清理: 无 | 下一步: iter32
- iter32 | 完成: stall 分类变体补强(no activity/not responding/hung) | 验证: hub-desktop lib | 清理: 无 | 下一步: iter33
- iter33 | 完成: composer 无 workspace 前置校验测试(不调后端+失败卡片渲染) | 验证: 21 web, tsc | 清理: 无 | 下一步: iter34
- iter33 | 完成: composer 无 workspace 守卫测试(按钮 disabled+Enter 路径错误卡片, 修正为双路径) | 验证: 21 web, tsc | 清理: 无 | 下一步: iter34
- iter34 | 完成: session 消失路径测试(detail null→错误卡片) | 验证: 22 web, tsc | 清理: 无 | 下一步: iter35
- iter35 | 完成: 质量门(全量 lib+ci-deps+打包+.app 冒烟 schema14) | 验证: 137+25+21+9+12 lib, ci 46 OK, 22 web | 清理: cargo clean | 下一步: iter36
- iter36 | 完成: 真实 claude light 集成测试(print adapter 完整 turn) | 验证: 138 lib tests | 清理: cargo clean | 下一步: iter37
- iter37 | 完成: permission 卡片 Allow 生命周期测试(respondHubPermission 精确分发+卡片锁定) | 验证: 23 web, tsc | 清理: 无 | 下一步: iter38
- iter38 | 完成: repair 引导链接测试(Refresh 后 Official guide→openExternal) | 验证: 23 web, tsc | 清理: 无 | 下一步: iter39
- iter38 | 完成: repair 引导链接测试(修正: action 挂载到已渲染引擎) | 验证: 23 web, tsc | 清理: 无 | 下一步: iter39

## 会话 checkpoint（iter38 末，压缩恢复点）
全量绿：hub-desktop 138 lib / hub-cli 25 / hub-core 21 / adapter-codex 9 / adapter-alva 12 / web 23 / ci-deps 46
真实引擎打包运行全覆盖：alva(4) codex(fresh+resume+fallback+worker) claude light(14) alva→codex handoff
修复清单：codex resume -C/-s；alva providers{} 解析；React key 含可编辑字段；clearAllMocks 不清实现(mock 泄漏)
剩余队列(iter39+)：watchdog stall 真实观察；cindy 演进(浏览器驱动/MCP 深化/automation 评估)；worker 中断真实引擎观察
恢复：读本文件续跑；每轮先 cargo test -p alva-hub-desktop --lib -p alva-hub-cli -p alva-hub-core + npm test(web/)
磁盘纪律：每轮 cargo clean；当前 48Gi 可用
- iter39 | 完成: createTeamRun 分发测试(lead+engine 参数, 返回 run 渲染) | 验证: 24 web, tsc | 清理: 无 | 下一步: iter40
- iter40 | 完成: 会话收尾质量门(全量绿) | 验证: hub-desktop 138+hub-cli 25+hub-core 21 lib, ci 46 OK, 24 web | 清理: cargo clean | 恢复: 下会话从 iter41 续
- iter41 | 完成: UI 全面重设计(中性深色+单一 accent+去渐变+排版提升, 用户反馈驱动) | 验证: 24 web, tsc, vite hmr, .app 冒烟 | 清理: cargo clean | 下一步: iter42
- iter42 | 完成: UI 重写后全量回归质量门 | 验证: 138+25+21+9+12 lib, ci 46 OK, 24 web | 清理: cargo clean | 下一步: iter43
- iter43 | 完成: sidebar fallback/team 面板可折叠(默认展开, ▾ 收起) | 验证: 25 web, tsc | 清理: 无 | 下一步: iter44

## 会话 checkpoint 2（iter43 末）
UI 反馈已回应：视觉系统重写(中性深色/单一 accent/去渐变/排版提升) + sidebar 可折叠
全量绿：hub-desktop 138 lib / hub-cli 25 / hub-core 21 / adapters 21 / web 25 / ci 46
恢复指令：读本文件 → iter44 续；先 cargo test -p alva-hub-desktop --lib + npm test(web/)
剩余队列：watchdog stall 真实观察；cindy 演进深化；UI 主区 IA(三列/tab) 候选
- iter44 | 完成: UI 线稿工具(wireframe.js+ui-wireframe.mjs, CDP 零依赖)+用它发现并修 2 个布局 bug(composer-model 撑高 topbar; sidebar 5 面板溢出→fallback/team 默认收起) | 验证: 25 web, tsc | 清理: 无 | 下一步: iter45
- iter45 | 完成: wireframe 工具支持真实窗口尺寸(--size=1180x760 Emulation 覆盖) | 验证: 布局健康(5 面板全首屏) | 清理: 无 | 下一步: iter46
- iter46 | 完成: wireframe 工作流文档化(ACCEPTANCE 新增 UI wireframe review 节) | 验证: 文档 | 清理: 无 | 下一步: iter47
- iter47 | 完成: wireframe 交互后检查(--click)+收尾质量门 | 验证: 138+25+21 lib, 25 web, tsc | 清理: cargo clean | 会话 checkpoint: iter47
- iter48 | 完成: 一键 wireframe 脚本(scripts/ui-wireframe.sh 仓库根可用) | 验证: 运行正常 | 清理: 无 | 下一步: iter49
- iter49 | 完成: End run/autonomy toggle 测试 | 验证: 26 web, tsc | 清理: 无 | 下一步: iter50
- iter50 | 完成: team run 收尾交互测试(autonomy/add worker/end run) | 验证: 26 web, tsc | 清理: 无 | 下一步: iter51
- iter50 | 完成: team run 收尾交互测试(修正: addTeamWorker mock 返回增长列表) | 验证: 26 web, tsc | 清理: 无 | 下一步: iter51

## 会话 checkpoint 3（iter51 末）
全量绿：hub-desktop 138 / hub-cli 25 / hub-core 21 / adapters 21 / web 26 / ci-deps 46
新增工具：web/scripts/wireframe.js + ui-wireframe.mjs + scripts/ui-wireframe.sh（UI 线稿，CDP 零依赖）
UI 反馈已闭环：视觉重写 + sidebar 折叠(默认收起 fallback/team) + 线稿发现修 2 个布局 bug
恢复指令：读本文件 → iter52 续；UI 改动后用 scripts/ui-wireframe.sh 线稿复核
- iter52 | 完成: 本地模型 AI UI 审查闭环跑通(Qwen3.5-9B @10.10.1.100:10443, key 123123)+修复其发现(折叠按钮 28px+aria-expanded) | 验证: 26 web, tsc | 清理: 无 | 下一步: iter53
- iter53/54 | 完成: 9B 视觉审查闭环(VL-4B 服务端坏但 9B 是多模态!)+两轮审查修复(对比度 3.5→4.88, sidebar 分色, ready 语义色) | 验证: 26 web, tsc | 清理: 无 | 下一步: iter55
- iter55 | 完成: 新模型(mxfp4)验证通过+视觉审查扩展 --click+展开态第3轮审查修复(占位符可读性/composer footer 层级) | 验证: 26 web, tsc | 清理: 无 | 下一步: iter56
- iter56 | 完成: 密集态视觉审查(fallback+team 全展开, mxfp4 模型)→结论: 折叠机制正确(9B 建议已实现), 无新 bug, P3 已修 | 验证: 审查完成 | 清理: 无 | 下一步: 数据态审查需造数据
