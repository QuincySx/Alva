# providers

> 来源：旧 JSON 包的 `requirements/providers.json`（该包已于 2026-11-09 删除，见 git 提交 690c4525）。本文件是这些需求的正本；页面位置按新包路径和元素 `data-name` 链给出。

## req_alva_provider_source — 来源选择与预设数据

状态：23–26是现有设计契约；静态原型只演示代表导航。当前实现及验收范围见本条末尾，未落地的认证来源仍保留需求。
触发/前置：23的添加连接→24来源选择；只读搜索与选择不创建连接、不联网、不启动本地服务或发现账户。选择预设→25，Custom/Local→26；25/26复用ProviderForm而不是步骤二/三。
- API服务预设、自定义/本地HTTP、本机Agent账户是不同认证来源。OpenAI、Anthropic、Gemini的名称、原生协议及完整请求地址是实例数据；不能用同一API Key表单伪装全部账户认证。
- OpenAI示例为openai_responses与https://api.openai.com/v1/responses；其余原生协议为openai_chat、anthropic、gemini。Endpoint是完整原生请求地址，不由SDK猜测补路径。Gemini须按准确模型ID得到完整地址，未解析模板不能冒充可调用地址。
- Local只配置用户已启动的本地HTTP服务；认证按来源明确选择无认证/API Key。自定义头与Key同样属于秘密数据。预设只初始化草稿，不测试、不授权。
- 本机Agent账户目标仍需用户主动检查、确认和对应adapter认证分支；不得扫描旧配置/凭据、把订阅账户提取成通用Key，或冒充已发现登录。
退出：创建表单Back回来源，Cancel清除本次未保存草稿回23；编辑模式携带既有ID/revision直接返回23，不通过换标题创建重复连接。来源切换草稿规则以req_alva_navigation_wizard为准。
owner：窗口只持有来源与表单草稿；共享product连接服务校验、持久化及发布目录，bootstrap负责可调用路由装配；SDK不拥有供应商目录或账户政策。
本波运行对齐（待本波实际回归确认）：原生提供OpenAI/Anthropic/Gemini/Custom/Local的手工来源与表单。未提供本机Agent账户检查/认证入口；该需求未被删除。测试连接与自动目录另见req_alva_provider_form/models，不以CRUD通过代替。
验收：预设初始化正确协议和完整地址；打开/搜索/Back/Cancel零会话、零模型/工具执行；新建/编辑各自保留正确身份；无账户能力时不显示已发现/已连接。

当前确认（2026-10-26）：本机Codex复用明确选择的现有CLI配置和登录状态；默认app-server，原exec仅显式选择。Alva不读取/复制授权秘值、不新增产品账户登录流程。厂商sandbox/approval policy独立显式配置，不映射Ask/Auto/Full为同义权限。真实审批与人类提问走两个请求种类，绑定实际execution/attempt/engine/native身份；旧回执拒绝。Claude只做本地fixture，不调真实账户。当前接线和真实账户验收分别见MIGRATION。

绑定位置：
- `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / APISources / ChooseOpenAI`
- `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / APISources / ChooseAnthropic`
- `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / APISources / ChooseGemini`
- `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / OtherSources / ChooseCustom`
- `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / OtherSources / ChooseLocal`
- `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / OtherSources / ChooseAccount`

标注：
- [behavior-contract] req_alva_provider_source — Preset/OpenAI→共用25；预填Responses完整地址，只突出Key。 — `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / APISources / ChooseOpenAI`
- [behavior-contract] req_alva_provider_source — Preset/Anthropic→同一共用表单的名称/协议/完整端点数据；无重复画板。 — `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / APISources / ChooseAnthropic`
- [behavior-contract] req_alva_provider_source — Preset/Gemini→共用表单；原生请求地址须结合准确模型ID显式解析。 — `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / APISources / ChooseGemini`
- [behavior-contract] req_alva_provider_source — Custom→26，保留完整协议/端点和认证配置。 — `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / OtherSources / ChooseCustom`
- [behavior-contract] req_alva_provider_source — Local→同一Custom表单的显式本地配置，认证决定Key显隐；未启动服务不可假称就绪。 — `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / OtherSources / ChooseLocal`
- [behavior-contract] req_alva_provider_source — Account认证分支仍是设计需求；本波无账户检查/发现能力，不伪装成Key接入或已登录。 — `pages/连接/24 选择供应商.html` → `SheetBody / Dialog / OtherSources / ChooseAccount`

