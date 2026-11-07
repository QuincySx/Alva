# INPUT: Alva semantic Tokens/components and the preserved workbench/provider/companion behavior contract
# OUTPUT: Component-composed product pages with top returns, compact preference rows, preset/custom Provider forms and optional inline models
# POS: Formal SmallPen product design; no runtime execution, account discovery or production configuration writes.
import argparse
import hashlib
import json
from pathlib import Path

from author import FOUNDATION, PRODUCT, ROOT, cli
from design_system import CANVAS, apply_nested, badge, block, box, button, definitions as component_definitions, empty, field, icon, icon_button, instance, native_context, notice, pending_instances, public_intent, ref, row, text

component_definitions()


def gap(name='Space'):return box(name,width='fill',height='fill')


def card(name,children,**extra):
    return box(name,children,**(block('card',width='fill',height='hug',fill=ref('color.surface'))|extra))


def title(name,label,description='',actions=(),size='subtitle',description_visible=True):
    return row(name,[box(name+'Copy',[text(name+'Title',label,size,strong=True),
        *([text(name+'Description',description,'caption','secondary',visible=description_visible)] if description else [])],width='fill',gap=ref('space.2')),*actions],align='start')


def page(name,module,children,layout='row',**extra):
    return dict(page=name,module=module,canvas=CANVAS,platform='desktop',width=ref('size.viewport.width'),height=ref('size.viewport.height'),
        layout=layout,fill=ref('color.canvas'),children=children)|extra


def app_sidebar(area='Work'):
    return instance('Sidebar','Sidebar',{'Area':area},width=ref('size.sidebar'),height='fill')


def page_header(label,description):
    return instance('Header','PageHeader',texts={'Title':label,'Description':description},width='fill',height=ref('size.header'))


def setting(name,label,description,on=False,action=None,last=False):
    return instance(name,'SettingRow',{'Control':'Action' if action else 'Switch'},texts={'Title':label,'Description':description},
        sets={'Copy.Description.visible':bool(description)}|({'Action.Label.text':action} if action else {'Control.props':{'State':'On' if on else 'Off'}})|({'stroke':ref('color.transparent')} if last else {}),width='fill',height='hug')


def preference(name,label,value):
    return instance(name,'SettingRow',{'Control':'Select'},texts={'Title':label,'Description':''},
        sets={'Copy.Description.visible':False,'Select.Label.text':value},width='fill',height='hug')


def segments(name,labels,selected=0):
    return row(name,[instance(name+str(i),'Segment',{'State':'Selected' if i==selected else 'Default'},texts={'Label':label},width='hug',height=32)
        for i,label in enumerate(labels)],width='hug',padding=4,gap=4,fill=ref('color.subtle'),radius=ref('radius.ctrl'))


def settings(name,category,label,description,content,actions=()):
    return page(name,'设置',[
        instance('SettingsNav','SettingsNav',{'Category':category},width=ref('size.navigation'),height='fill'),
        box('SettingsRegion',[box('SettingsMain',[title('PageTitle',label,description,actions,size='heading',description_visible=False),*content],width='fill',maxWidth=1120 if category=='Models' else 960,
            height='fill',**block('page'))],width='fill',height='fill',align='center'),
    ])


def resource(name,label,description,kind='File'):
    return instance(name,'ResourceRow',{'Kind':kind},texts={'Title':label,'Description':description},width='fill',height='hug')


def skill(name,label,description,state='Shared',enabled=True):
    return instance(name,'SkillRow',{'State':state},texts={'Title':label,'Description':description},sets={'Enabled.props':{'State':'On' if enabled else 'Off'}},width='fill',height='hug')


def message(name,author,body,model='GPT · ALVA',partner=False):
    return box(name,[instance(name+'Header','MessageHeader',{'Author':'Partner' if partner else 'Assistant'},texts={'Author':author,'Model':model},sets={'Avatar.Label.text':author[:1]} if partner else None,width='fill',height='hug'),
        text(name+'Body',body,'subtitle',lineHeight=ref('line.reading'))],width='fill',height='hug',gap=ref('space.3'))


def new_task():
    return page('21 新任务','工作台',[app_sidebar('New'),box('Main',[
        page_header('新任务','alva-agent · 项目'),
        box('Welcome',[box('WelcomeContent',[
            instance('WelcomeBadge','Icon',{'Name':'Sparkles'},sets={'fill':ref('color.accentSoft'),
                'radius':ref('radius.card'),'Glyph.color':ref('color.accent')},width=40,height=40),
            text('WelcomeTitle','想到什么，就从这里开始。','display',strong=True,margin=[0,0,4,0]),
            text('WelcomeDescription','描述目标、引用文件，或选择一位伙伴一起完成。','body','secondary',margin=[0,0,12,0]),
            instance('Composer','Composer',{'State':'Idle'},width='fill',height='hug'),
            row('Suggestions',[button('ReadProject','理解这个项目','Ghost',symbol='Folder'),button('Research','整理资料','Ghost',symbol='Book'),button('Review','检查代码','Ghost',symbol='Code')],width='hug'),
            row('Hints',[icon('Privacy','Lock',small=True),text('LocalHint','连接与权限由你选择，操作过程随时可查看。','caption','secondary')]),
        ],width='fill',maxWidth=760,gap=ref('space.3'))],width='fill',height='fill',padding=ref('space.6'),align='center',justify='center'),
    ],width='fill',height='fill',gap=0)])


