# L7 · Hub shared product services
> Headless与native共用同一产品业务、DTO与事实权威，不依赖desktop/MyGo。
## 地位
位于纯 Hub core 之外的 L7 产品服务层；可消费 harness、platform/host、platform/storage 与显式 integration，不能被 SDK 反向依赖。
## 逻辑
认领HubSession后绑定native owner。上传原件耐久保存，模型仅收小引用和真实处理说明；Inspector和历史按目标归属有界只读。UI的行/Markdown/滚动转换留apps/hub/desktop。
## 约束
无MyGo/WebView/React；不靠build tags隐藏UI传递依赖。读取Inspector不restore、不启动Agent/MCP。History/Team工具验证actual Tool与非空matching call、精确字段/重复键/Unicode；产品归属由已存branch重审，不凭同名Tool或nativeID字符串推断。设置仅下一次装配消费。Skills限定saved cwd与可恢复归档。
## 业务域清单
| 文件 | 职责 |
|---|---|
| service.go | 产品session/Turn服务、expected-branch原子准入、20MiB UploadReader原文件引用与显式审批，持有独立Gateway/问答owner |
| service_test.go | 真实双store/local harness、上传/权限/历史/Inspector回归 |
| history.go | 产品绑定的有界metadata/字面关键词/正文range Tool与Plugin |
| inspector.go | 消费registered EngineInspector；分支/最多8hop真实typed child授权、小索引/搜索/2KiB正文DTO/背压live metadata、默认禁用MCP导入 |
| export.go | 精确branch/typed child授权的bounded JSONL；HTTP只返回bytes，native显式保存到私有exports，不接受client host/native path |
| approvals.go | 真实running root/typed child权限路由，不接受UI childID或借用父权限 |
| aep.go | native-only显式source检查与disabled pin保存、下一Turn严格StagePinnedAEP；不是HTTP任意host-path导入 |
| commands.go | native/operator-only显式Hooks/diagnostics JSON disabled编辑，不读取或执行命令，也不作为HTTP任意命令导入 |
| tools.go / questions.go | native严格工具JSON编辑与显式下一Turn开关；actual root/typed ancestry过滤的独立真人问题/回答，不借用审批授权 |
| browser.go | native/operator-only browser严格JSON disabled编辑，不发现exe/启动进程/导入用户profile，不是HTTP任意host-path入口 |
| skills.go | saved cwd受管技能初始化、精确文档、Install/Archive/Restore |
| media.go | 已存branch→native的冷媒体索引/manifest/完整哈希预览/receipt，共用Library且不启动decoder/model |
| playback.go | 显式本地Play的product/branch/transcode-only归属；视频需独立decoder批准并构造caller-owned Playback，音频返回SHA/canonical验证PCM bytes；不发模型/新receipt |
| skills_test.go | 完整frontmatter、归档receipt与symlink祖先拒绝 |
| locks.go | 同一实际SDK ToolLockRegistry装配Plugin，非第二执行runtime |
| team.go | 有界owned Worker队列pumps、显式resume/stop、drain边界kick与账户路由准入 |
| team_test.go | 实际完整Alva Worker FIFO/同native续轮、observer detach与byte-only上传预算 |
| team_tool.go | 绑定实际Lead/native的hub_team Tool/Plugin，mutation HITL、小索引/2KiB读取 |
| team_tool_test.go | foreign native拒绝、mutation拒批零变更与显式批准真实Team回归 |
| automation.go / automation_test.go | 显式owned本地timer与真实SDK普通Turn；构造不执行、下一完整间隔、无补跑/OS daemon，关闭先阻止准入再交既有Runtime收尾 |
| shell.go | 已存分支授权的应用内后台job冷状态/真实Stop/idle reset，无PID或nativeID客户端输入 |
| worker_draft.go / worker_draft_test.go | Worker附件仅本会话引用入队，事务固定原branch，失败保留草稿、入队不执行，不转移Lead/sibling原件 |
| worktree.go | 已认领Lead/Team/Worker的explicit隔离、冷SHA index/raw diff小页与双workspace租约下二次确认原字节采纳；整树/index/HEAD删除审阅与单独确认、retire保留历史/证据，不接受client host/native path |
