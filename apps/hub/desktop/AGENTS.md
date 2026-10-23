# L7 · Hub native UI product
> 独立Hub产品UI，复用基础自绘controls但不污染直接harness入口的依赖图。
## 地位
仅通过 apps/hub/product 共享 UI 无关业务；共享 apps/ui/chat 与 apps/ui/modules 控件包不依赖 direct、Hub 或执行 owner。
## 逻辑

正式工作台复用21/22设计：220侧栏、52页头、限宽欢迎输入区和独立滚动对话。首页Build/建议/新任务导航不创建会话，明确首条Send才经既有Create、可选Switch、所选文件逐项Upload、初次Detail和冻结prompt的Submit准入；初次及同会话重载完成前禁发，准入隔离此前旧观察；失败保留草稿和已提交身份。项目目录仅通过原生显式选择用于新会话，不改变已有会话或授予权限。已有高级面板/故障切换/播放收尾仍有明确入口。

应用设置拥有独立导航壳和同一返回来源；分类切换不覆盖来源，返回保留会话/Inspector定位及草稿，媒体离开仍真实join并释放正文，返回只恢复冷索引位置。Stop按本窗session和不可变执行owner身份记录未完成请求，跨会话导航不丢失，实际终态才清理；提交回执仅清理匹配的捕获草稿，不覆盖后续输入。

General是会话外的应用偏好页；原生Select只编辑本窗草稿，明确Save/Reload复用共享PreferencesService与有版本的原子保存。Build/观察仅读已提交内存快照，theme/density/body size在所有窗口生效；外部修改、冲突与存储失败保留草稿，实际保存返回前阻止关闭其窗口。Reduced motion只读系统偏好，不伪装独立应用功能。

AEP pane区分原生JS/Python的完整host访问与嵌入式WASIp1的无目录/环境/网络运行，二者都先检查disabled pin并单独批准下一Turn；空interpreter仅选择manifest=wasm。
apps/hub/product.HubService拥有产品认领与原始文件耐久保存，UI只提交引用清单与读取有限产品历史；本包仅拥有页面交互、轮询与展示转换。
原生窗口独立250ms观察时钟最多一个排队UI closure，进度/审批与终态查询不依赖Build或compositor重绘；关闭observer取消时钟，过期closure不查询或Stop。自绘Build仅呈现当前状态。
等待pane复用同一观察时钟冷读已提交checkpoint；其他窗口保存/继续/放弃会更新本窗，只为同branch/Turn、同请求ID与binding的未解决请求保留草稿。revision变化撤销旧放弃确认，明确操作/刷新后的旧读不得回盖；读失败禁用checkpoint操作，成功后恢复，不自动执行。
持久问答复用同一binding草稿与既有interaction.Answer，提供无默认选项的单选/多选/自由文本；显式保存后仍暂停，不授予文件/进程/网络权限。结果按实际编码后的16KiB限制；到期请求只展示不再接受回答或审批。
每个问题用请求ID建立独立控件容器，输入标签带问题header；同一检查点的其他问题消失时，剩余问题的草稿/焦点不得串到别的请求。
等待面板顶部固定Refresh/Continue，完整请求、回答与放弃审阅位于独立滚动区；长问题/已保存结果不能把唯一继续按钮挤出视口。
会话侧栏在selected detail的UpdatedAt不旧于当前列表项时投影其真实状态，避免正文已完成而侧栏卡running；不修改冻结列表的顺序、成员或分页cursor，也不拿较旧detail覆盖较新列表。
输入区浮层按实际边界定位：/与@在整卡上方6px，控件菜单优先在各自触发器上方；不足时在视口内翻转/滚动。MyGo AttachTo同时绑定焦点与几何，picker保留Message作为owner，以实际frame差值定位整卡外沿；仅几何变化时补一帧，不能加入额外Tab停靠点或持续重绘。输入区8px对称内距、工具栏32px，Send/Stop为32px圆形。

