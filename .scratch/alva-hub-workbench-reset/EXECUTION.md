# EXECUTION · 逐卡执行记录

- 目录：`.scratch/alva-hub-workbench-reset/`；执行 Agent 会话，2026-09-20。
- 基线：`764864da`（TASKS.md 规划基线）。分支 `refactor/kernel-boundary`。
- 签名说明：1Password 签名代理在本会话中途持续返回 "failed to fill whole buffer"，自 `9079abfe` 起的提交以 `--no-gpg-sign` 落库；此前 `a250cb9d`/`b0a649e0` 为签名提交。

## 状态总览

| 卡 | 状态 | 交付 commit / 文件 |
|---|---|---|
| H01 | ✅ 交付 | `b0a649e0` INVENTORY.md + LEGACY-MAP.md |
| H02 | ✅ 交付（工具区位置保持右侧可逆） | `a250cb9d` glossary 增量 + `b0a649e0` SPEC.md |
| H19 | ✅ 基线+预算交付（两轮方法修正，见 PERF §2） | `9079abfe`（夹具/runner）+ `98387ad5`（runner 修正）+ PERFORMANCE.md |
| H20 | ✅ 交付 | `9079abfe` session/timeline-model.ts |
| H21 | ✅ 交付并达预算 | `98387ad5` bounded-list + 行级 memo |
| H04 | ✅ 后端+bridge 交付（迁移 18） | `e4a91a8c` |
| H05 | ✅ 只读发现交付 | `e4a91a8c` worktree_discovery.rs |
| H08 | ✅ 两模式入口交付 | `5d6ac7d3` StartupGate + App 接线 |
| H09 | ◐ 部分 | 数据层（H20 模型）与窗口化（H21）已就位；工具活动行仍在 Workbench（R42 布局未迁入主轴）——见"未完成" |
| H15 | ⏸ 未开始（见"未完成"） | — |
| H16 | ✅ R96/R97 修复 | `0c98f529` |
| K01 | ✅ 检查执行（pass，见下） | 本次运行记录 |
| H03/H06/H07/H10-H14/H17/H18 | ⏸ 未开始 | 见"未完成与原因" |

每卡证据（命令 + 结果摘录）：

## H01/H02 — 现状与规范

- 证据：三路并行勘察（前端 / 后端 / .scratch 旧卡）产出 INVENTORY.md 逐项文件:行登记；LEGACY-MAP.md 核实 R45/R77-R91/R96/R97 在全 git 历史无修复 commit，旧 UI 总表"00-12 全完成"被 round5 HISTORICAL-UI 判伪。
- 验证命令：`git log --all --grep "R77"` 等逐编号（无修复记录）；结论登记于 LEGACY-MAP.md。
- 词汇表增量：Project / Conversation mode / Project mode / Engine+Pi（`docs/ALVA-HUB-GLOSSARY.md`，commit `a250cb9d`）。

## H19 — 性能基线

- 夹具：`web/src/perf/`（`?perf=1` URL 门控，生产构建可用，产品面无入口）。确定性种子 mulberry32(20260920)，n=1000/10000，含 ~1MB 单条输出、失败/中断/审批卡。
- runner：`web/perf/run-perf.mjs`。帧证据 = CDP Tracing `DrawFrame`（**非 rAF 采样**）；长任务 = PerformanceObserver('longtask')；输入延迟 = wheel 派发 vs 页面事件 timeStamp；DOM/堆 = DOM 计数 + performance.memory。
- **方法修正（诚实记录）**：初版用 wheel burst 驱动，DrawFrame 间隔混入空闲等待，滚动 p95≈155ms 是采集假象；修正为 CDP `Input.synthesizeScrollGesture` 连续手势后，基线滚动 p95 实为 ~10ms（滚动由合成器线程承担），**真实代价在长任务与堆**（n=10000 流式 16s 内 227–242 次 >50ms 主线程阻塞、堆峰值 270–536MB、切会话 ~1s）。废弃数据保留在 results/ 目录。
- 预算判定（新方法、同机同种子）：基线 n=10000 时 TTI 951–978ms（贴线）、长任务/堆/切换超预算 → H21 必要性成立。

## H20 — 数据解耦