def work_panel():
    return box('RightWorkspace',[
        instance('PanelHeader','PanelHeader',width='fill',height=52),
        box('FilePath',[text('DocumentPath','docs / 工作台方案.md','caption','secondary')],width='fill',padding=ref('space.4')),
        box('Document',[
            badge('DocumentType','文档预览'),text('DocumentTitle','Alva 工作台方案','heading',strong=True),
            text('DocumentLead','以对话为中心，让资料、执行与结果各有位置。','body','secondary',lineHeight=ref('line.reading')),
            text('DocH1','01  模型与连接','subtitle',strong=True),text('DocP1','先选择模型来源，再启用需要的模型。模型、执行 Agent 和参数在同一个选择器中调整。','body',lineHeight=ref('line.reading')),
            text('DocH2','02  长期伙伴','subtitle',strong=True),text('DocP2','伙伴保存职责、记忆和 Skills。换模型时保持身份，群聊使用自己的消息记录。','body',lineHeight=ref('line.reading')),
            text('DocH3','03  可见的执行','subtitle',strong=True),text('DocP3','工具、审批与子任务可以展开查看。关闭面板只影响视图。','body',lineHeight=ref('line.reading')),
            gap('DocumentSpace'),row('DocumentFooter',[icon('Saved','Check',small=True,color='success'),text('DocumentSaved','已保存到当前项目','caption','secondary')]),
        ],width='fill',height='fill',padding=ref('space.4'),gap=ref('space.4')),
    ],width=ref('size.panel'),height='fill',fill=ref('color.surface'),stroke={'color':ref('color.border'),'width':[0,0,0,1]},gap=0)


def conversation(waiting=False):
    content=[
        box('UserMessage',[text('UserText','把模型连接和伙伴群聊的流程整理成一份可实现的方案。','body')],width='fill',padding=ref('space.4'),fill=ref('color.subtle'),radius=ref('radius.card')),
        message('Assistant','Alva','我会先梳理连接与模型选择，再把伙伴、Skills 和群聊的边界理清楚。'),
        instance('ToolActivity','ToolActivity',{'State':'Collapsed'},texts={'Label':'已读取 3 份项目资料'},width='fill',height='hug'),
    ]
    if waiting:content.append(instance('Question','Question',{'State':'Pending'},width='fill',height='hug'))
    else:
        content.extend([resource('Artifact','工作台方案.md','文档 · 已保存到当前项目'),
            message('AssistantSummary','Alva','方案已整理好。你可以在右侧查看，接下来继续细化模型配置和伙伴能力。'),
            row('MessageActions',[icon_button('CopyMessage','Copy'),icon_button('ShareMessage','External'),icon_button('MoreMessage','More'),text('Completion','已完成 · 24 秒','caption','secondary')])])
    if waiting:content=[content[0],content[-1]]
    content.extend([gap('MessageSpace'),instance('Composer','Composer',{'State':'Waiting' if waiting else 'Idle'},
        sets={'Prompt.text':'回答上方问题后继续'} if waiting else None,width='fill',height='hug'),
        row('ThreadFooter',[text('ThreadState','等待你的回答' if waiting else '当前项目 · 可继续补充','caption','secondary',visible=False),badge('Context','Context  18%')],justify='end')])
    main=box('Main',[page_header('整理桌面工作台','alva-agent · 当前会话'),row('Body',[
        box('Conversation',content,width='fill',height='fill',padding=ref('space.6'),gap=ref('space.4')),
        *([] if waiting else [work_panel()])],height='fill',gap=0)],width='fill',height='fill',gap=0)
    return page('40 等待用户回答' if waiting else '22 对话与面板','工作台',[app_sidebar(),main])


def models():
    def c(name,title,description,state='Default',label='O'):
        return instance(name,'ConnectionRow',{'State':state},texts={'Title':title,'Description':description},sets={'Avatar.Label.text':label},width='fill',height='hug')
    return settings('23 模型与连接','Models','模型与连接','管理模型来源、可用目录与默认路由。',[
        row('Manager',[
            box('Connections',[text('ConnectionsLabel','我的连接','caption','secondary'),c('OpenAI','OpenAI · 主连接','API Key','Selected'),
                c('Anthropic','Anthropic · 工作','API Key',label='A'),c('Gemini','Google Gemini','尚未连接','Disconnected','G'),
                c('Local','本地服务','尚未连接','Disconnected','L'),
                gap('ConnectionSpace'),button('LocalAccount','连接本机账户',symbol='Key',width='fill'),button('AddProvider','添加供应商',symbol='Plus',width='fill',visible=False)],
                width=ref('size.navigation'),height='fill',fill=ref('color.surface'),**block('navigation')),
            box('Models',[
                title('ConnectionTitle','OpenAI · 主连接','API Key · ALVA',[button('EditConnection','连接设置')]),
                row('CatalogHeading',[badge('Connected','连接就绪','Success'),text('EnabledCount','2 个模型已启用','caption','secondary'),icon_button('Refresh','Refresh'),button('AddModel','添加模型',symbol='Plus')]),
                instance('SearchModels','Input',{'Kind':'Search','State':'Default'},texts={'Value':'搜索模型…'},width='fill',height=ref('control.height')),
                segments('ModelKinds',['对话','图像生成']),
                instance('GPT','ModelRow',{'Mode':'Manage','State':'Selected'},texts={'Title':'GPT','Description':'目录模型 · 视觉输入 · 工具调用'},width='fill',height='hug'),
                instance('GPTMini','ModelRow',{'Mode':'Manage','State':'Default'},texts={'Title':'GPT Mini','Description':'目录模型 · 文本 · 工具调用'},width='fill',height='hug'),
                instance('Custom','ModelRow',{'Mode':'Manage','State':'Unavailable'},texts={'Title':'我的自定义模型','Description':'手动添加 · 能力待确认'},width='fill',height='hug'),
                notice('Availability','启用模型后即可选择','是否可调用仍取决于账户、连接和当前 Agent。',visible=False),
                gap('ModelSpace'),text('CatalogHint','刷新保留你的选择与手动模型。没有目录时，也能按准确 ID 添加。','caption','secondary',visible=False),
            ],width='fill',height='fill',padding=ref('space.4'),gap=ref('space.4'),fill=ref('color.surface')),
        ],height='fill',gap=ref('space.3'),radius=ref('radius.card')),
        row('DefaultSelection',[text('DefaultLabel','应用默认模型与 Agent','body'),button('DefaultModel','GPT · ALVA',symbol='Chip')]),
    ],[button('AddConnection','添加供应商','Primary',symbol='Plus')])