## 约束
用户预览只保留一个明确的客户端与数据目录；更新前核对未发送草稿、待导入文件和真实运行状态，保全后正常退出再更新，不逐轮另起预览客户端。自动化画面检查优先使用独立无窗口宿主。
无WebView/npm/React。用户显式文件选择才读取本地原件，20MiB/文件、8文件/40MiB每轮。原始媒体不自动送模型，不假称压缩/抽帧。消息呈现不是模型历史权威。
媒体evidence只开4条metadata页，显式manifest/selection/receipt点击才读有界JSON，Preview/显式Play才读全SHA核验派生BLOB；一个bitmap、最多一个pending帧UI closure。仅单独批准decoder后Play播放已验证静音MP4；已保存canonical WAV可独立显式播放，MyGo拥有共享系统output，window只拥有一条voice；声音故障不报成功。Pause/Seek/导航须join自身voice或真cancel/reap decoder，未完成保留owner并阻止关窗；无autoplay/WebView/模型投递，5fps播放clock不冒充source PTS，diagnostics非持久receipt。真实OS窗口可观察QA仍待完成。
## 业务域清单
| 文件 | 职责 |
|---|---|
| workspace_picker_desktop.go / workspace_picker_desktop_test.go | 原生输入/显式菜单触发的命令、Skills及项目引用搜索；取消/迟到结果隔离、键盘选取仅准备草稿 |
| workspace_plan_desktop.go / workspace_plan_desktop_test.go | 首页本窗计划意图与已存会话真实下一Turn策略，成功保存才更新branch缓存 |
| workspace_prepared_desktop.go / workspace_prepared_desktop_test.go | 捕获typed选择、首发迁移/失败保留及只消费匹配回执，不覆盖新草稿 |
| workspace_shell_desktop.go / workspace_shell_desktop_test.go | 正式导航、页头、限宽首页、仅编辑草稿的建议与真实高级菜单；冷构建/几何/菜单回归 |
| workspace_composer_geometry_desktop_test.go | 首页/对话短长输入的浮层上方锚点、工具栏不被覆盖、32px圆形操作与8px对称内边距、紧凑菜单触发点回归 |
| workspace_composer_desktop.go / workspace_composer_desktop_test.go | 多行输入、真实附件/路由/权限说明及发送/停止状态；IME、32KiB、浮层、草稿和双轴对齐 |
| workspace_attachments_desktop.go / workspace_attachments_desktop_test.go | 首页待导入路径、明确发送时复用Upload、部分导入失败只重试剩余项、保留原件和引用、分支不兼容提前禁发 |
| workspace_settings_desktop.go / workspace_settings_desktop_test.go | 真实设置分类共用左上返回、原会话/视图与媒体冷索引定位恢复；草稿/运行/播放收尾回归 |
| workspace_lifecycle_desktop_test.go | 真实取消收尾期间跨会话保持Stopping与防重Stop、延迟准入期间新输入不被旧回执擦除 |
| workspace_actions_desktop.go / workspace_actions_desktop_test.go | 明确首发的真实Create/Detail/Submit、部分失败身份保留、草稿导航、同会话重载禁发与持久fallback可取消回归 |
| workspace_view_desktop.go / workspace_routing_desktop.go / workspace_project_desktop.go | 高级面板分派、保存分支与账户/计费归属的fallback批准/禁用、原生项目目录选择 |
| workspace_assets_desktop.go / assets/ | 一次注册有平台回退的设计字体、固定Lucide原始SVG与语义表面色；包含来源哈希、完整OFL/ISC/MIT许可 |
| preferences_desktop.go / preferences_desktop_test.go | 无会话General页、共享选项的实际原生Select与明确Save/Reload、保留冲突/失败草稿；跨窗theme/density/body size和只读系统reduced motion，SQLite/bootstrap重开、冷Build与保存期间关窗守卫的无窗口回归 |
| wait_desktop.go / wait_desktop_test.go | 默认off等待面板、完整请求/已保存结果与可复制Turn ID、跨窗观察/绑定草稿/旧读排除/读失败禁用；只保存结果、同Turn继续和重新审阅后放弃，1240×850无窗口控件回归 |
| wait_question_desktop.go / wait_question_desktop_test.go | 持久问题单选/多选/文本与实际编码限额；关闭重开后保存/恢复同Turn、澄清后写入仍单独审批、过期禁用、并存问题焦点/草稿隔离及1240×818滚动/按钮视口回归，不要求手写JSON |
| app_desktop.go | 正式工作台装配、共享虚拟消息、异步会话/观察/发送与明确审批；高级面板由workspace入口分派 |
| panes_desktop.go | MCP/native SubAgent显式settings、固定高水位Inspector、typed child钻取/返回父页与2KiB原文 |
| export_desktop.go | 已选branch/typed子路径的实际JSONL私有导出；可选真实关联child，失败不报成功，Close等待写入；分类只筛当前metadata页 |
| aep_desktop.go | native-only显式文件夹/JSON检查、disabled pin保存、独立host同意与下一Turn启停 |
| aep_desktop_test.go | MyGo实际输入JSON检查与独立批准，不启动任何模型/解释器 |
| commands_desktop.go | 原生Hooks/diagnostics明确argv JSON编辑、独立host批准与下一Turn启停，不自动运行诊断 |
| commands_desktop_test.go | MyGo实际JSON/批准和持久memory opt-in控件，保存/观察不启动任务 |
| shell_desktop_test.go | 真实应用内后台job冷列表与native显式Stop等待reap，不启动模型 |
| tools_desktop.go / tools_desktop_test.go | 六类工具原生配置/独立enable、跨pane真实单选/多选/自由回答控件，问答不增加权限；同pane的后台Shell列表/Stop/idle reset复用应用owner |
| gateway_desktop.go / gateway_desktop_test.go | 实际嵌入Gateway显式start/Stop与可复制一次token，pane/status不调用模型、stop清token |
| media_settings_desktop.go | 默认off媒体工具、显式decoder绝对路径、编辑禁用和独立host批准，不自动启动 |
| media_desktop.go / media_desktop_test.go | cold来源/产物/selection/receipt分页、显式实际JPEG预览/释放及MyGo无窗口回归 |
| browser_desktop.go / browser_desktop_test.go | explicit browser JSON禁用保存/独立host-network批准/编辑撤销与零启动控件回归 |
| handoff_desktop.go / handoff_desktop_test.go | 仅当前bounded历史页的handoff ID→显式读取exact已保存脱敏snapshot，不恢复或执行 |
| playback_desktop.go / playback_desktop_test.go | actual ffmpeg+MyGo Play/Pause/seek、单Bitmap.Update或audio mixer position、one-closure背压、导航回收；fixture双流排空后回收，失败也清理；真实OS窗口另验 |
| audio_desktop.go / audio_desktop_test.go | canonical WAV→有界in-memory MyGo voice、显式Play/seek/Close，设备错误不冒充可听成功；冷构造/非发声fake voice测试，不声称真实声学验收 |
| automation_desktop.go / automation_desktop_test.go | 普通会话paused-first CRUD、固定账户/计费确认、编辑暂停与二次config-only删除；不私自Tick或执行 |
| app_desktop_test.go | MyGo无窗口启动/输入/停止状态与selected侧栏终态/新旧快照/cursor隔离回归 |
| observer.go / observer_test.go | 独立时钟的有界UI观察；无重绘仍刷新，慢UI不堆积，关闭不迟到读或Stop |
| debug_desktop.go / debug_desktop_test.go | 独立诊断owner注册实际产品视图/草稿/创建与选择/Send/Stop，所有action走原方法与operator控制；状态不暴露配置/问题正文，凭据页及尚无current Build的旧scene拒绝截图；真实HTTP+MyGo Tester+durable create不执行模型 |
| windows_desktop.go / windows_desktop_test.go | 最多8个独立草稿/视图窗口共享单一product/runtime，关闭非最后observer不Stop；最后窗口active时要求真实Stop |
| skills_desktop.go | 受管项目skills原生编辑/下一轮enable，以及显式compatibility配置文件的下一轮启停；无目录时仍可使用规则插件 |
| compatibility_desktop_test.go | MyGo实际配置路径输入/明确启停、未初始化native Skills仍可用与零执行回归 |
| transcript.go | bounded产品历史转稳定ID、typed消息行；不是模型上下文 |
| transcript_test.go | 消息ID、类型与重叠分页去重回归 |
| team_desktop.go | 原生collaboration、真实Worker会话focus、附件草稿只引用入队并固定原branch、FIFO编辑/取消/分页、显式resume/stop和持久生命周期 |
| team_desktop_test.go | MyGo实际输入建Team/Worker且不执行，跨pane审批可见回归 |
| worktree_desktop.go / worktree_desktop_test.go | explicit首Turn前隔离、冷SHA/path索引与8KiB raw diff页、二次确认真实采纳及整树删除审阅/独立确认/Worker退休；离开释放正文、未知不重试，无OS窗口验收主张 |
| approvals_desktop.go | 所有pane共用显式one-call审批控件，展示actual请求InputSummary供审阅，不藏在Chat之后 |