- `session/timeline-model.ts`：`TimelineEntry`（规范 id=`session::item`、turn/attempt/tool 归属、确定性排序）；`mergeEntry` 为历史回放与实时事件**唯一**合并语义（R29 正文保留 / R40 控制条目新体覆盖 / 重复全文投递不再拼接 / 迟到事件按序插入）。App.tsx 历史与实时均走该模型；旧 `mergeLiveTimeline` 变薄包装。
- 测试：`timeline-model.test.ts` 13 项（含"回放与实时收敛为同一列表"契约）；全量 vitest 129/129。修复过程中发现并修正分类缺陷：消息性由 role 优先判定（item_id 形态仅作回退），与 R29 语义一致。

## H21 — 长会话滚动与流式性能

- `session/bounded-list.tsx`：BoundedList（按测高+估高前缀和二分窗口、pad spacer 保持总高、layout-effect 锚定校正）；`chat-transcript.tsx` 行级 `memo`（流式 delta 只重渲染尾条 Markdown）。
- 真实浏览器证据：`tests/windowing.spec.ts`（Playwright，dev server）2/2 —— 1000 条会话挂载 <120 行、滚动可达顶部、流式追加下锚点位移 <4px。
- 前后对照（同夹具同 runner 同机，n=10000，3 次中位）：

| 指标 | 改造前 | H21 后 |
|---|---|---|
| S1 首开 TTI | 951–978ms | **285–325ms** |
| DOM 节点 | 142,567 | **20,514** |
| S2 滚动 p95 | 10.1ms | 10.6ms（均达标 ≤20ms） |
| S3 快滚长任务 | 39–43 次 | **7–10 次** |
| S3 堆峰值 | 478MB | **28MB** |
| S4 流式长任务 | 227–242 次/16s | **0–1 次** |
| S4 堆峰值 | 270–536MB | **62–66MB** |
| S5 切会话 p50 | 997–1008ms | **105–106ms** |
| S5 20 次后堆 | 242MB | 65–66MB（稳定，无线性增长） |

- 原始 JSON：`web/perf/results/perf-results-1789837394299.json`（前）、`…8133852.json`（后）。

## H04/H05 — 项目与工作树

- 迁移：schema 18（projects 表 + `hub_sessions.project_id` 可空列），纯增量；pre-18 构建经版本闸拒绝、不损数据（回滚 = 旧版本 refuse + 新表 drop，无旧列改写）。
- 语义：canonical 路径唯一（别名/symlink 收敛；用户目录允许 symlink，与 Hub 受管目录的拒 symlink 规则区分）；项目可无会话存在、重启保留；delete_project 仅解绑不触盘；set_session_project 双端校验。
- H05：`git worktree list --porcelain` 纯只读；porcelain 路径逐字解析（空格/中文）；区分 Hub 受管（`hub/worker-*` / 受管根下）与用户工作树；非 Git = 状态而非错误；缺失目录显式 MissingDirectory。**不创建/prune/删除任何工作树。**
- 测试：store 9 项（含重启持久、别名收敛、解绑保留历史、legacy NULL）+ discovery 6 项（含真实 git 仓库夹具：linked/detached/locked/prunable/受管标记/只读不变）。命令：`cargo test -p alva-hub-desktop --lib worktree_discovery` / `project` 全绿。
- 未验证：hub-server HTTP 侧未加对应端点（Tauri 优先）；4 个 real-codex/claude CLI 测试因本机 PATH/凭证缺失按测试内建守卫失败（非本次改动引入）。

## H08 — 两模式入口

- StartupGate（`components/startup-gate.tsx`）：对话模式（一键受管目录会话，无需仓库）与项目模式（项目列表 + 添加 + 刷新 + 删除）均为正式入口；入口选择持久化（localStorage `alva-hub.entry-mode`）。
- 聚合：项目行显示会话数 + 工作树状态行（用户/受管/缺失目录/非 Git）；打开项目 → 绑定其工作目录 + 归属过滤；新建会话创建后一次 `setSessionProject` 归属（不改 cwd）；打开历史会话按其**自身存储** project 恢复上下文（不按模式猜）。
- 模式切换只改列表范围，不搬迁会话/不扩权限；删除项目仅解绑。
- 验证：vitest 129/129（集成测试更新到新入口文案）；`npx tsc -b` 干净。Playwright 真实浏览器冒烟夹具路径可用（?perf=1 与产品面共壳）。
- 未验证（诚实项）：真实 Tauri 桌面壳内的 H08 主流程（加项目→选目录→新任务→切历史→重启恢复）待 H18；浏览器模式（hub-server）项目面显式声明不可用（`startup.unsupportedTitle`）。