def dialog(name,module,title_label,description,body,footer,width=720,back=None):
    return page(name,module,[box('Dialog',[
        *([back] if back else []),
        instance('DialogHeader','DialogHeader',texts={'Title':title_label,'Description':description},sets={'Copy.Description.visible':bool(description)},width='fill',height='hug'),
        *body,box('DialogSpace',width='fill',height=0,visible=False),
        instance('DialogFooter','DialogFooter',texts={'Scope':footer[0],'Save.Label':footer[1]},sets={'Scope.visible':bool(footer[0])},width='fill',height='hug'),
    ],width='fill',maxWidth=width,height='hug',fill=ref('color.surface'),stroke=ref('color.border'),
        shadow=ref('shadow.popup'),**block('dialog'))],layout='column',align='center',justify='center',padding=ref('space.6'),fill=ref('color.scrim'))


def provider_setup(source):
    if source=='Picker':
        def choice(key,label,symbol,description=''):
            return instance('Choose'+key,'MenuItem',{'State':'Default'},texts={'Label':label,'Description':description,'Shortcut':''},
                sets={'Icon.props':{'Name':symbol},'Copy.Description.visible':bool(description),'Shortcut.visible':False},width='fill',height='hug')
        groups=[box('APISources',[text('APITitle','API 服务','caption','secondary'),
            choice('OpenAI','OpenAI','Chip'),choice('Anthropic','Anthropic','Chip'),choice('Gemini','Google Gemini','Sparkles')],width='fill',height='hug',gap=ref('space.1')),
            box('OtherSources',[text('OtherTitle','其他来源','caption','secondary'),
                choice('Custom','自定义连接','Globe','兼容 API 或自有服务'),choice('Local','本地服务','Terminal','连接已经启动的 HTTP 服务'),
                choice('Account','本机 Agent 账户','Key','由你选择并确认，只用于对应 Agent')],width='fill',height='hug',gap=ref('space.1'))]
        d=dialog('24 选择供应商','连接','添加供应商','',[
            instance('ProviderSearch','Input',{'Kind':'Search','State':'Default'},texts={'Value':'搜索服务…'},width='fill',height=ref('control.height')),*groups],
            ('',''),width=720,back=button('WizardBack','返回模型与连接','Ghost',symbol='ChevronLeft'))
        d['children'][0]['children']=[n for n in d['children'][0]['children'] if n['name']!='DialogFooter']
    else:
        preset=source=='Preset'
        d=dialog('25 配置连接' if preset else '26 自定义连接','连接','OpenAI' if preset else '自定义连接','api.openai.com' if preset else '',[
            instance('ProviderForm','ProviderForm',{'Source':source},width='fill',height='hug'),
            instance('ProviderModels','ProviderModels',{'State':'Idle' if preset else 'Ready'},width='fill',height='hug')],
            ('','保存连接'),width=800,back=button('WizardBack','返回选择供应商','Ghost',symbol='ChevronLeft'))
    d['children'][0]['gap']=ref('space.4')
    return d


def roster(group=False):
    return box('Roster',[
        row('RosterTitle',[text('RosterHeading','伙伴','subtitle',strong=True),icon_button('NewCompanion','Plus')]),
        instance('SearchPartners','Input',{'Kind':'Search','State':'Default'},texts={'Value':'搜索伙伴和群聊…'},width='fill',height=ref('control.height')),
        instance('Research','CompanionRow',{'State':'Default' if group else 'Selected'},width='fill',height='hug'),
        instance('Reviewer','CompanionRow',{'State':'Default'},texts={'Title':'审查伙伴','Description':'代码审查 · 1 项 Skill'},sets={'Avatar.Label.text':'审'},width='fill',height='hug'),
        instance('Group','CompanionRow',{'State':'Selected' if group else 'Default'},texts={'Title':'产品讨论组','Description':'你 + 2 位伙伴'},sets={'Avatar.Label.text':'群'},width='fill',height='hug'),
        button('NewGroup','新建群聊',symbol='Plus',width='fill'),gap('RosterSpace'),text('RosterHint','长期伙伴，持续积累。','caption','secondary',visible=False),
    ],width=ref('size.navigation'),height='fill',fill=ref('color.surface'),stroke={'color':ref('color.border'),'width':[0,1,0,0]},**block('navigation'))