## req_alva_provider_form — 共用连接字段、折叠与测试

状态：共用ProviderForm Source=Preset/Custom两种结构；创建/编辑、供应商名称与内容均为实例数据，不增加screen、Token或状态组合轴。
触发/状态：创建选择来源或23连接设置进入同一表单；编辑读取既有稳定ID与revision。Preset默认突出Key，ConnectionSettings展开现有名称/认证/协议/完整端点组；Custom直接显示完整字段。草稿→校验→保存中→成功或错误；错误定位Field并保留可恢复输入，不能静默回到空表单。
- API Key以密码输入遮蔽；普通连接读取DTO只返回HasAPIKey及HeaderNames，不回传秘密。编辑时Key留空表示保留；显式Clear表示清除，Auth=none同时清除Key；替换必须是明确输入。自定义头只有ReplaceHeaders=true才按显式全量替换，否则保留；未提交不改已保存值。
- 本波秘密由程序选定的私有SQLite中metadata/credential分表保存，权限0600；不宣称系统钥匙串、加密或第三方安全库已实现。不能进入普通配置导出、聊天、诊断、错误文本或日志；窗口取消/结束表单释放本窗秘密草稿，不删除已保存凭据。
- 测试连接由共享ConnectionsService显式提供：用户显式触发，使用当前草稿快照，复用Button Loading/Disabled与Field/Notice。成功/认证拒绝/超时/取消分别来自实际owner；测试结果与目录获取、模型启用、Agent兼容性分开。草稿变更后旧结果不能证明新配置已验证，关闭/取消须由真实owner清理请求。
- Test以同源、无生成的模型目录GET验证当前草稿访问；四协议适配，不宣称推理/计费/Agent兼容已通过。自定义/本地必须显式目录地址；保存不联网，未验证保存仍可用。取消/Close等待真实请求收尾。
退出/禁用：返回保留本次草稿；Cancel不修改持久连接。保存中禁止重复写入与含糊退出，等真实回执；冲突先显示reload要求并保留草稿，普通错误允许修正/明确重试。
owner：UI负责字段、焦点、折叠与秘密草稿；共享product连接服务拥有验证/提交/秘密回执；存储组件由bootstrap装配，不由UI识别SQL错误或回退默认库。
验收：Preset展开可编辑真实端点；编辑空Key保存保留旧值、Clear才清除；读取/错误/日志无Key或秘密头；真实本地HTTP的401/超时/取消/来源变更和迟到结果回归已通过；外部账户/真实平台另验，不能由静态TestStatus代替。

绑定位置：
- `pages/连接/25 配置连接.html` → `SheetBody / Dialog / ProviderForm`
- `pages/连接/26 自定义连接.html` → `SheetBody / Dialog / ProviderForm`

标注：
- [behavior-contract] req_alva_provider_form — 共用Preset/Custom字段；编辑携ID/revision、空Key保留/显式Clear、秘密不回传。可选测试需求保留，本波未实现。 — `pages/连接/25 配置连接.html` → `SheetBody / Dialog / ProviderForm`
- [behavior-contract] req_alva_provider_form — 共用Preset/Custom字段；编辑携ID/revision、空Key保留/显式Clear、秘密不回传。可选测试需求保留，本波未实现。 — `pages/连接/26 自定义连接.html` → `SheetBody / Dialog / ProviderForm`

## req_alva_provider_models — 同表单模型获取、选择与手填