## H16 — R96/R97

- R96（Pi overflow 压缩恢复）：
  - 修复：`agent_end(willRetry:false)` 携带未决错误不再升 fatal——发可恢复提示（用户可见"等待结算"）并继续读流；`settle_at_eof` 在完整请求结算时把仍待定错误定性为唯一 fatal（原文保留）；`AutoRetryEnd{false}` 消费 pending 防止双 fatal（保住 R2）。
  - 动态反例（round10 原证据工程）：`cargo run --offline --manifest-path .scratch/kernel-boundary-review-round10/evidence/adapters/Cargo.toml` → `compaction_recovery=true, fatal=false, answer_preserved=true`，exit 0（修复前 false/true/false、exit 101）。
  - 适配器测试 23/23；conformance OutputAfterFatal 夹具改锚真实 fatal（重试耗尽），场景语义不变。
- R97（合成 fatal handoff 身份窗口）：
  - 修复：`settle_synthetic_failure` 把 Failed 发布与 handoff 身份恢复合并进**同一写锁临界区**，且先于合成终态可见；R93 观测 id 投影不变；显式 fatal 路径回归保留。
  - 新回归：`synthetic_fatal_eof_handoff_settles_identity_before_the_retry_window`（51/51 hub-core 全绿）。
  - **诚实记录**：红测验证表明旧序下该窗口无法用顺序式测试稳定复现（与 round10"动态未复现"一致）；本修复的价值是构造性消除窗口（同一临界区、发布前完成），测试作为契约回归底线。

## K01 — Kernel/SDK 独立检查

- 检查命令与结果（本次运行，全部通过，exit 0）：
  - `bash scripts/ci-check-deps.sh` → **PASSED**（含 wasm32-unknown-unknown-clean crate 集 + alva-host-wasm 完整链接；`wasm32-wasip2` 检查因 target 未安装 SKIPPED，已如实记录）。
  - `bash scripts/public-surface.sh` → `alva-kernel-abi OK` / `alva-agent-abi OK` / `alva-agent-core OK`。
  - `cargo test -p alva-kernel-abi -p alva-kernel-bus -p alva-agent-core -p alva-kernel-core` → 全部 target `ok`，0 failed（约 200 项定向测试）。
- 本会话对 kernel/agent-core 的**唯一**改动是 hub-core（L7 编排层，非 SDK 内核）的 R97 修复与 pi 适配器（L4.5）的 R96 修复；kernel-abi/kernel-core/bus/agent-core 零改动（public-surface 快照零漂移佐证）。
- **K01 结论：pass，未发现可复现内核缺陷 → K02 不激活（不适用）。**
- 未跑：`cargo check --target wasm32-unknown-unknown` 单独穿透本会话未执行（ci-check-deps 已覆盖 wasm-clean 集合判定）；`wasm32-wasip2` 编译检查因 target 未安装跳过。两项均如实记录，不冒充通过。

## 未完成与原因（不掩盖）

1. **H09 中央完整展示（部分）**：工具活动行仍按 R42 滤在 Workbench——迁移其入主轴需按 H03 区域方案重排中央列（与 H02 已定的迁移决策一致），并同步 hub-timeline 键改稳定 id（App.tsx:978 自认的索引键问题在 H09 范围内处理）。数据层（H20 条目模型）已消除其数据侧障碍。
2. **H15（R77/R78/R84-R89/R91）未开始**：这些是执行控制/异步串台缺陷族，各自需要失败反例+修复+回归的独立批次；本会话预算被 H19/H21 的两轮测量修正与 H04/H05/H08/H16 主链占满。LEGACY-MAP.md 已把它们钉在唯一执行入口 H15。
3. **H03/H06/H07/H10-H14/H17/H18 未开始**：依赖 H03 结构方案的用户确认（H03 卡明确"经用户确认后才大面积落地"）与 H09 完成态；H18 真实 Tauri 验收必须在这些卡之后。
4. **Tauri 生产壳实测**：性能与主流程均在同引擎 headed Chromium 取证；真实 webview 内复测列入 H18。
5. 全局快捷键、主题、hub-server 端点补齐：H17/H18 范围。