def companion_chat(group=False):
    label='产品讨论组' if group else '研究伙伴'
    content=[
        box('UserMessage',[text('UserText','@研究伙伴 @审查伙伴 一起看看模型连接流程，先说各自的建议。' if group else '帮我整理模型供应商的连接方案，按约定的格式。')],
            width='fill',padding=ref('space.4'),fill=ref('color.subtle'),radius=ref('radius.card')),
        message('ResearchMessage','研究伙伴','我先核对模型来源与接入方式，再按资料研究的格式整理依据。','GPT · ALVA',True),
        badge('UsedSkills','资料研究 · 文档整理','Accent'),
    ]
    if group:content.extend([message('ReviewMessage','审查伙伴','我会检查错误与保存边界。连接失败时保留草稿，凭据不会进入群消息。','Sonnet · Claude',True),
        notice('RoundDone','本轮结束，等待下一条消息','','Success')])
    else:content.append(resource('ResearchResult','模型连接方案.md','伙伴工作区 · 已保存'))
    content.extend([gap('ChatSpace'),instance('Composer','Composer',{'State':'Idle'},sets={'Prompt.text':'@ 选择伙伴，或发一条消息…' if group else '和研究伙伴说说你要做的事…',
        'Controls.Model.Label.text':'仅 @ 回应' if group else 'GPT · ALVA'},width='fill',height='hug'),
        text('MemoryScope','群聊独立记录，不自动带入私聊与私人记忆。' if group else '连续对话 · 伙伴身份、记忆和 Skills 保持。','caption','secondary',visible=False)])
    return page('29 伙伴群聊' if group else '27 伙伴私聊','伙伴',[app_sidebar('Partners'),roster(group),box('Main',[
        row('PartnerHeader',[box('HeaderCopy',[text('PartnerTitle',label,'subtitle',strong=True),text('PartnerDescription','你 · 研究伙伴 · 审查伙伴' if group else '资料研究与整理 · 2 项 Skills','caption','secondary',visible=group)],width='fill',gap=4),
            button('AddToGroup','继续讨论' if group else '添加到群','Ghost',symbol='Users'),button('Settings','群设置' if group else '伙伴设置',symbol='Sliders')],height=ref('size.header'),padding=[0,ref('space.6')],fill=ref('color.surface'),stroke={'color':ref('color.border'),'width':[0,0,1,0]}),
        *([row('GroupPolicy',[badge('GroupMode','仅 @ 回应','Accent',visible=False),text('GroupLimit','依成员顺序 · 每位 1 次 · 最多 3 次回复','caption','secondary')],padding=[8,24])] if group else []),
        box('Chat',content,width='fill',height='fill',padding=ref('space.6'),gap=ref('space.4')),
    ],width='fill',height='fill',gap=0)])


def companion_profile():
    return dialog('28 伙伴资料','伙伴','研究伙伴','',[
        segments('ProfileTabs',['身份与能力','记忆','历史']),
        row('ProfileColumns',[
            box('Identity',[
                field('Name','名字','研究伙伴'),field('Model','模型与 Agent','GPT · ALVA',kind='Select'),
                text('RoleLabel','职责与做事方式','body',strong=True),instance('Role','TextArea',{'State':'Default'},texts={'Value':'核对来源，整理依据；不确定时说明，不把推测写成事实。'},width='fill',height=96),
                field('Permission','执行权限','每次询问',kind='Select'),setting('Memory','长期记忆','',True),
            ],width='fill',height='hug',gap=ref('space.4')),
            box('Capabilities',[
                row('SkillTitle',[text('SkillsTitle','Skills','subtitle',strong=True),icon_button('AddSkill','Plus')]),
                skill('ResearchSkill','资料研究','共享库 · 固定 v1'),skill('DocSkill','文档整理','仅此伙伴 · v2','Owned'),
                field('Toolsets','工具与 MCP','已选工具集',kind='Select'),
                notice('Learning','新 Skill 保存前确认','能力说明不会授予权限；群聊按参与范围加载。'),
            ],width='fill',height='hug',gap=ref('space.4')),
        ],height='hug',align='start',gap=ref('space.6')),
    ],('此伙伴 · 下一轮生效','保存伙伴'),width=800)


def create_group():
    def member(name,label,description):
        return row(name,[instance(name+'Check','Checkbox',{'State':'On'},sets={'Label.visible':False},width=18,height=20),
            box(name+'Copy',[text(name+'Title',label,'body',strong=True),text(name+'Description',description,'caption','secondary')],width='fill',gap=4)],
            padding=ref('space.3'),fill=ref('color.accentSoft'),stroke=ref('color.accent'),radius=ref('radius.ctrl'))
    return dialog('30 新建群聊','伙伴','新建群聊','',[
        field('Name','群名称','产品讨论组'),text('MembersTitle','选择伙伴','body',strong=True),
        member('ResearchMember','研究伙伴','资料研究 · 文档整理'),
        member('ReviewMember','审查伙伴','代码审查'),
        row('Rules',[field('ReplyMode','谁回应','仅 @ 的伙伴',kind='Select'),field('Order','发言顺序','按成员顺序',kind='Select')]),
        row('Limits',[field('Budget','每轮回复上限','3 次'),field('Workspace','共享工作区','未选择',kind='Select')]),
        notice('Privacy','只共享本群已授权内容','伙伴私聊和私人记忆不会自动带入群聊。'),
    ],('','创建群聊'))


