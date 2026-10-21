# L7 · Hub SQLite product authority
> 独立 product.sqlite 保存产品事实，不复制或恢复 native 执行正文。
## 地位
具体存储依赖 `hub` 与安全SQLite宿主封装；Hub core 不反向依赖本目录。
## 逻辑
独占进程锁后打开0700/0600私有库、建表并修复中断Turn。新Turn和首Attempt、fallback认领、事件/终态都在事务提交；固定产品归属与activeAttempt防止迟到结果覆盖新轮。
## 约束
首次版本不读取旧Rust数据；user_version=0但已有任何业务表的库拒绝准入，不向未知库补建Hub表。最多64分支、7个授权fallback路由；history每页128项。handoff只传32条有界Turn索引，超量明确要求检索，不静默声称完整历史。Stop/可能副作用禁止自动fallback。实例锁释放与DB关闭由Store.Close显式拥有。
所有公开Store错误在provider边界映射共享hub.ErrNotFound；product/Tool/HTTP不得识别sql.ErrNoRows。内部SQL原因可保留供诊断，事务/schema行为保持。

Waiting保留Turn身份、Worker Task及Automation的RunningTurnID；运行资源可以已退出，队列/timer不因此认领下一轮。只实际已开始且有合法checkpoint receipt的Attempt能提交Waiting；crash edge恢复保留该事实，无自动继续。

## 业务域清单
| 文件 | 职责 |
|---|---|
| wait.go / wait_test.go | 独立WaitStore、branch归属审阅、同Turn新Attempt事务、waiting冻结与Worker/计划crash edge/no fallback回归 |
| store.go | schema、产品事务、分支保留、handoff、分页历史与中断修复 |
| store_test.go | 可逆路由/安全fallback、native事实归属、commit失败/忽略sink错误不能成功/大handoff/恢复回归 |
| settings.go | session-scoped默认禁用编辑、独立host确认启用与idle-only耐久snapshot |
| settings_test.go | 配置导入不自批、归属/active限制和关闭重开耐久回归 |
| compatibility_test.go | 项目相对配置路径默认off、导入不启用、idle-only与关闭重开保存；provider不读文件 |
| tools.go / tools_test.go | 六类可选工具默认off、编辑清授权、Shell/Web独立access批准、idle-only和重开耐久 |
| aep.go / aep_test.go | inspected AEP pin独立批准/编辑失效/idle-only与重开耐久 |
| commands.go | Hooks/diagnostics编辑清批准、独立explicit enable、idle-only的next-turn进程snapshot |
| media.go / media_test.go | 默认off媒体能力、decoder编辑清批准、独立host同意、idle-only和重开耐久回归 |
| browser.go / browser_test.go | 显式fresh-browser snapshot编辑禁用、独立批准、重开与active不变回归 |
| native_owner.go | native root到唯一product owner的metadata-only反查，不读native正文 |
| sessions.go / sessions_test.go | 首页全历史keyset/字面检索，仅主会话、不把Worker当无关会话 |
| timeline.go | shared product clock下分支/fallback/handoff typed时间线 |
| team_snapshot.go / team_snapshot_test.go | 同事务Lead完整roster/Worker仅self的handoff metadata，队列不消费、16worker/16KiB与重启冻结证据 |
| handoff_policy.go | 保存和engine编码前复用shared known-credential redactor，不复制regex或称任意秘密检测 |
| team.go | 真独立Worker会话、FIFO事务认领/附件草稿RequiredBranchID同事务准入和claim、parked head不预约其他Worker资源、older-head公平资源预约、生命周期与Lead真实终态回执 |
| team_test.go | Worker身份/路由/队列、休眠/归档/恢复、crash未知不重放回归 |
| activity_test.go | native build/prepare启动前Activity耐久风险与6类effect fallback回归 |
| provider_failure_test.go | 真实Alva HTTP provider分类驱动fallback，mixed SessionError与prepare风险禁止重放 |
| automation.go / automation_test.go | owner-scoped乐观计划CRUD、明确计费批准、普通Turn同事务认领/收尾；busy/missed跳过、Stop/崩溃暂停且绝不自动fallback/重放 |
| worktree.go / worktree_test.go | 首Turn前one-way隔离、cwd原子绑定、revision证据/明确adoption；独立removal review/明确删除后retire，不retarget历史；未就绪/未知准入阻断与crash未知不重放 |
| errors.go | SQL内部错误到共享Hub语义的边界映射 |