状态：ProviderModels的Idle/Loading/Ready/Empty/Unavailable/Error六态保留为自动目录需求；它们独立于来源，不做全排列。四协议自动目录获取/刷新/搜索/分页/显式选择与手填合并已由共享owner接线；不按供应商猜任意/models端点。
手填触发：在25/26可选模型区或23手动添加，填写准确模型ID、显示名与显式启用。空ID/重复的连接ID+准确模型ID不得添加；保留大小写/符号，不能按显示名合并。ImageCapable由用户明确声明，不按名字推断视觉、工具、Context或思考能力；无证据的能力为未知。
保存边界：未选模型仍可保存连接，不能自动启用全部模型。手填只改变本次草稿；明确Save一次提交连接与模型配置，真实成功后才能显示已保存。模型启用只表示后续选择可见，不证明连接已测试或该Agent实际兼容。
自动目录契约与实现：
- Fetch使用当前未保存草稿快照，显式发起；不先建Provider、不自动发生成测试。Loading保留手填入口，旧请求取消且晚到/过期结果不得覆盖较新草稿。
- Ready才显示真实目录、筛选、选择数；不自动全选。刷新保留手动ID、显式启用与覆盖，新条目不静默启用。
- Empty表示请求成功且空；Unavailable表示服务不提供目录；Error表示实际失败且保留草稿/选择并给重试。不能把缺少实现、失败和成功空目录混成暂无模型，也不能把手填列表冒称Fetch所得。
- 401/超时/取消由实际目录owner返回；失败或不支持仍允许手填和未验证保存。长目录滚动/分页、真实查询、异步过期结果须随服务实现验收。
退出/禁用：移出未存模型只改草稿；禁用已存模型走同一连接revision写入，失败保留原事实。禁用影响之后的目录选择与准入，不取消当前执行、不静默换到另一连接。
owner：UI维护手填/选择草稿，product连接目录服务验证稳定身份与revision，bootstrap将已启用连接/模型发布为实际可选路由；SDK不猜能力或实现统一/models端点。
验收：无目录服务可手填并保存；同连接重复ID拒绝，不同连接同ID分别保留；关闭/禁用不Stop当前轮；重开保持配置。六态/过期请求/401恢复、实际GET与取消收尾已有独立回归；CRUD回归不代替目录或外部账户验收。

2026-10-24会话参数范围：参数面板明确区分“当前会话”和“模型默认”；当前会话按真实账户/模型键独立revision保存/清除并核对完整回执。下一新Turn应用覆盖，运行/Waiting续轮仍使用原Attempt参数。Clear恢复已提交模型默认，失败/冲突保留本窗草稿并要求Reload；失效路由拒绝保存，不能猜engine能力。Context缺实际可信声明时保持未声明只读。本机Agent认证仍未接入。

绑定位置：
- `pages/连接/25 配置连接.html` → `SheetBody / Dialog / ProviderModels`
- `pages/连接/26 自定义连接.html` → `SheetBody / Dialog / ProviderModels`

标注：
- [behavior-contract] req_alva_provider_models — 共用六态自动目录目标；本波只接准确ID/显示名/启用的手填，Ready样例不冒称已获取目录。 — `pages/连接/25 配置连接.html` → `SheetBody / Dialog / ProviderModels`
- [behavior-contract] req_alva_provider_models — 共用六态自动目录目标；本波只接准确ID/显示名/启用的手填，Ready样例不冒称已获取目录。 — `pages/连接/26 自定义连接.html` → `SheetBody / Dialog / ProviderModels`

## req_alva_provider_save — 保存、编辑与退出的事实边界