def general():
    return settings('31 通用','General','通用','外观、交互与窗口偏好。',[
        card('Appearance',[title('AppearanceTitle','外观'),box('PreferenceRows',[
            preference('Theme','颜色主题','跟随系统'),preference('Density','信息密度','舒适'),
            preference('Language','语言','简体中文'),preference('FontSize','正文大小','14 px'),
            setting('Motion','减弱动效','',False,last=True)],width='fill',height='hug',gap=0)]),
        card('Interaction',[title('InteractionTitle','交互与通知',actions=[button('Shortcuts','快捷键',symbol='Code')]),box('InteractionRows',[
            setting('Notification','任务完成通知','完成、失败或需要你处理时提醒。',True),
            setting('Sound','提示音','仅用于重要任务状态。',False),setting('WindowLayout','记住窗口布局','恢复面板与位置，不自动执行命令。',True,last=True),
        ],width='fill',height='hug',gap=0)]),
        notice('GeneralScope','偏好可以随时调整','更改主题不会影响当前任务或执行权限。',visible=False),
    ])


def plugins():
    return settings('32 插件与工具','Plugins','插件与工具','安装、启用与授权分别管理。',[
        row('PluginToolbar',[instance('PluginSearch','Input',{'Kind':'Search','State':'Default'},texts={'Value':'搜索已安装插件…'},width='fill',height=ref('control.height')),segments('PluginFilter',['全部','已启用','需处理'])]),
        card('PluginList',[skill('FilesPlugin','文件与目录','内置工具 · 当前项目 · 已启用'),skill('ShellPlugin','命令执行','内置工具 · 每次询问'),
            skill('McpPlugin','MCP 连接','2 个连接 · 工具按需发现'),skill('ScriptPlugin','脚本插件','JS / WASM · 按宿主能力装配','Owned')],gap=0),
        card('McpConnections',[title('McpTitle','MCP 连接','目录与正文按需加载。',[button('AddMcp','添加连接',symbol='Plus')],description_visible=False),
            resource('LocalMcp','项目文档服务','当前项目 · 连接就绪','Link'),resource('PendingMcp','开发工具服务','尚未连接','Link')]),
        notice('PluginScope','启用不等于授权','工具执行仍经过现有权限检查；不同平台可用能力分别标明。',visible=False),
    ],[button('InstallPlugin','安装插件','Primary',symbol='Plus')])


def skills():
    return settings('33 规则与 Skills','Skills','规则与 Skills','查看来源、作用域和实际生效的版本。',[
        row('RuleToolbar',[instance('RuleSearch','Input',{'Kind':'Search','State':'Default'},texts={'Value':'搜索规则或 Skill…'},width='fill',height=ref('control.height')),segments('RuleFilter',['Skills','项目规则','兼容来源'])]),
        card('SkillList',[skill('Research','资料研究','共享库 · 固定 v1'),skill('Review','代码审查','共享库 · 固定 v1'),skill('Docs','文档整理','研究伙伴自有 · v2','Owned'),
            skill('Unavailable','自定义工作流','来源缺失 · 当前不会挂载','Unavailable',False)],gap=0),
        card('RuleSources',[title('RuleSourceTitle','规则来源'),resource('NativeRules','ALVA.md','项目规则 · 显式加载'),
            setting('Compatibility','标准格式兼容','按需开启；相同来源与身份去重，不按名称合并。',False)]),
        notice('SkillGrant','Skill 说明不授予权限','新增或改写共享 Skill 时，明确保存位置与影响范围。',visible=False),
    ],[button('AddSkill','添加 Skill','Primary',symbol='Plus')])


def storage():
    return settings('34 数据与存储','Storage','数据与存储','数据归属与存储组件独立选择。',[
        card('StorageBackend',[title('BackendTitle','当前存储'),row('BackendFields',[field('StorageProvider','存储组件','本地持久存储',kind='Select'),field('StorageScope','数据范围','当前用户',kind='Select')]),
            notice('StoragePort','存储通过统一能力接入','可由产品装配本地、内存或远端实现；未安装的组件不显示为可用。',visible=False),
            setting('StorageDetails','连接与健康状态','查看组件声明、数据位置与可用性。',action='查看详情')]),
        card('DataManagement',[title('DataTitle','数据管理'),setting('Export','导出数据','显式选择会话、伙伴或共享内容。',action='导出'),
            setting('Retention','保留策略','原始记录、附件与派生文件分别设置。',action='配置'),
            setting('Cache','派生缓存','只清理可重建内容，保留原始文件。',action='查看')]),
        notice('StorageChange','切换存储前先确认数据范围','切换连接和迁移历史分开操作，不自动搬运或删除原数据。','Warning'),
    ])


def security():
    return settings('35 权限与安全','Security','权限与安全','权限以真实操作和执行范围为准。',[
        card('PermissionModes',[title('PermissionTitle','默认审批方式'),
            instance('Ask','QuestionOption',{'State':'Selected'},texts={'Copy.Title':'每次询问','Copy.Description':'需要权限时暂停，等待你明确决定。'},width='fill',height='hug'),
            instance('Edits','QuestionOption',{'State':'Default'},texts={'Copy.Title':'允许常规编辑','Copy.Description':'高风险操作仍需单独确认。'},width='fill',height='hug'),
            instance('Plan','QuestionOption',{'State':'Default'},texts={'Copy.Title':'计划模式','Copy.Description':'先调查与规划，执行变更前再切换。'},width='fill',height='hug')]),
        card('Authorizations',[title('AuthorizationTitle','已授予的范围'),resource('WorkspaceGrant','alva-agent','当前项目 · 文件访问','Directory'),
            setting('RememberedRules','已记住的审批规则','可查看与撤销；不扩大其他会话权限。',action='管理规则')]),
        notice('QuestionVsApproval','提问与审批是两种交互','选项回答不会自动批准命令、文件访问或外部操作。',visible=False),
    ])


def gateway():
    return settings('36 Gateway','Gateway','Gateway','管理显式启动的模型协议入口。',[
        card('GatewayState',[title('GatewayTitle','本地网关','服务尚未启动。',[badge('GatewayStatus','已停止')],description_visible=False),
            row('GatewayAddress',[field('Host','监听地址','127.0.0.1'),field('Port','端口','8080')]),
            row('GatewayAuth',[field('GatewayKey','访问密钥','尚未设置',kind='Password'),field('GatewayRoute','默认路由','未选择',kind='Select')]),
            row('GatewayActions',[button('StartGateway','启动服务','Primary','Disabled',symbol='Play'),button('GatewayLogs','查看日志',symbol='Activity')]),
            notice('GatewayScope','设置访问密钥和默认路由后可启动。','')]),
        card('GatewayRoutes',[title('GatewayRoutesTitle','协议与路由'),resource('ChatRoute','Chat Completions / Responses','按已配置路由转发','Link'),
            resource('NativeRoute','Anthropic / Gemini','能力与格式以当前转换实现为准','Link')]),
    ])


def developer(evaluation=False):
    label='模型评估' if evaluation else '集成测试'
    return settings('38 开发者评估' if evaluation else '37 开发者测试','Developer',label,
        '按任务集比较表现、成本与稳定性。' if evaluation else '验证接入、工具调用与交互协议是否正确。',[
        segments('DeveloperMode',['集成测试','模型评估'],1 if evaluation else 0),
        card('DeveloperRun',[title('RunTitle','新建评估' if evaluation else '运行检查'),
            row('TargetFields',[field('Route','模型路由','选择测试专用路由',kind='Select'),field('Suite','任务集' if evaluation else '检查组','工具使用与提问' if evaluation else '协议与工具契约',kind='Select')]),
            row('RunFields',[field('Budget','费用预算','未设置'),field('Trials','重复次数','3' if evaluation else '1')]),
            row('RunActions',[button('Run','开始评估' if evaluation else '开始测试','Primary','Disabled',symbol='Play'),text('RunHint','选择路由并确认预算后运行。','caption','secondary')])]),
        card('DeveloperResults',[title('ResultsTitle','评估记录' if evaluation else '检查结果'),empty('NoResults','还没有评估记录' if evaluation else '还没有测试结果',
            '','Chart' if evaluation else 'Code')]),
        notice('DeveloperBoundary','评估与测试分别记录','开发者操作不暗中使用订阅登录态，也不改变日常对话路由。',visible=False),
    ])


def model_picker():
    options=card('ModelOptions',[
        row('ModelOptionsHeader',[text('OptionsName','GPT','subtitle',strong=True),icon_button('FavoriteModel','Star')]),
        text('OptionsSource','OpenAI · 主连接 · 对话','caption','secondary'),
        field('OptionsAgent','Agent','ALVA',kind='Select'),
        text('EffortTitle','思考程度','body',strong=True),segments('Effort',['默认','低','中','高']),
        field('Context','Context 上限 · token','128000'),setting('Vision','视觉输入','',True),
        button('OpenModelSettings','模型设置','Ghost',symbol='Sliders',width='fill'),
        text('OptionScope','当前会话 · 下一轮生效','caption','secondary'),
    ],width=300,height='hug')
    picker=card('ModelPicker',[
        row('SearchRow',[instance('ModelSearch','Input',{'Kind':'Search','State':'Focus'},texts={'Value':'搜索模型…'},width='fill',height=ref('control.height')),icon_button('ClosePicker','Close')]),
        row('PickerBody',[
            box('ProviderFilters',[icon_button('Favorites','Star'),icon_button('AllProviders','Chip'),icon_button('OpenAIProvider','Globe'),icon_button('AnthropicProvider','Brain'),icon_button('LocalProvider','Terminal')],width=40,height='fill',gap=ref('space.3')),
            box('Models',[instance('Model'+str(i),'ModelRow',{'State':'Hover' if i==0 else 'Selected' if i==2 else 'Default'},texts={'Title':label,'Description':source},
                width='fill',height='hug') for i,(label,source) in enumerate([('GPT','OpenAI · 主连接 · 视觉输入'),('GPT Mini','OpenAI · 主连接 · 文本'),('Sonnet','Anthropic · 工作连接'),('Opus','Anthropic · 工作连接'),('Gemini Pro','Google · 主连接')])],width='fill',height='hug',gap=4),
        ],gap=ref('space.3'),height='hug',align='start'),
        row('PickerFooter',[button('ManageModels','配置模型','Ghost',symbol='Plus'),text('PickerHint','点击模型行直接选用','caption','secondary',visible=False)]),
    ],width='fill',maxWidth=640,height='hug')
    return page('39 模型选择器','交互',[row('ModelSelectionSurface',[options,picker],width='fill',maxWidth=960,height='hug',align='start',gap=ref('space.3'))],
        layout='column',width=ref('size.viewport.width'),height=760,padding=32,align='center',justify='center',fill=ref('color.scrim'))