状态：设计契约与本波真实CRUD接线范围；静态保存连线仍只回23，实际写入必须经过共享产品服务回执。
触发/前置：字段/原生协议/完整URL有效，当前读取成功且持有revision；测试与目录不是前置条件。创建由本窗生成稳定RequestID并捕获完整提交，存储在同一事务中写入实际连接、秘密与创建回执；编辑必须带现有ID与expected revision，更新同一连接。不能由表单标题或显示名推断模式。
状态迁移：管理只读→创建/编辑草稿→保存中→成功回管理并定位该连接；失败回同一草稿显示实际可恢复原因。保存中禁止重复提交；成功之前保留输入，成功之后使用回执ID/revision刷新，旧加载结果不覆盖较新提交。冲突保留草稿并要求显式Reload，不能自动用新revision重放修改。
创建结果不确定：保留原RequestID与草稿，用户可明确重试完全相同的创建，或显式ResolveCreation只读恢复原回执。仅提交字段/秘密完全相同且创建对象仍未被修改或删除时，重试返回同一ID和不可变脱敏Created回执；不同输入复用原RequestID明确冲突，不按名称找对象，也不另建替代连接。恢复同时返回当前同一身份：已被其他窗口改动时保留草稿并要求显式Reload；已删除时标记失踪且禁止重建，返回管理审阅。当前较新值或删除事实不能被原Created回执覆盖。
禁用/删除：23显式Enable/Disable持久化同一连接revision；模型显式启停随同一连接保存。禁用仅影响之后的选择和准入，不调用Stop、不改当前Attempt快照。删除必须二次确认并带同一ID/revision；取消零变更，失败保留现有连接，成功才移出目录。默认值/收藏若引用被禁用或删除对象，应显示不可用，不默默改到同名模型或其他账户。
退出：创建Back回来源保留本次草稿，创建Cancel/关闭明确放弃本次未保存表单回23；编辑Back/Cancel回23，保留既存配置，不创建新连接。设置分类导航保留本窗未保存编辑，保存期间阻止关闭；真正放弃由明确Cancel。确切来源切换与返回规则见req_alva_navigation_wizard。
事实分开：已保存、测试通过、模型已启用、Agent兼容和当前可调用分别显示。当前CRUD未附带测试连接、自动目录或本机账户认证能力，不显示已验证/已发现登录；这些仍是待完成的设计要求。42/43的完整模型参数/能力默认值编辑、39模型优先选择器/收藏/每行Agent参数浮层仍不能由简化手填列表宣称完成。
owner：未保存表单、创建RequestID与捕获提交只在本窗内存持有，不承诺关窗或重启后自动恢复；已提交连接与脱敏创建回执由存储持久化。窗口管理表单/确认/焦点与草稿；共享product.ConnectionsService（Read/Save/Create/ResolveCreation/Delete/SetEnabled/Targets/ResolveTarget/Close）和hub.ConnectionsStore及可选ConnectionCreationStore显式存储口拥有CRUD/CAS、创建恢复与秘密；bootstrap/实际执行owner负责路由准入、当前快照与资源收尾。UI不自行改SQL或Stop执行。
验收：双击只一次创建；编辑ID保持；冲突/存储失败草稿保留且无秘密；空Key保留与显式Clear各自正确；取消/删除取消不改既存；禁用后下一轮不能选择/准入、当前轮继续由原owner收尾；已保存配置重开持久，删除二次确认生效；丢失创建回执后明确恢复仍为原ID，保留恢复期间的后来编辑；另一窗口修改或删除后不能由旧创建重试覆盖或复活。
来源：apps/hub/connections.go、apps/hub/product/connections.go、apps/hub/sqlite/connections.go、apps/hub/desktop/workspace_connections_desktop.go及各自回归。

绑定位置：
- `pages/连接/25 配置连接.html` → `SheetBody / Dialog / DialogFooter`
- `pages/连接/26 自定义连接.html` → `SheetBody / Dialog / DialogFooter`
- `pages/设置/23 模型与连接.html` → `SettingsRegion / SettingsMain / Manager / Models / ConnectionTitle / EditConnection`
- `pages/设置/23 模型与连接.html` → `SettingsRegion / SettingsMain / Manager`

标注：
- [behavior-contract] req_alva_provider_save — 稳定RequestID创建；未知回执明确重试/恢复，Created脱敏且不可变；较新/删除要求显式审阅或Reload。本窗未存草稿与请求身份不承诺重启恢复。 — `pages/连接/25 配置连接.html` → `SheetBody / Dialog / DialogFooter`
- [behavior-contract] req_alva_provider_save — 稳定RequestID创建；未知回执明确重试/恢复，Created脱敏且不可变；较新/删除要求显式审阅或Reload。本窗未存草稿与请求身份不承诺重启恢复。 — `pages/连接/26 自定义连接.html` → `SheetBody / Dialog / DialogFooter`
- [behavior-contract] req_alva_provider_save — Save真实回执后回23；Edit携既存ID/revision不重建，失败保稿/冲突Reload；Cancel零写入、启停不Stop当前轮。 — `pages/设置/23 模型与连接.html` → `SettingsRegion / SettingsMain / Manager / Models / ConnectionTitle / EditConnection`
- [behavior-contract] req_alva_provider_save — 23连接/模型管理：Enable/Disable走revision持久化，仅影响后续选择；Delete二次确认，取消零变更，保存/失败/冲突按真实回执，不Stop当前Attempt。 — `pages/设置/23 模型与连接.html` → `SettingsRegion / SettingsMain / Manager`