def composer_menu():
    return page('41 输入区添加菜单','交互',[box('AnchoredMenu',[
        instance('AddMenu','ComposerMenu',{'View':'Add'},width=352,height='hug'),
        instance('Composer','Composer',{'State':'Idle'},width='fill',height='hug'),
    ],width='fill',maxWidth=720,height='hug',gap=6)],layout='column',height=760,padding=32,align='center',justify='center',fill=ref('color.canvas'))


def model_configuration():
    return dialog('42 模型设置','连接','模型设置','OpenAI · 主连接',[
        instance('ModelForm','ModelForm',{'Mode':'Edit'},width='fill',height='hug'),
    ],('此连接的模型默认值 · 下一轮生效','保存设置'))


def manual_model():
    return dialog('43 手动添加模型','连接','手动添加模型','添加到 OpenAI · 主连接',[
        instance('ModelForm','ModelForm',{'Mode':'Add'},width='fill',height='hug'),
    ],('此连接 · 不改变已有模型','添加到连接'))


def definitions():
    from reading_pilot import decorate
    return [decorate(d) for d in [new_task(),conversation(),models(),*[provider_setup(source) for source in ('Picker','Preset','Custom')],companion_chat(),companion_profile(),companion_chat(True),create_group(),
        general(),plugins(),skills(),storage(),security(),gateway(),developer(),developer(True),model_picker(),conversation(True),composer_menu(),model_configuration(),manual_model()]
    ]


def navigation_links():
    ds=definitions();names={d['page'][:2]:d['module']+' / '+d['page'] for d in ds}
    links=[]
    for d in ds:
        source=d['module']+' / '+d['page']
        if d['page'][:2] in ['21','22','27','29','40']:
            links.extend([(source,'Sidebar / NewTask',names['21']),(source,'Sidebar / CurrentThread',names['22']),
                (source,'Sidebar / Partners',names['27']),(source,'Sidebar / OpenSettings',names['31'])])
        if d['module']=='设置':
            for category,key in [('General','31'),('Models','23'),('Plugins','32'),('Skills','33'),('Storage','34'),('Security','35'),('Gateway','36'),('Developer','37')]:
                links.append((source,'SettingsNav / '+category,names[key]))
            links.append((source,'SettingsNav / BackToWork',names['22']))
    links.extend([
        # Attach/permission/hover reference shared surfaces through native notes;
        # do not regenerate per-page overlay boards or make an empty editor submit.
        (names['21'],'Composer / Model',names['39']),(names['22'],'Composer / Model',names['39']),
        (names['23'],'AddConnection',names['24']),(names['23'],'AddProvider',names['24']),(names['23'],'EditConnection',names['25']),
        (names['23'],'AddModel',names['43']),(names['43'],'DialogFooter / Save',names['23']),
        (names['24'],'ChooseOpenAI',names['25']),(names['24'],'ChooseCustom',names['26']),(names['24'],'WizardBack',names['23']),
        (names['25'],'WizardBack',names['24']),(names['26'],'WizardBack',names['24']),
        (names['25'],'DialogFooter / Save',names['23']),(names['26'],'DialogFooter / Save',names['23']),
        (names['27'],'Settings',names['28']),(names['27'],'AddToGroup',names['30']),(names['27'],'Roster / NewGroup',names['30']),
        (names['27'],'Roster / Group',names['29']),(names['28'],'DialogFooter / Save',names['27']),(names['30'],'DialogFooter / Save',names['29']),
        (names['29'],'Roster / Research',names['27']),(names['39'],'ManageModels',names['23']),(names['39'],'OpenModelSettings',names['42']),
        (names['39'],'ClosePicker',names['21']),(names['37'],'DeveloperMode1',names['38']),(names['38'],'DeveloperMode0',names['37']),
        (names['40'],'Question / Submit',names['22']),(names['42'],'DialogFooter / Save',names['23']),
    ])
    for key in ['24','25','26','28','30','42','43']:
        target=names['27'] if key in ['28','30'] else names['23']
        links.append((names[key],'DialogHeader / Close',target))
        if key!='24':links.append((names[key],'DialogFooter / Cancel',target))
    return links


def connect(projection=None):
    if projection:return connect_batch(json.loads(Path(projection).read_text()))
    snapshot=cli('flow','list',PRODUCT,label='ds2-read-product-links')
    if snapshot.get('resultFile'):snapshot=json.loads(Path(snapshot['resultFile']['path']).read_text())
    existing=snapshot.get('text','')
    links=navigation_links()
    for source,element,target in links:
        if f'{source} (desktop) / {element} —click→ {target}' in existing:continue
        ident=hashlib.sha256((source+' / '+element+' -> '+target).encode()).hexdigest()[:20]
        cli('flow','link',PRODUCT,'--from',source+' / '+element,'--to',target,'--on','click','--batch-id','alva-ds2-nav-'+ident,label='ds2-product-link')


def connect_batch(projection):
    # Read actual native IDs from a revision-matched public Web workspace projection.
    # Writes still go through the CLI's guarded, atomic typed operations.
    current=cli('project','show',PRODUCT,label='ds2-navigation-base')
    if projection['revision']!=current['revision']:raise ValueError('Navigation projection is stale; read the Web workspace again.')
    screens={s['name']:s for p in (PRODUCT/'screens').glob('*.json') for s in [json.loads(p.read_text())]}
    sets,_,_=native_context();updates={}
    for source,element,target in navigation_links():
        screen=screens[source];presentation=next(p for p in screen['presentations'] if p.get('platform')=='desktop')
        tree=presentation;owner=None;prefix=[];parts=element.split(' / ')
        for index,name in enumerate(parts):
            matches=[n for n in tree['nodes'].values() if n['name']==name]
            if len(matches)!=1:raise ValueError((source,element,name,len(matches)))
            node=matches[0]
            if index<len(parts)-1 and node['type']=='INSTANCE':
                if owner is None:owner=node
                else:prefix.append(node['id'])
                component=sets[node['instance']['component']['assetId']]
                tree=next(v for v in component['variants'] if v['selection']==node['instance']['variant'])
        destination=screens[target];dest_pres=next(p for p in destination['presentations'] if p.get('platform')=='desktop')
        dest_id=projection['runtime']['nodes'][destination['id']][dest_pres['id']][dest_pres['rootId']]
        interaction={'action-type':'navigate','destination':dest_id,'event-type':'click'}
        override='__'.join([*prefix,node['id']])+':interactions' if owner else None
        stored=owner or node;key=(screen['id'],presentation['id'],stored['id'],override)
        if key not in updates:
            before=stored['instance'].get('overrides',{}).get(override,[]) if owner else stored.get('interactions',[])
            updates[key]={'before':before,'after':list(before)}
        if interaction not in updates[key]['after']:updates[key]['after'].append(interaction)
    operations=[]
    for (screen_id,presentation_id,node_id,override),value in updates.items():
        if value['before']==value['after']:continue
        operation={'screenId':screen_id,'presentationId':presentation_id,'nodeId':node_id}
        if override:operation.update(type='set-instance-override',overridePath=override,value=value['after'])
        else:operation.update(type='update-presentation-node',changes={'interactions':value['after']})
        operations.append(operation)
    if not operations:
        print('Navigation already matches; no write.',flush=True);return current['revision']
    digest=hashlib.sha256(json.dumps(operations,sort_keys=True).encode()).hexdigest()[:16]
    batch={'baseRevision':current['revision'],'batchId':'alva-ds2-navigation-'+current['revision'][:12]+'-'+digest,'operations':operations}
    path=ROOT/'source'/'intents'/('ds2-navigation-'+digest+'.json');path.write_text(json.dumps(batch,ensure_ascii=False,indent=2)+'\n')
    return cli('advanced','apply',PRODUCT,'--batch',path,label='ds2-navigation-batch')['revision']


def write(only=None):
    revision=cli('project','show',PRODUCT,label='ds2-product-base')['revision']
    for d in definitions():
        if only is None or d['page'] in only:
            receipt=cli('page','draw',PRODUCT,intent=public_intent(d),label='ds2-product-'+d['page'][:2]+'-'+revision[:12]);revision=receipt['revision']
            revision=finish_page(d,revision)
            from reading_pilot import finish
            revision=finish(d,revision)


def finish_page(definition,revision):
    sets,tokens,package=native_context();operations=[]
    title=definition['module']+' / '+definition['page']
    screen=next(json.loads(p.read_text()) for p in (PRODUCT/'screens').glob('*.json') if json.loads(p.read_text())['name']==title)
    for presentation in screen['presentations']:
        for name,updates in pending_instances(definition):
            node=next(n for n in presentation['nodes'].values() if n['name']==name)
            before=dict(node.get('instance',{}).get('overrides',{}))
            apply_nested(node,updates,sets,tokens,package)
            for override_path,value in node['instance'].get('overrides',{}).items():
                if override_path not in before or value!=before[override_path]:
                    operations.append({'type':'set-instance-override','screenId':screen['id'],'presentationId':presentation['id'],
                        'nodeId':node['id'],'overridePath':override_path,'value':value})
    if not operations:return revision
    digest=hashlib.sha256(json.dumps(operations,sort_keys=True).encode()).hexdigest()[:16]
    batch={'baseRevision':revision,'batchId':'alva-ds2-page-nested-'+revision[:12]+'-'+digest,'operations':operations}
    path=ROOT/'source'/'intents'/('ds2-page-nested-'+definition['page'][:2]+'.json');path.write_text(json.dumps(batch,ensure_ascii=False,indent=2)+'\n')
    return cli('advanced','apply',PRODUCT,'--batch',path,label='ds2-page-nested')['revision']


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');parser.add_argument('--links',action='store_true');parser.add_argument('--since');parser.add_argument('--only',nargs='*');parser.add_argument('--projection',help='Revision-matched public Web workspace JSON for atomic navigation writes')
    args=parser.parse_args()
    if args.write or args.links:
        if not args.since:parser.error('Writing requires a reviewed Product --since revision')
        result=cli('changes',PRODUCT,'--since',args.since,label='ds2-read-product-changes')
        if result.get('changeCount'):raise SystemExit('Review package changes before continuing.')
    if args.write:write(args.only)
    elif args.links:connect(args.projection)
    else:print('Prepared',len(definitions()),'formal product page definitions.')
