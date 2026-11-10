# INPUT: Preserved wireframes, current Alva product contracts and the original interface icon catalog
# OUTPUT: Namespaced semantic Tokens and a reusable Alva control/domain component library
# POS: The Python model of the design system; next/build_next.py turns it into alva.smallpen's tokens.css and components/ (nothing here writes a package).
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

CANVAS="01 产品界面"
BASES={}


def ref(path):return "{ui."+path+"}"


def token_definitions():
    out=[]
    def add(name,kind,value,dark=None,values=None,description=None):
        overrides={key:other for key,other in (values or {}).items() if other!=value}
        if dark is not None and dark!=value:overrides['Appearance/Dark']=dark
        out.append(dict(name='ui.'+name,type=kind,value=value,**({'values':overrides} if overrides else {}),
            **({'description':description} if description else {})))
    pairs={
        'canvas':('#F3F4F7','#14161B'),'surface':('#FFFFFF','#1C1F26'),
        'subtle':('#F5F6F8','#252932'),
        'hover':('#E9EBF0','#2C313C'),'sidebar':('#F3F4F7','#181B21'),
        'text':('#272A34','#EAECF3'),'secondary':('#626B7A','#A6ADBA'),
        'border':('#E2E5EB','#303640'),
        'accent':('#6555CD','#AAA1FF'),'onAccent':('#FFFFFF','#231F46'),
        'accentSoft':('#EFEDF9','#2C2945'),
        'controlBorder':('#929AA9','#7F8999'),
        'success':('#32704F','#A0CDB3'),
        'successBg':('#EDF4EE','#29392F'),'warning':('#856029','#E4C596'),
        'warningBg':('#F8F1E5','#3C3428'),'danger':('#AC4050','#E6A6B2'),
        'dangerBg':('#F8ECEE','#422E33'),'scrim':('#E3E6ED','#101216'),
        'transparent':('#FFFFFF00','#FFFFFF00'),
    }
    for key,(light,dark) in pairs.items():add('color.'+key,'color',light,dark)
    add('font.family','font-family','Alva UI Sans SC')
    add('font.icons','font-family','Alva Interface Icons')
    add('font.mono','font-family','JetBrains Mono',description='Shared code, log and identifier font; OFL asset, not a per-component size or color.')
    for key,val in [('caption',11),('body',13),('subtitle',14),('heading',20),('display',28)]:add('font.size.'+key,'font-size',val)
    add('font.size.icon','font-size',16);add('font.size.iconSmall','font-size',14)
    for key,val in [('regular',400),('strong',600)]:add('font.weight.'+key,'font-weight',val)
    for key,val in [('normal',1.5),('reading',1.6)]:add('line.'+key,'number',val)
    for key,val in [('1',4),('2',8),('3',12),('4',16),('6',24),('8',32)]:add('space.'+key,'spacing',val)
    for key,val in [('small',4),('ctrl',8),('card',16),('pill',99)]:add('radius.'+key,'border-radius',val)
    add('border.focus','stroke-width',2)
    add('control.height','sizing',32)
    add('control.icon','sizing',28)
    add('control.row','sizing',28)
    add('icon.size','sizing',16);add('icon.small','sizing',14)
    add('size.sidebar','sizing',220)
    add('size.navigation','sizing',208)
    add('size.panel','sizing',432,values={'Viewport/Narrow':320})
    add('size.viewport.width','sizing',1440,values={'Viewport/Narrow':1024})
    add('size.viewport.height','sizing',960)
    add('size.header','sizing',36)
    add('size.panelHeader','sizing',52)
    add('shadow.popup','shadow',{'offsetX':0,'offsetY':8,'blur':30,'spread':-6,'color':'#2026351A'},
        {'offsetX':0,'offsetY':8,'blur':30,'spread':-6,'color':'#00000066'})
    add('motion.normal','number',160,description='One transition duration in milliseconds; reduced motion disables decorative transitions in the implementation.')
    add('motion.spinner','number',840,description='One linear revolution in milliseconds while busy; reduced motion uses a static indicator and text.')
    add('motion.reduced','boolean',False)
    return out


# Block recipes compose the shared scale; they do not create more Tokens or axes.
# Fixed dimensions of a component sample are not application layout constraints.
BLOCKS={
    'navigation':dict(padding=ref('space.3'),gap=ref('space.2')),
    'page':dict(padding=ref('space.6'),gap=ref('space.6')),
    'card':dict(padding=ref('space.4'),gap=ref('space.4'),radius=ref('radius.card')),
    'list':dict(padding=[ref('space.2'),ref('space.3')],gap=ref('space.2'),radius=ref('radius.ctrl')),
    'form':dict(gap=ref('space.4')),
    'dialog':dict(padding=ref('space.6'),gap=ref('space.4'),radius=ref('radius.card')),
    'composer':dict(padding=ref('space.2'),gap=ref('space.2'),radius=ref('radius.card')),
    'popover':dict(padding=ref('space.2'),gap=ref('space.1'),radius=ref('radius.card'),
        fill=ref('color.surface'),stroke=ref('color.border'),shadow=ref('shadow.popup')),
}


def block(kind,**extra):return deepcopy(BLOCKS[kind])|extra


def text(name,value,size='body',color='text',strong=False,**extra):
    return dict(name=name,text=value,fontFamily=ref('font.family'),fontSize=ref('font.size.'+size),
        fontWeight=ref('font.weight.strong' if strong else 'font.weight.regular'),lineHeight=ref('line.normal'),
        color=ref('color.'+color),width='fill')|extra


def box(name,children=(),**extra):return dict(name=name,children=list(children),layout='column')|extra


def popover(name,children=(),**extra):return box(name,children,**block('popover',**extra))


def row(name,children=(),**extra):return box(name,children,**(dict(layout='row',width='fill',align='center',gap=ref('space.2'))|extra))


def instance(name,component,props=None,texts=None,sets=None,**extra):
    if component=='ModelRow':props={'Mode':'Picker',**(props or {})}
    nested={
        'Field':{'Value':'Control.Value'},
        'ConnectionRow':{'Title':'Copy.Title','Description':'Copy.Description'},
        'ModelRow':{'Title':'Copy.Title','Description':'Copy.Description'},
        'SkillRow':{'Title':'Copy.Title','Description':'Copy.Description'},
        'ResourceRow':{'Title':'Copy.Title','Description':'Copy.Description'},
        'TaskRow':{'Title':'Copy.Title','Description':'Copy.Description'},
        'CompanionRow':{'Title':'Copy.Title','Description':'Copy.Description'},
        'Notice':{'Title':'Copy.Title','Description':'Copy.Description'},
        'PageHeader':{'Title':'Copy.Title','Description':'Copy.Description'},
        'DialogHeader':{'Title':'Copy.Title','Description':'Copy.Description'},
        'Progress':{'Title':'Copy.Title','Percent':'Copy.Percent'},
        'QuestionOption':{'Title':'Copy.Title','Description':'Copy.Description'},
        'MenuItem':{'Label':'Copy.Label','Description':'Copy.Description'},
        'Question':{'Title':'Header.Title'},'Approval':{'Title':'Header.Title'},
    }.get(component,{})
    if texts:texts={nested.get(k,k):v for k,v in texts.items()}
    overrides=dict(sets or {});direct_text={}
    for key,value in (texts or {}).items():
        if '.' in key:overrides[key+'.text']=value
        else:direct_text[key]=value
    pending={}
    base=BASES.get('Alva '+component)
    if base:
        pending={k:v for k,v in overrides.items() if crosses_instance(base,k)}
        overrides={k:v for k,v in overrides.items() if k not in pending}
    return dict(name=name,use='Alva '+component,**({'props':props} if props else {}),
        **({'text':direct_text} if direct_text else {}),**({'set':overrides} if overrides else {}),
        **({'_nestedSets':pending} if pending else {}),**extra)


def icon(name,symbol='Plus',small=False,color=None):
    overrides={}
    if color:overrides['Glyph.color']=ref('color.'+color)
    if small:overrides['Glyph.fontSize']=ref('font.size.iconSmall')
    return instance(name,'Icon',{'Name':symbol},sets=overrides,
        width=ref('icon.small' if small else 'icon.size'),height=ref('icon.small' if small else 'icon.size'))


def button(name,label,kind='Secondary',state='Default',symbol=None,content=None,**extra):
    content=content or ('Leading' if symbol else 'Text')
    sets={}
    if symbol:sets['Icon.props']={'Name':symbol}
    if symbol and content=='Trailing':sets={'Trailing.props':{'Name':symbol}}
    height=ref('control.height')
    return instance(name,'Button',{'Kind':kind,'State':state,'Content':content},texts={'Label':label},sets=sets,**(dict(width=height if content=='Icon' else 'hug',height=height)|extra))


def icon_button(name,symbol='Plus',state='Default'):
    return instance(name,'IconButton',{'State':state},sets={'Icon.props':{'Name':symbol}},width=ref('control.icon'),height=ref('control.icon'))


def badge(name,label,tone='Neutral',**extra):
    return instance(name,'Badge',{'Tone':tone},texts={'Label':label},width='hug',height='hug',**extra)


def field(name,label,value,hint='',state='Default',kind='Text',**extra):
    return instance(name,'Field',{'State':state},texts={'Title':label,'Value':value,'Hint':hint},
        sets={'Hint.visible':bool(hint),'Control.props':{'Kind':kind,'State':state}},width='fill',height='hug',**extra)


def notice(name,title,description,tone='Info',**extra):
    return instance(name,'Notice',{'Tone':tone},texts={'Title':title,'Description':description},
        sets={'Copy.Description.visible':bool(description)},width='fill',height='hug',**extra)


def empty(name,title,description,symbol='Folder',state='Empty',action=False):
    return instance(name,'EmptyState',{'State':state},texts={'Title':title,'Description':description},
        sets={'Icon.props':{'Name':symbol},'Action.visible':action,'Description.visible':bool(description)},width='fill',height='hug')


def component(name,base,properties=None,variants=None,category='Controls',description=''):
    def own_path(node,target,path=()):
        if node['name']==target:return path
        if node.get('use'):return None
        for child in node.get('children',[]):
            found=own_path(child,target,(*path,child['name']))
            if found is not None:return found
        return None
    rules=deepcopy(variants or [])
    for rule in rules:
        qualified={}
        for path,value in rule['set'].items():
            parts=path.split('.')
            found=own_path(base,parts[0]) if len(parts)>1 else None
            qualified['.'.join([*found,*parts[1:]]) if found else path]=value
        rule['set']=qualified
    BASES['Alva '+name]=base
    return dict(name='Alva '+name,base=base,properties=properties or {},variants=rules,category=category,description=description)


def button_definitions():
    """One action family, plus persistent toggles; loading owns a separate glyph."""
    states=['Default','Hover','Pressed','Focus','Disabled','Loading','LoadingFocus']
    def paint(color):
        return {name+'.Glyph.color':ref('color.'+color) for name in ['Icon','Trailing','Spinner']}|{'Label.color':ref('color.'+color)}
    def surface(fill,stroke=None):
        return {'fill':ref('color.'+fill),'stroke':ref('color.'+(stroke or fill))}
    def rule(when,changes):return {'when':when,'set':changes}
    def focus():
        # The typed finishing pass expresses two rounded inset strokes. Outer
        # strokes/shadows are clipped by SmallPen's component export bounds.
        return {'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}
    base=row('Button',[icon('Spinner','Loader',small=True),icon('Icon','Plus',small=True),text('Label','创建任务','body',width='hug',textAlign='center'),icon('Trailing','ChevronDown',small=True)],
        width='hug',height=ref('control.height'),justify='center',padding=[0,ref('space.2')],
        fill=ref('color.subtle'),radius=ref('radius.ctrl'))
    rules=[rule({},paint('text')|{'Spinner.visible':False,'Trailing.visible':False}),
        rule({'Content':'Text'},{'Icon.visible':False}),
        rule({'Content':'Trailing'},{'Icon.visible':False,'Trailing.visible':True}),
        rule({'Content':'Icon'},{'Label.visible':False,'padding':0,'width':ref('control.height')}),
    ]
    palette={
        'Primary':('text','surface','text','text'),
        'Secondary':('surface','text','hover','hover'),
        'Ghost':('transparent','secondary','subtle','hover'),
        'Danger':('transparent','danger','dangerBg','dangerBg'),
    }
    for kind,(fill,foreground,hover,pressed) in palette.items():
        rules.append(rule({'Kind':kind},surface(fill)|paint(foreground)))
        for state,color in [('Hover',hover),('Pressed',pressed)]:
            rules.append(rule({'Kind':kind,'State':state},surface(color)))
    rules.append(rule({'Kind':'Primary','State':'Hover'},{'opacity':0.98}))
    rules.append(rule({'Kind':'Primary','State':'Pressed'},{'opacity':0.96}))
    rules.append(rule({'Kind':'Secondary','State':'Default'},{'stroke':ref('color.border')}))
    for state in ('Loading','LoadingFocus'):
        rules.append(rule({'Kind':'Secondary','State':state},surface('subtle')))
    rules.append(rule({'Kind':'Ghost','State':'Pressed'},paint('text')))
    rules.append(rule({'Kind':'Danger','State':'Pressed'},surface('danger')|paint('surface')))
    for state in ('Focus','LoadingFocus'):rules.append(rule({'State':state},focus()))
    rules.append(rule({'State':'Disabled'},surface('subtle')|paint('secondary')))
    for state in ('Loading','LoadingFocus'):
        rules.append(rule({'State':state},{'Spinner.visible':True,'Icon.visible':False,'Trailing.visible':False}))
        rules.append(rule({'State':state,'Content':'Text'},{'Label.opacity':0}))
    button_spec=component('Button',base,{'Kind':['Primary','Secondary','Ghost','Danger'],'State':states,
        'Content':['Text','Leading','Trailing','Icon']},rules,'Controls',
        'One 36px action height; width hugs content. Four intents and content placements share seven real interaction states. Primary uses text/surface inversion, Secondary uses a neutral surface and border, Ghost is quiet, Danger is contextual. Loading retains the action label: Text keeps transparent text in layout and overlays a centered Loader; Leading/Trailing replace the corresponding icon slot; Icon keeps its square. Keep the same content and allocated width across states. Runtime preserves accessible name/focus, exposes busy, suppresses repeat activation and owns motion. Icon-only actions require a name.')
    icon_states=['Default','Hover','Pressed','Focus','Disabled','Loading','LoadingFocus','Selected','SelectedHover','SelectedPressed','SelectedFocus','SelectedDisabled']
    ibase=row('IconButton',[icon('Spinner','Loader'),icon('Icon')],width=ref('control.icon'),height=ref('control.icon'),
        justify='center',fill=ref('color.transparent'),stroke=ref('color.transparent'),radius=ref('radius.ctrl'))
    irules=[rule({}, {'Spinner.visible':False}),rule({'State':'Hover'},surface('subtle')),rule({'State':'Pressed'},surface('hover'))]
    for state in ('Selected','SelectedHover','SelectedPressed','SelectedFocus','SelectedDisabled'):
        irules.append(rule({'State':state},surface('hover' if state in ('SelectedHover','SelectedPressed') else 'accentSoft')|{'Icon.Glyph.color':ref('color.text' if state in ('SelectedHover','SelectedPressed') else 'color.accent')}))
    for state in ('Focus','LoadingFocus','SelectedFocus'):irules.append(rule({'State':state},focus()))
    for state in ('Loading','LoadingFocus'):irules.append(rule({'State':state},{'Icon.visible':False,'Spinner.visible':True}))
    for state in ('Disabled','SelectedDisabled'):irules.append(rule({'State':state},{'Icon.Glyph.color':ref('color.secondary' if state=='SelectedDisabled' else 'color.secondary')}))
    icon_spec=component('IconButton',ibase,{'State':icon_states},irules,'Controls',
        'Compact toolbar action; tooltip and accessible name are required. Selected variants are persistent toggles with aria-pressed. Loading cannot be replaced by an icon override; the adjacent task Stop action remains independent.')
    tbase=row('ToggleButton',[icon('Icon','Panel',small=True),text('Label','面板','body',width='hug',textAlign='center')],
        width='hug',height=ref('control.height'),justify='center',padding=[0,ref('space.3')],fill=ref('color.transparent'),radius=ref('radius.ctrl'))
    trules=[rule({'Content':'Text'},{'Icon.visible':False})]
    toggle_states=['Default','Hover','Pressed','Focus','Disabled','Selected','SelectedHover','SelectedPressed','SelectedFocus','SelectedDisabled']
    for state in toggle_states:
        changes={}
        if state.startswith('Selected'):
            changes=surface('hover' if state in ('SelectedPressed','SelectedHover') else 'accentSoft')|{'Label.color':ref('color.text'),'Icon.Glyph.color':ref('color.text' if state in ('SelectedPressed','SelectedHover') else 'color.accent')}
        elif state in ('Hover','Pressed'):changes=surface('subtle' if state=='Hover' else 'hover')
        if state.endswith('Focus'):changes|=focus()
        if state.endswith('Disabled'):
            foreground='secondary' if state=='SelectedDisabled' else 'secondary'
            changes|={'Label.color':ref('color.'+foreground),'Icon.Glyph.color':ref('color.'+foreground)}
        if changes:trules.append(rule({'State':state},changes))
    toggle_spec=component('ToggleButton',tbase,{'State':toggle_states,'Content':['Text','Leading']},trules,'Selection',
        'Persistent on/off choice, not an action result. Selected remains visible when focused or disabled. Stable accessible label with aria-pressed; use Tab or Segment for navigation and exclusive selection.')
    return [button_spec,icon_spec,toggle_spec]


def complete_common_states(defs):
    """Append real interaction states without multiplying content or size axes."""
    by_name={d['name'].removeprefix('Alva '):d for d in defs}
    def state(name,value,base,changes):
        d=by_name[name]
        inherited=[deepcopy(r) for r in d['variants'] if r['when'].get('State')==base]
        if value not in d['properties']['State']:d['properties']['State'].append(value)
        for rule in inherited:
            rule['when']['State']=value
            d['variants'].append(rule)
        d['variants'].append({'when':{'State':value},'set':changes})
    focus={'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}
    for name in ('NavItem','Tab','MenuItem','Segment'):
        state(name,'SelectedFocus','Selected',focus)
        disabled={('Copy.Label.color' if name=='MenuItem' else 'Label.color'):ref('color.secondary')}
        if name=='NavItem':disabled['Icon.Glyph.color']=ref('color.secondary')
        state(name,'SelectedDisabled','Selected',disabled)
    state('Segment','Focus','Default',focus)
    state('Tab','Disabled','Default',{'Label.color':ref('color.secondary'),'Icon.Glyph.color':ref('color.secondary')})
    state('QuestionOption','Focus','Default',focus)
    state('QuestionOption','SelectedFocus','Selected',focus)
    state('QuestionOption','SelectedDisabled','Selected',{'Radio.props':{'State':'OnDisabled'},'Copy.Title.color':ref('color.secondary')})
    state('Input','ErrorFocus','Error',{'stroke':{'color':ref('color.danger'),'width':ref('border.focus')}})
    state('Input','ReadOnly','Default',{'Trailing.visible':True,'Trailing.props':{'Name':'Lock'},'Value.color':ref('color.text')})
    state('Field','ErrorFocus','Error',{'Control.props':{'Kind':'Text','State':'ErrorFocus'}})
    state('Field','ReadOnly','Default',{'Control.props':{'Kind':'Text','State':'ReadOnly'}})
    state('TextArea','ErrorFocus','Error',{'stroke':{'color':ref('color.danger'),'width':ref('border.focus')}})
    state('TextArea','ReadOnly','Default',{'fill':ref('color.subtle'),'Value.color':ref('color.text')})
    state('ToolActivity','Expanded','Collapsed',{'Chevron.props':{'Name':'ChevronUp'}})
    state('ToolActivity','Focus','Collapsed',focus)
    state('ToolActivity','ExpandedFocus','Expanded',focus)
    state('ToolActivity','ErrorExpanded','Error',{'Chevron.props':{'Name':'ChevronUp'}})
    state('TaskRow','Queued','Waiting',{'Status.Label.text':'等待运行','Status.props':{'Tone':'Neutral'},'Icon.props':{'Name':'Clock'},'Copy.Description.text':'研究伙伴 · ALVA'})


def content_definitions():
    """Shared primitives for existing attachment, reading, selection and inspector flows."""
    focus={'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}
    option=component('SelectOption',row('SelectOption',[
        box('Copy',[text('Label','当前项目','body'),text('Description','仅此项目','caption','secondary',visible=False)],width='fill',gap=ref('space.1')),
        icon('Check','Check',small=True,color='accent')],width=320,height='hug',**block('list'),fill=ref('color.surface'),stroke=ref('color.transparent')),
        {'State':['Default','Hover','Selected','Focus','SelectedFocus','Disabled','SelectedDisabled']},[
            {'when':{'State':s},'set':{'Check.visible':False}} for s in ['Default','Hover','Focus','Disabled']]+[
            {'when':{'State':s},'set':{'fill':ref('color.subtle')}} for s in ['Hover','Selected','SelectedFocus','SelectedDisabled']]+[
            {'when':{'State':s},'set':focus} for s in ['Focus','SelectedFocus']]+[
            {'when':{'State':s},'set':{'Copy.Label.color':ref('color.secondary'),'Check.Glyph.color':ref('color.secondary')}} for s in ['Disabled','SelectedDisabled']],
        'Selection','Option for single-select lists. Search, empty/loading/error content and the popup container are composed by the page. Selection and keyboard focus remain independent; multiselect uses Checkbox.')
    attachment=component('Attachment',row('Attachment',[
        icon('Icon','File',color='accent'),box('Copy',[text('Name','工作台方案.md','body'),text('Meta','12 KB','caption','secondary')],width='fill',gap=ref('space.1')),
        icon_button('Retry','Refresh'),icon_button('Remove','Close')],width=320,height='hug',padding=ref('space.2'),gap=ref('space.2'),
        fill=ref('color.surface'),stroke=ref('color.border'),radius=ref('radius.ctrl')),
        {'State':['Ready','Uploading','Error','Unavailable']},[
            {'when':{},'set':{'Retry.visible':False}},
            {'when':{'State':'Uploading'},'set':{'Icon.props':{'Name':'Loader'},'Copy.Meta.text':'正在上传 · 60%'}},
            {'when':{'State':'Error'},'set':{'Icon.props':{'Name':'Warning'},'Icon.Glyph.color':ref('color.danger'),'Copy.Meta.text':'上传失败，文件已保留','Copy.Meta.color':ref('color.danger'),'Retry.visible':True}},
            {'when':{'State':'Unavailable'},'set':{'Icon.props':{'Name':'Lock'},'Copy.Meta.text':'来源不可用','Copy.Name.color':ref('color.secondary')}},
        ],'Resources','Draft attachment/reference, not a permission grant. Remove only removes the draft reference; while uploading it requests cancellation and awaits cleanup. Retry requires an actual failure. Icon actions need accessible names and tooltips.')
    code=component('CodeBlock',box('CodeBlock',[
        row('Header',[text('Language','Go','caption','secondary'),button('Copy','复制','Ghost',symbol='Copy')]),
        text('Code','result, err := agent.Run(ctx, input)\nif err != nil {\n    return err\n}',fontFamily=ref('font.mono'),lineHeight=ref('line.reading')),
    ],width=560,height='hug',padding=ref('space.4'),gap=ref('space.2'),fill=ref('color.subtle'),radius=ref('radius.ctrl')),
        {'State':['Default','Copied','CopyError']},[
            {'when':{'State':'Copied'},'set':{'Copy.Label.text':'已复制','Copy.Icon.props':{'Name':'Check'}}},
            {'when':{'State':'CopyError'},'set':{'Copy.Label.text':'复制失败','Copy.Icon.props':{'Name':'Warning'}}},
        ],'Reading','Code/log content is selectable text. Preserve whitespace; horizontal scrolling, long-content virtualization and clipboard result handling belong to the runtime. No syntax-color Token expansion or permanent animation.')
    citation=component('Citation',row('Citation',[
        box('Index',[text('Number','1','caption',strong=True,width='hug',textAlign='center')],width=24,height=24,align='center',justify='center',fill=ref('color.subtle'),radius=ref('radius.small')),
        box('Copy',[text('Title','工作台方案.md','body'),text('Location','当前项目 · 第 2 节','caption','secondary')],width='fill',gap=ref('space.1')),
        icon('Open','External',small=True)],width=360,height='hug',**block('list'),fill=ref('color.surface'),stroke=ref('color.transparent')),
        {'State':['Default','Focus','Unavailable']},[
            {'when':{'State':'Focus'},'set':focus},
            {'when':{'State':'Unavailable'},'set':{'Open.props':{'Name':'Warning'},'Copy.Location.text':'第 2 节 · 来源不可用','Copy.Title.color':ref('color.secondary')}},
        ],'Reading','Citation points to an actual file/page/range or URL. Opening is observation, not model delivery or permission. Unavailable sources retain their identity and location in the runtime.')
    detail=component('DetailRow',row('DetailRow',[
        text('Label','连接来源','body','secondary',width=120),text('Value','OpenAI · 主连接','body'),icon_button('Copy','Copy')],
        width=560,height='hug',padding=[ref('space.2'),0],align='start',stroke={'color':ref('color.border'),'width':[0,0,1,0]}),
        variants=[{'when':{},'set':{'Copy.visible':False}}],category='Data',
        description='Read-only inspector/configuration row. Value wraps and remains selectable; optional copy is an instance setting. IDs/log values may bind ui.font.mono. Narrow layouts stack label/value in their parent composition.')
    return [option,attachment,code,citation,detail]


def provider_definitions():
    form=component('ProviderForm',box('ProviderForm',[
        button('ConnectionSettings','连接设置','Ghost',symbol='ChevronDown'),
        box('ConnectionDetails',[
            row('Identity',[field('Name','连接名称','我的模型服务'),field('Auth','认证方式','API Key',kind='Select')],gap=ref('space.4')),
            field('Protocol','API 协议','OpenAI Responses',kind='Select'),
            field('Endpoint','完整请求地址','https://api.example.com/v1/responses'),
        ],width='fill',height='hug',**block('form')),
        field('ApiKey','API Key','••••••••••••••••',kind='Password'),
        row('Actions',[button('Advanced','高级设置','Ghost',symbol='ChevronDown'),
            box('Space',width='fill',height=1),button('Test','测试连接','Ghost')]),
        notice('TestStatus','等待测试','','Info',visible=False),
    ],width=752,height='hug',**block('form')),{'Source':['Preset','Custom']},[
        {'when':{'Source':'Preset'},'set':{'ConnectionDetails.visible':False,'Actions.Advanced.visible':False,
            'ConnectionDetails.Identity.Name.Control.Value.text':'OpenAI · 主连接',
            'ConnectionDetails.Endpoint.Control.Value.text':'https://api.openai.com/v1/responses'}},
        {'when':{'Source':'Custom'},'set':{'ConnectionSettings.visible':False}},
    ],'Models','Shared add/edit connection fields. Source is a real structural choice; provider identity, name, endpoint and add/edit mode are instance data. Presets prefill full native endpoints; ConnectionSettings reveals the existing hidden group. Test is optional and separate from save/model discovery. Credentials here are masked examples, never real secrets.')
    models=component('ProviderModels',box('ProviderModels',[
        row('Header',[text('Title','模型（可选）','subtitle',strong=True),
            text('Count','已选 2','caption','secondary',width='hug',visible=False),button('Fetch','获取模型','Secondary',symbol='Refresh')]),
        text('Status','尚未获取模型','caption','secondary'),
        notice('Error','模型获取失败','检查地址或凭据后重试，已填写内容会保留。','Danger',visible=False),
        instance('Search','Input',{'Kind':'Search','State':'Default'},texts={'Value':'搜索模型…'},width='fill',height=ref('control.height'),visible=False),
        box('Choices',[
            instance('ModelA','Checkbox',{'State':'On'},texts={'Label':'GPT'},width='fill',height=32),
            instance('ModelB','Checkbox',{'State':'On'},texts={'Label':'GPT Mini'},width='fill',height=32),
        ],width='fill',height='hug',gap=ref('space.1'),visible=False),
        row('Manual',[instance('ID','Input',{'Kind':'Text','State':'Default'},texts={'Value':'输入模型 ID'},width='fill',height=ref('control.height')),
            button('Add','添加','Secondary',state='Disabled')]),
    ],width=752,height='hug',gap=ref('space.2')),{'State':['Idle','Loading','Ready','Empty','Unavailable','Error']},[
        {'when':{'State':'Loading'},'set':{'Header.Fetch.props':{'Kind':'Secondary','Content':'Leading','State':'Loading'},'Status.visible':False}},
        {'when':{'State':'Ready'},'set':{'Status.visible':False,'Search.visible':True,'Choices.visible':True,'Header.Count.visible':True}},
        {'when':{'State':'Empty'},'set':{'Status.text':'目录暂未返回模型，可手动添加或稍后刷新。','Header.Fetch.Label.text':'重新获取'}},
        {'when':{'State':'Unavailable'},'set':{'Status.text':'此服务不提供模型目录，可手动添加。','Header.Fetch.visible':False}},
        {'when':{'State':'Error'},'set':{'Status.visible':False,'Error.visible':True,'Header.Fetch.Label.text':'重试'}},
    ],'Models','One optional inline model region, independent of ProviderForm.Source. Fetch uses the current draft without saving it; failed/unsupported catalog keeps manual ID and connection save available. Model labels are design examples. Manual Add enables only for a valid nonduplicate ID; stable identity is connection ID plus exact model ID. Loading/retry/selection do not create more pages or a Source×State product.')
    return [form,models]


def definitions():
    defs=[]
    glyphs=json.loads((ROOT/'fonts'/'icons.json').read_text())['glyphs']
    glyph=dict(name='Glyph',text=glyphs[0]['character'],fontFamily=ref('font.icons'),fontSize=ref('font.size.icon'),fontWeight=400,
        lineHeight=1,color=ref('color.secondary'),width='fill',textAlign='center')
    defs.append(component('Icon',row('Icon',[glyph],width=20,height=20,justify='center'),
        {'Name':[g['name'] for g in glyphs]},[{'when':{'Name':g['name']},'set':{'Glyph.text':g['character']}} for g in glyphs],
        'Symbols','Lucide 1.61.0 line icons only, with the official 24-unit canvas and 2-unit strokes. Stable Alva symbol names/codepoints; semantic color follows the theme.'))
    defs.extend(button_definitions())
    input_states=['Default','Focus','Error','Disabled','Loading']
    defs.append(component('Input',row('Input',[icon('Leading','Search',small=True),text('Value','搜索或输入内容','body','secondary'),icon('Trailing','ChevronDown',small=True)],
        width=320,height=ref('control.height'),padding=[0,ref('space.2')],fill=ref('color.subtle'),stroke=ref('color.border'),radius=ref('radius.ctrl')),
        {'Kind':['Text','Search','Select','Password'],'State':list(input_states)},[
            {'when':{'Kind':'Text'},'set':{'Leading.visible':False,'Trailing.visible':False}},
            {'when':{'Kind':'Search'},'set':{'Trailing.visible':False}},
            {'when':{'Kind':'Select'},'set':{'Leading.visible':False}},
            {'when':{'Kind':'Password'},'set':{'Leading.props':{'Name':'Key'},'Trailing.props':{'Name':'Eye'},'Value.text':'••••••••••••••••'}},
            {'when':{'State':'Focus'},'set':{'fill':ref('color.surface'),'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}},
            {'when':{'State':'Error'},'set':{'fill':ref('color.surface'),'stroke':ref('color.danger'),'Trailing.visible':True,'Trailing.props':{'Name':'Warning'}}},
            {'when':{'State':'Disabled'},'set':{'fill':ref('color.canvas'),'opacity':0.6,'Value.color':ref('color.secondary')}},
            {'when':{'State':'Loading'},'set':{'Trailing.visible':True,'Trailing.props':{'Name':'Loader'}}},
        ]))
    defs.append(component('Field',box('Field',[text('Title','连接名称','body'),
        instance('Control','Input',{'Kind':'Text','State':'Default'},width='fill',height=ref('control.height')),
        text('Hint','仅影响下一轮任务','caption','secondary')],width=360,height='hug',gap=ref('space.2')),
        {'State':list(input_states)},[
            {'when':{'State':s},'set':{'Control.props':{'Kind':'Text','State':s}}} for s in input_states]+[
            {'when':{'State':'Error'},'set':{'Hint.color':ref('color.danger'),'Hint.text':'请检查此项配置'}},
            {'when':{'State':'Disabled'},'set':{'Title.color':ref('color.secondary')}}]))
    defs.append(component('TextArea',box('TextArea',[text('Value','说明任务、引用资料，或输入你的回答…','body','secondary')],
        width=360,height=96,padding=[ref('space.3'),ref('space.2')],fill=ref('color.subtle'),stroke=ref('color.border'),radius=ref('radius.ctrl')),
        {'State':['Default','Focus','Error','Disabled']},[
            {'when':{'State':'Focus'},'set':{'fill':ref('color.surface'),'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}},
            {'when':{'State':'Error'},'set':{'fill':ref('color.surface'),'stroke':ref('color.danger')}},
            {'when':{'State':'Disabled'},'set':{'fill':ref('color.canvas'),'opacity':0.6}}]))
    defs.append(component('Checkbox',row('Checkbox',[box('Mark',[icon('Check','Check',small=True,color='onAccent')],width=18,height=18,align='center',justify='center',
        fill=ref('color.surface'),stroke=ref('color.controlBorder'),radius=ref('radius.small')),
        text('Label','选择此项','body')],width=240,height=32,fill=ref('color.surface')),
        {'State':['Off','On','Mixed','Focus','Disabled','OnFocus','OnDisabled','MixedFocus','MixedDisabled']},[
            {'when':{'State':'Off'},'set':{'Check.visible':False}},
            {'when':{'State':'On'},'set':{'Mark.fill':ref('color.accent'),'Mark.stroke':ref('color.accent')}},
            {'when':{'State':'Mixed'},'set':{'Mark.fill':ref('color.accent'),'Check.props':{'Name':'More'}}},
            {'when':{'State':'Focus'},'set':{'Mark.stroke':{'color':ref('color.accent'),'width':ref('border.focus')},'Check.visible':False}},
            {'when':{'State':'Disabled'},'set':{'Mark.fill':ref('color.subtle'),'Check.visible':False,'Label.color':ref('color.secondary')}}]))
    defs[-1]['variants'].extend([
        {'when':{'State':s},'set':{'Mark.fill':ref('color.accent'),'Mark.stroke':ref('color.accent'),
            **({'Mark.Check.props':{'Name':'More'}} if s.startswith('Mixed') else {})}}
        for s in ['OnFocus','OnDisabled','MixedFocus','MixedDisabled']])
    defs[-1]['variants'].extend([{'when':{'State':s},'set':{'Mark.stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}} for s in ['OnFocus','MixedFocus']])
    defs[-1]['variants'].extend([{'when':{'State':s},'set':{'Mark.fill':ref('color.subtle'),'Mark.stroke':ref('color.border'),'Mark.Check.Glyph.color':ref('color.secondary'),'Label.color':ref('color.secondary')}} for s in ['OnDisabled','MixedDisabled']])
    defs.append(component('Radio',row('Radio',[box('Ring',[box('Dot',width=8,height=8,fill=ref('color.accent'),radius=ref('radius.pill'))],
        width=18,height=18,align='center',justify='center',fill=ref('color.surface'),stroke=ref('color.controlBorder'),radius=ref('radius.pill')),
        text('Label','单选项','body')],width=240,height=32),{'State':['Off','On','Focus','Disabled','OnFocus','OnDisabled']},[
            {'when':{'State':s},'set':{'Dot.visible':False}} for s in ['Off','Focus','Disabled']]+[
            {'when':{'State':'On'},'set':{'Ring.stroke':ref('color.accent')}},
            {'when':{'State':'Focus'},'set':{'Ring.stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}},
            {'when':{'State':'Disabled'},'set':{'Ring.fill':ref('color.subtle'),'Label.color':ref('color.secondary')}}]))
    defs[-1]['variants'].extend([
        {'when':{'State':'OnFocus'},'set':{'Ring.stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}},
        {'when':{'State':'OnDisabled'},'set':{'Ring.Dot.fill':ref('color.secondary'),'Label.color':ref('color.secondary')}}])
    defs.append(component('Switch',row('Switch',[box('OnSpacer',width='fill',height=1,visible=False),box('Thumb',width=16,height=16,fill=ref('color.secondary'),radius=ref('radius.pill'))],
        width=38,height=24,padding=4,gap=0,fill=ref('color.subtle'),stroke=ref('color.border'),radius=ref('radius.pill')),
        {'State':['Off','On','Focus','Disabled','OnFocus','OnDisabled']},[
            {'when':{'State':'On'},'set':{'fill':ref('color.accent'),'stroke':ref('color.accent'),'OnSpacer.visible':True,'Thumb.fill':ref('color.onAccent')}},
            {'when':{'State':'Focus'},'set':{'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}},
            {'when':{'State':'Disabled'},'set':{'Thumb.fill':ref('color.secondary')}}]))
    defs[-1]['variants'].extend([
        {'when':{'State':'OnFocus'},'set':{'OnSpacer.visible':True,'fill':ref('color.accent'),'stroke':{'color':ref('color.accent'),'width':ref('border.focus')},'Thumb.fill':ref('color.onAccent')}},
        {'when':{'State':'OnDisabled'},'set':{'OnSpacer.visible':True,'Thumb.fill':ref('color.secondary')}}])
    tones=['Neutral','Accent','Success','Warning','Danger','Info']
    tone_map={'Accent':('accentSoft','accent'),'Success':('successBg','success'),'Warning':('warningBg','warning'),'Danger':('dangerBg','danger'),'Info':('subtle','secondary')}
    defs.append(component('Badge',row('Badge',[text('Label','状态','caption','secondary',width='hug',textAlign='center')],width='hug',height='hug',justify='center',padding=[4,8],
        fill=ref('color.subtle'),radius=ref('radius.pill')),{'Tone':tones},[
            {'when':{'Tone':tone},'set':{'fill':ref('color.'+colors[0]),'Label.color':ref('color.'+colors[1])}} for tone,colors in tone_map.items()]))
    defs.append(component('Avatar',box('Avatar',[text('Label','研','body','accent',True,width='hug',textAlign='center')],width=32,height=32,align='center',justify='center',
        fill=ref('color.accentSoft'),stroke=ref('color.border'),radius=ref('radius.ctrl')),
        {'Kind':['Person','Agent','Provider']},[
            {'when':{'Kind':'Person'},'set':{'radius':ref('radius.pill')}},
            {'when':{'Kind':'Provider'},'set':{'fill':ref('color.subtle'),'Label.color':ref('color.secondary')}}]))
    defs.append(component('NavItem',row('NavItem',[icon('Icon','Chat'),text('Label','项目与会话','body'),
        text('Meta','3','caption','secondary',width='hug',textAlign='center')],width=208,height=ref('control.row'),padding=[0,ref('space.3')],
        fill=ref('color.transparent'),stroke=ref('color.transparent'),radius=ref('radius.ctrl')),
        {'State':['Default','Hover','Selected','Focus','Disabled']},[
            {'when':{'State':'Hover'},'set':{'fill':ref('color.hover')}},
            {'when':{'State':'Selected'},'set':{'fill':ref('color.surface'),'Label.color':ref('color.text'),'Icon.Glyph.color':ref('color.text')}},
            {'when':{'State':'Focus'},'set':{'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}},
            {'when':{'State':'Disabled'},'set':{'Label.color':ref('color.secondary')}}]))
    defs.append(component('Tab',row('Tab',[icon('Icon','File',small=True),text('Label','文件','body',width='hug'),
        icon('Close','Close',small=True)],width=136,height=ref('control.row'),padding=[0,ref('space.2')],fill=ref('color.subtle'),stroke=ref('color.subtle'),radius=ref('radius.ctrl')),
        {'State':['Default','Selected','Focus','Busy']},[
            {'when':{'State':'Selected'},'set':{'fill':ref('color.surface'),'stroke':ref('color.surface')}},
            {'when':{'State':'Focus'},'set':{'stroke':ref('color.accent')}},
            {'when':{'State':'Busy'},'set':{'Icon.props':{'Name':'Loader'}}}]))
    defs.append(component('MenuItem',row('MenuItem',[icon('Icon','Folder'),box('Copy',[text('Label','添加目录','body'),text('Description','选择当前任务可用的资料','caption','secondary')],width='fill',gap=ref('space.1')),
        text('Shortcut','⌘ O','caption','secondary',width='hug')],width=320,height='hug',padding=ref('space.3'),fill=ref('color.surface'),stroke=ref('color.surface'),radius=ref('radius.ctrl')),
        {'State':['Default','Hover','Selected','Disabled','Danger','Focus']},[
            {'when':{'State':s},'set':{'fill':ref('color.hover')}} for s in ['Hover','Selected']]+[
            {'when':{'State':'Disabled'},'set':{'Label.color':ref('color.secondary')}},
            {'when':{'State':'Danger'},'set':{'Label.color':ref('color.danger'),'Icon.Glyph.color':ref('color.danger')}}]))
    defs[-1]['variants'].append({'when':{'State':'Focus'},'set':{'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}})
    defs.append(component('Tooltip',box('Tooltip',[text('Label','显示运行详情','caption')],width='hug',height='hug',padding=[8,12],fill=ref('color.surface'),
        stroke=ref('color.border'),radius=ref('radius.small'),shadow=ref('shadow.popup'))))
    defs.append(component('Notice',row('Notice',[icon('Icon','Info'),box('Copy',[text('Title','连接状态','body',strong=True),
        text('Description','说明发生了什么，以及可以采取的下一步。','caption','secondary')],width='fill',gap=ref('space.2'))],
        width=480,height='hug',padding=ref('space.4'),align='start',fill=ref('color.subtle'),stroke=ref('color.subtle'),radius=ref('radius.ctrl')),
        {'Tone':['Info','Success','Warning','Danger']},[
            {'when':{'Tone':tone},'set':{'fill':ref('color.'+bg),'stroke':ref('color.'+bg),'Title.color':ref('color.'+fg),'Icon.Glyph.color':ref('color.'+fg),'Icon.props':{'Name':symbol}}}
            for tone,bg,fg,symbol in [('Info','subtle','secondary','Info'),('Success','successBg','success','Check'),('Warning','warningBg','warning','Warning'),('Danger','dangerBg','danger','Warning')]]))
    defs[-1]['variants'].append({'when':{'Tone':'Success'},'set':{'fill':ref('color.transparent'),'stroke':ref('color.transparent'),'padding':[8,0]}})
    defs.append(component('EmptyState',box('EmptyState',[icon('Icon','Folder'),text('Title','还没有内容','subtitle',strong=True,textAlign='center'),
        text('Description','添加项目或打开已有内容，继续你的工作。','caption','secondary',textAlign='center'),button('Action','开始添加',symbol='Plus')],width=360,height='hug',padding=ref('space.8'),gap=ref('space.3'),
        align='center',fill=ref('color.surface'),radius=ref('radius.card')),
        {'State':['Empty','Loading','Error','Unavailable']},[
            {'when':{'State':'Loading'},'set':{'Icon.props':{'Name':'Refresh'},'Title.text':'正在加载…','Action.visible':False}},
            {'when':{'State':'Error'},'set':{'Icon.props':{'Name':'Warning'},'Title.text':'加载失败','Action.Label.text':'重试'}},
            {'when':{'State':'Unavailable'},'set':{'Icon.props':{'Name':'Lock'},'Title.text':'当前不可用','Action.Label.text':'查看说明'}}], 'Feedback'))
    defs.append(component('SettingRow',row('SettingRow',[box('Copy',[text('Title','配置项','body'),text('Description','作用域、来源与生效时机','caption','secondary')],
        width='fill',gap=ref('space.1')),instance('Control','Switch',{'State':'Off'},width=38,height=24),button('Action','配置'),
        button('Select','当前值','Ghost',content='Trailing')],width=620,height='hug',padding=[ref('space.2'),0],
        fill=ref('color.surface'),stroke={'color':ref('color.border'),'width':[0,0,1,0]}),{'Control':['Switch','Action','Select']},[
            {'when':{'Control':'Switch'},'set':{'Action.visible':False,'Select.visible':False}},
            {'when':{'Control':'Action'},'set':{'Control.visible':False,'Select.visible':False}},
            {'when':{'Control':'Select'},'set':{'Control.visible':False,'Action.visible':False,'Copy.Description.visible':False}}], 'Settings',
        'One preference row: label and optional necessary description on the left; Switch, Action or content-hugging Select on the right. Select reuses Ghost/Trailing Button states and shared SelectOption/Popover content. Values/options are instance data; form fields remain Field/Input. Symmetric 8px vertical padding; description gap4.'))
    defs.append(component('ConnectionRow',row('ConnectionRow',[instance('Avatar','Avatar',{'Kind':'Provider'},texts={'Label':'O'},width=32,height=32,visible=False),
        box('Copy',[text('Title','OpenAI · 主连接','body',strong=True),text('Description','API Key','caption','secondary')],width='fill',gap=ref('space.1')),
        icon('Status','ChevronRight',small=True,color='secondary')],width=250,height='hug',padding=ref('space.3'),fill=ref('color.surface'),stroke=ref('color.surface'),radius=ref('radius.ctrl')),
        {'State':['Default','Selected','Disconnected','Error']},[
            {'when':{'State':'Selected'},'set':{'fill':ref('color.subtle'),'stroke':ref('color.subtle'),'Status.props':{'Name':'Check'},'Status.Glyph.color':ref('color.accent')}},
            {'when':{'State':'Disconnected'},'set':{'Status.props':{'Name':'ChevronRight'},'Status.Glyph.color':ref('color.secondary'),'Description.text':'未配置'}},
            {'when':{'State':'Error'},'set':{'Status.props':{'Name':'Warning'},'Status.Glyph.color':ref('color.danger'),'Description.text':'连接失败 · 可重试'}}], 'Models'))
    defs.append(component('ModelRow',row('ModelRow',[instance('Avatar','Avatar',{'Kind':'Provider'},texts={'Label':'G'},width=32,height=32,visible=False),
        box('Copy',[text('Title','GPT','body',strong=True),text('Description','OpenAI · 主连接 · 视觉输入','caption','secondary')],width='fill',gap=ref('space.1')),
        badge('Effort','ALVA · 默认',visible=False),icon_button('Favorite','Star'),icon_button('Settings','Sliders'),instance('Enabled','Switch',{'State':'On'},width=38,height=24,visible=False)],width=480,height='hug',padding=ref('space.3'),
        fill=ref('color.surface'),stroke=ref('color.surface'),radius=ref('radius.ctrl')),
        {'Mode':['Picker','Manage'],'State':['Default','Hover','Selected','Unavailable']},[
            {'when':{'Mode':'Manage'},'set':{'Favorite.visible':False,'Effort.visible':False,'Enabled.visible':True}},
            {'when':{'State':'Hover'},'set':{'fill':ref('color.hover')}},
            {'when':{'State':'Selected'},'set':{'fill':ref('color.subtle'),'stroke':ref('color.subtle')}},
            {'when':{'State':'Unavailable'},'set':{'Title.color':ref('color.secondary'),'Description.text':'当前 Agent 不支持此连接','Enabled.props':{'State':'Disabled'}}}], 'Models'))
    defs.append(component('SkillRow',row('SkillRow',[icon('Icon','Book'),box('Copy',[text('Title','资料研究','body',strong=True),
        text('Description','共享 Skills 库 · 固定 v1','caption','secondary')],width='fill',gap=ref('space.1')),badge('Source','共享',visible=False),
        instance('Enabled','Switch',{'State':'On'},width=38,height=24),icon_button('More','More')],width=620,height='hug',padding=[ref('space.4'),0],
        fill=ref('color.surface'),stroke={'color':ref('color.border'),'width':[0,0,1,0]}),{'State':['Shared','Owned','Unavailable']},[
            {'when':{'State':'Owned'},'set':{'Source.Label.text':'伙伴自有','Description.text':'仅此伙伴 · v2 · 可编辑'}},
            {'when':{'State':'Unavailable'},'set':{'Description.text':'来源不可用 · 不会挂载','Enabled.props':{'State':'Disabled'}}}], 'Skills'))
    defs.append(component('ResourceRow',row('ResourceRow',[icon('Icon','File',color='accent'),box('Copy',[text('Title','工作台方案.md','body',strong=True),
        text('Description','当前项目 · 12 KB · 已保存','caption','secondary')],width='fill',gap=ref('space.1')),icon('Open','ChevronRight',small=True)],width=620,height='hug',padding=ref('space.3'),
        fill=ref('color.surface'),stroke=ref('color.border'),radius=ref('radius.ctrl')),
        {'Kind':['File','Directory','Link','Media']},[
            {'when':{'Kind':kind},'set':{'Icon.props':{'Name':symbol}}} for kind,symbol in [('File','File'),('Directory','Folder'),('Link','Link'),('Media','Image')]], 'Resources'))
    defs.append(component('TaskRow',row('TaskRow',[icon('Icon','Activity'),box('Copy',[text('Title','检查模型连接流程','body',strong=True),
        text('Description','研究伙伴 · ALVA · 12 秒','caption','secondary')],width='fill',gap=ref('space.1')),badge('Status','运行中','Accent'),icon_button('Action','ChevronRight')],
        width=620,height='hug',padding=ref('space.3'),fill=ref('color.surface'),radius=ref('radius.ctrl')),
        {'State':['Running','Waiting','Done','Failed','Cancelled']},[
            {'when':{'State':state},'set':{'Status.Label.text':label,'Status.props':{'Tone':tone},'Icon.props':{'Name':symbol}}}
            for state,label,tone,symbol in [('Waiting','等你确认','Warning','Help'),('Done','已完成','Success','Check'),('Failed','失败','Danger','Warning'),('Cancelled','已取消','Neutral','Stop')]], 'Execution'))
    defs.append(component('ToolActivity',row('ToolActivity',[icon('Icon','Code',small=True),text('Label','已读取 3 份资料','body','secondary'),icon('Chevron','ChevronDown',small=True)],
        width=440,height='hug',padding=ref('space.2'),fill=ref('color.transparent'),radius=ref('radius.ctrl')),
        {'State':['Collapsed','Running','Error']},[
            {'when':{'State':'Running'},'set':{'Icon.props':{'Name':'Loader'},'Label.text':'正在检查项目文件…'}},
            {'when':{'State':'Error'},'set':{'Icon.props':{'Name':'Warning'},'Label.color':ref('color.danger'),'Label.text':'工具执行失败 · 查看详情'}}], 'Conversation'))
    defs.append(component('QuestionOption',row('QuestionOption',[instance('Radio','Radio',{'State':'Off'},texts={'Label':'选项'},sets={'Label.visible':False},width=18,height=20),
        box('Copy',[text('Title','按推荐方案继续','body',strong=True),text('Description','说明选择的影响与代价','caption','secondary')],width='fill',gap=ref('space.1'))],
        width=540,height='hug',padding=ref('space.3'),fill=ref('color.surface'),stroke=ref('color.border'),radius=ref('radius.ctrl')),
        {'State':['Default','Selected','Disabled']},[
            {'when':{'State':'Selected'},'set':{'fill':ref('color.accentSoft'),'stroke':ref('color.accent'),'Radio.props':{'State':'On'}}},
            {'when':{'State':'Disabled'},'set':{'Radio.props':{'State':'Disabled'},'Title.color':ref('color.secondary')}}], 'Interaction'))
    defs.append(component('Approval',box('Approval',[row('Header',[icon('Icon','Shield',color='warning'),text('Title','需要你的许可','subtitle',strong=True),badge('Status','等待确认','Warning')]),
        text('Description','研究伙伴请求执行命令，影响范围如下。','body','secondary'),
        box('Effect',[text('Command','go test ./tests/...','body'),text('Scope','当前项目 · 运行命令','caption','secondary')],width='fill',padding=ref('space.3'),gap=8,fill=ref('color.subtle'),radius=ref('radius.ctrl')),
        row('Actions',[button('Deny','拒绝'),box('ActionSpace',width='fill',height=1),button('Allow','仅允许这一次','Primary')])],width=560,height='hug',padding=ref('space.4'),gap=ref('space.4'),
        fill=ref('color.surface'),stroke=ref('color.border'),radius=ref('radius.card')),
        {'State':['Pending','Allowed','Denied','Expired']},[
            {'when':{'State':s},'set':{'Status.Label.text':label,'Status.props':{'Tone':tone},'Actions.visible':False}}
            for s,label,tone in [('Allowed','已允许','Success'),('Denied','已拒绝','Neutral'),('Expired','请求已过期','Neutral')]], 'Interaction'))
    defs.append(component('Step',row('Step',[box('Number',[text('NumberLabel','1','body',strong=True,width='hug',textAlign='center')],width=28,height=28,align='center',justify='center',fill=ref('color.subtle'),radius=ref('radius.pill')),
        text('Label','选择供应商','body','secondary')],width=200,height='hug',padding=[8,0],fill=ref('color.surface')),{'State':['Current','Done','Next']},[
            {'when':{'State':'Current'},'set':{'Number.fill':ref('color.accent'),'NumberLabel.color':ref('color.onAccent'),'Label.color':ref('color.text')}},
            {'when':{'State':'Done'},'set':{'Number.fill':ref('color.successBg'),'NumberLabel.color':ref('color.success')}}], 'Navigation'))
    defs.append(component('DialogHeader',row('DialogHeader',[box('Copy',[text('Title','配置你的工作方式','heading',strong=True),text('Description','选择需要的选项，再保存到当前作用域。','caption','secondary')],
        width='fill',gap=8),icon_button('Close','Close')],width=640,height='hug',align='start',padding=[0,0,ref('space.4'),0]),category='Surfaces'))
    defs.append(component('DialogFooter',row('DialogFooter',[text('Scope','应用设置 · 下一轮生效','caption','secondary'),button('Cancel','取消'),button('Save','保存','Primary')],
        width=640,height='hug',justify='end',padding=[ref('space.4'),0,0,0],fill=ref('color.surface'),stroke={'color':ref('color.border'),'width':[1,0,0,0]}),category='Surfaces'))
    defs.append(component('Progress',box('Progress',[row('Copy',[text('Title','正在执行','body'),text('Percent','60%','caption','secondary',width='hug')]),
        row('Track',[box('Fill',width='fill',height=4,fill=ref('color.accent'),radius=2),box('Remaining',width=120,height=4,fill=ref('color.subtle'),radius=2)],height=4,gap=0)],
        width=360,height='hug',gap=ref('space.2')),{'State':['Running','Done','Error']},[
            {'when':{'State':'Done'},'set':{'Title.text':'已完成','Percent.text':'100%','Remaining.visible':False,'Fill.fill':ref('color.success')}},
            {'when':{'State':'Error'},'set':{'Title.text':'执行失败','Fill.fill':ref('color.danger')}}], 'Feedback'))
    defs.append(component('Skeleton',box('Skeleton',[box('LineOne',width='fill',height=12,fill=ref('color.subtle'),radius=4),
        box('LineTwo',width='fill',height=12,fill=ref('color.subtle'),radius=4),box('LineThree',width=180,height=12,fill=ref('color.subtle'),radius=4)],
        width=360,height='hug',gap=ref('space.3'),padding=ref('space.4'),fill=ref('color.surface')),category='Feedback'))
    def nav(name,label,symbol,selected=False,meta=''):
        return instance(name,'NavItem',{'State':'Selected' if selected else 'Default'},texts={'Label':label,'Meta':meta},
            sets={'Icon.props':{'Name':symbol}},width='fill',height=ref('control.row'))
    defs.append(component('Sidebar',box('Sidebar',[
        row('Brand',[box('BrandMark',[instance('Mark','Icon',{'Name':'Sparkles'},sets={'Glyph.text':'a','Glyph.fontFamily':ref('font.family'),
            'Glyph.fontSize':ref('font.size.subtitle'),'Glyph.fontWeight':ref('font.weight.strong'),'Glyph.color':ref('color.onAccent')},width=ref('icon.size'),height=ref('icon.size'))],
            width=32,height=32,align='center',justify='center',fill=ref('color.accent'),radius=ref('radius.ctrl')),
            text('BrandLabel','Alva','subtitle',strong=True)],height=44,padding=4),
        button('NewTask','新任务','Secondary',symbol='Plus',width='fill'),
        nav('Search','搜索任务与资料','Search'),nav('Partners','伙伴与群聊','Users'),nav('Automations','自动化','Calendar'),
        text('ProjectTitle','项目','caption','secondary',margin=[8,0,0,0]),
        nav('Project','alva-agent','Folder',meta=''),nav('CurrentThread','整理桌面工作台','Chat',True),
        nav('SecondThread','检查插件加载','Chat'),nav('AnotherProject','我的资料','Folder'),
        text('RecentTitle','最近','caption','secondary',margin=[8,0,0,0]),
        nav('RecentTask','整理模型连接','Clock'),nav('RecentResearch','本周资料研究','Clock'),
        box('SidebarSpace',width='fill',height='fill'),
        row('Footer',[instance('Owner','Avatar',{'Kind':'Person'},texts={'Label':'S'},width=28,height=28),
            text('OwnerName','Smallraw','body'),icon_button('OpenSettings','Settings')],padding=4),
    ],width=ref('size.sidebar'),height=960,**block('navigation',gap=ref('space.1')),fill=ref('color.subtle')),
        {'Area':['Work','Partners','Settings','New']},[
            {'when':{'Area':'Partners'},'set':{'CurrentThread.props':{'State':'Default'},'Partners.props':{'State':'Selected'}}},
            {'when':{'Area':'Settings'},'set':{'CurrentThread.props':{'State':'Default'},'OpenSettings.props':{'State':'Pressed'}}},
            {'when':{'Area':'New'},'set':{'CurrentThread.props':{'State':'Default'}}},
        ],'Shell'))
    categories=[('General','通用','Settings'),('Models','模型与连接','Chip'),('Plugins','插件与工具','Plugin'),('Skills','规则与 Skills','Book'),
        ('Storage','数据与存储','Database'),('Security','权限与安全','Shield'),('Gateway','Gateway','Globe'),('Developer','开发者','Code')]
    settings_nodes=[button('BackToWork','返回工作台','Ghost',symbol='ChevronLeft',width='hug'),
        text('Title','设置','heading',strong=True),text('Scope','应用设置','caption','secondary',visible=False)]
    for key,label,symbol in categories:
        settings_nodes.append(instance(key,'NavItem',{'State':'Default'},texts={'Label':label,'Meta':''},sets={'Icon.props':{'Name':symbol},'Label.visible':True,'Meta.visible':False},width='fill',height=ref('control.row')))
    settings_nodes.append(box('SettingsSpace',width='fill',height='fill'))
    defs.append(component('SettingsNav',box('SettingsNav',settings_nodes,width=ref('size.navigation'),height=960,padding=ref('space.4'),gap=ref('space.2'),fill=ref('color.canvas'),
        stroke=ref('color.transparent')),{'Category':[c[0] for c in categories]},[
            {'when':{'Category':key},'set':{key+'.props':{'State':'Selected'}}} for key,_,_ in categories], 'Shell'))
    defs.append(component('PageHeader',row('PageHeader',[row('Copy',[text('Title','整理桌面工作台','subtitle',strong=True,width='hug'),text('Description','alva-agent · 当前会话','caption','secondary')],width='fill',gap=ref('space.2')),
        icon_button('Panel','Panel'),icon_button('More','More')],width=900,height=ref('size.header'),padding=[0,ref('space.2'),0,ref('space.6')],fill=ref('color.surface')),category='Shell'))
    defs.append(component('PanelHeader',row('PanelHeader',[
        instance('FileTab','Tab',{'State':'Selected'},texts={'Label':'文件'},width='hug',height=ref('control.height')),
        instance('TaskTab','Tab',{'State':'Default'},texts={'Label':'任务'},sets={'Icon.props':{'Name':'Task'}},width='hug',height=ref('control.height')),
        icon_button('Add','Plus'),box('Spacer',width='fill',height=1),icon_button('Detach','External'),icon_button('Hide','Panel')],width=432,height=ref('size.panelHeader'),padding=[0,8],gap=ref('space.1'),
        fill=ref('color.transparent')),category='Shell'))
    defs.append(component('CompanionRow',row('CompanionRow',[instance('Avatar','Avatar',{'Kind':'Agent'},texts={'Label':'研'},width=32,height=32),
        box('Copy',[text('Title','研究伙伴','body',strong=True),text('Description','资料研究 · 2 项 Skills','caption','secondary')],width='fill',gap=ref('space.1'))],width=260,height='hug',padding=ref('space.3'),
        fill=ref('color.transparent'),stroke=ref('color.transparent'),radius=ref('radius.ctrl')),{'State':['Default','Selected','Paused','Error']},[
            {'when':{'State':'Selected'},'set':{'fill':ref('color.accentSoft'),'stroke':ref('color.accentSoft')}},
            {'when':{'State':'Paused'},'set':{'Description.text':'已暂停 · 可以恢复'}},
            {'when':{'State':'Error'},'set':{'Description.text':'运行失败 · 查看详情','Description.color':ref('color.danger')}}], 'Companions'))
    defs.append(component('MessageHeader',row('MessageHeader',[instance('Avatar','Avatar',{'Kind':'Agent'},texts={'Label':'A'},width=28,height=28),
        text('Author','Alva','body',strong=True,width='hug'),text('Model','GPT · ALVA','caption','secondary'),icon_button('More','More')],width=620,height='hug'),
        {'Author':['Assistant','User','Partner']},[
            {'when':{'Author':'User'},'set':{'Author.text':'你','Avatar.Label.text':'S','Avatar.props':{'Kind':'Person'},'Model.text':'刚刚'}},
            {'when':{'Author':'Partner'},'set':{'Author.text':'研究伙伴','Avatar.Label.text':'研'}}], 'Conversation'))
    defs.append(component('Question',box('Question',[
        row('Header',[icon('Icon','Help',color='accent'),text('Title','接下来按哪种方式整理？','subtitle',strong=True),badge('Status','等待回答','Accent')]),
        text('Description','选择一项，或在 Other 中填写你的方案。','caption','secondary',visible=False),
        instance('First','QuestionOption',{'State':'Selected'},texts={'Title':'先完成模型与连接（推荐）','Description':'先具备可用模型，再扩展伙伴与群聊'},width='fill',height='hug'),
        instance('Second','QuestionOption',{'State':'Default'},texts={'Title':'先整理伙伴与 Skills','Description':'优先确认长期伙伴的身份与能力'},width='fill',height='hug'),
        instance('Third','QuestionOption',{'State':'Default'},texts={'Title':'两部分一起规划','Description':'统一设计，分别推进实现'},width='fill',height='hug'),
        field('Other','其他方案','写下你的选择…'),
        row('Footer',[text('Scope','仅回答此问题，不授予执行权限','caption','secondary'),button('Submit','提交回答','Primary')]),
    ],width=620,height='hug',padding=ref('space.4'),gap=ref('space.3'),fill=ref('color.surface'),stroke=ref('color.border'),radius=ref('radius.card')),
        {'State':['Pending','Submitted','Expired']},[
            {'when':{'State':s},'set':{'Status.Label.text':label,'Footer.visible':False,'Other.visible':False}}
            for s,label in [('Submitted','已提交'),('Expired','问题已过期')]], 'Interaction'))
    defs.append(component('Segment',row('Segment',[text('Label','全部','body',width='hug',textAlign='center')],width='hug',height=32,justify='center',padding=[0,12],
        fill=ref('color.subtle'),stroke=ref('color.subtle'),radius=ref('radius.ctrl')),{'State':['Default','Selected','Disabled']},[
            {'when':{'State':'Selected'},'set':{'fill':ref('color.surface'),'stroke':ref('color.border'),'Label.color':ref('color.accent')}},
            {'when':{'State':'Disabled'},'set':{'Label.color':ref('color.secondary')}}], 'Selection'))
    defs.append(component('Pagination',row('Pagination',[text('Summary','已显示 1–20 项','caption','secondary'),icon_button('Previous','ChevronLeft'),icon_button('Next','ChevronRight')],
        width=620,height='hug',padding=[ref('space.3'),0]),{'State':['Ready','Loading','End']},[
            {'when':{'State':'Loading'},'set':{'Summary.text':'正在加载下一页…','Next.props':{'State':'Loading'}}},
            {'when':{'State':'End'},'set':{'Summary.text':'已显示全部结果','Next.props':{'State':'Disabled'}}}], 'Navigation'))
    defs.append(component('Toast',row('Toast',[icon('Icon','Check',color='success'),text('Label','设置已保存，下一轮生效','body'),icon_button('Close','Close')],
        width=420,height='hug',padding=ref('space.3'),fill=ref('color.surface'),stroke=ref('color.border'),radius=ref('radius.ctrl'),shadow=ref('shadow.popup')),
        {'Tone':['Success','Error','Info']},[
            {'when':{'Tone':'Error'},'set':{'Icon.props':{'Name':'Warning'},'Icon.Glyph.color':ref('color.danger'),'Label.text':'保存失败，草稿已保留'}},
            {'when':{'Tone':'Info'},'set':{'Icon.props':{'Name':'Info'},'Icon.Glyph.color':ref('color.secondary')}}], 'Feedback'))
    defs.append(component('TreeRow',row('TreeRow',[icon('Expand','ChevronRight',small=True),icon('Icon','Folder',small=True),text('Label','docs','body'),text('Meta','3','caption','secondary',width='hug')],
        width=300,height=32,padding=[0,8],fill=ref('color.surface'),stroke=ref('color.transparent'),radius=ref('radius.small')),{'Kind':['Folder','File'],'State':['Default','Selected','Focus','SelectedFocus','Unavailable']},[
            {'when':{'Kind':'File'},'set':{'Expand.visible':False,'Icon.props':{'Name':'File'},'Label.text':'ARCHITECTURE.md','Meta.visible':False}},
            *[{'when':{'State':s},'set':{'fill':ref('color.accentSoft'),'Label.color':ref('color.accent')}} for s in ['Selected','SelectedFocus']],
            *[{'when':{'State':s},'set':{'stroke':{'color':ref('color.accent'),'width':ref('border.focus')}}} for s in ['Focus','SelectedFocus']],
            {'when':{'State':'Unavailable'},'set':{'Label.color':ref('color.secondary'),'Icon.Glyph.color':ref('color.secondary')}},
        ],'Resources','Kind and selection/focus are independent. Parent owns indentation, expanded children and roving focus; expanded folders override Expand to ChevronDown. No arbitrary-depth tree or virtualization is claimed.'))
    defs.append(component('ModelForm',box('ModelForm',[
        row('Identity',[field('ModelID','模型 ID','填写准确的模型 ID',hint='按模型服务要求填写。'),field('DisplayName','显示名称','我的模型')],align='start'),
        row('Limits',[field('Purpose','模型用途','对话',kind='Select'),field('Context','Context 上限 · token','未声明')]),
        box('Capabilities',[
            text('CapabilityTitle','能力声明','subtitle',strong=True),
            row('CapabilityFields',[
                instance('Vision','SettingRow',{'Control':'Switch'},texts={'Title':'视觉输入','Description':'按服务说明填写。'},sets={'fill':ref('color.transparent')},width='fill',height='hug'),
                instance('Tools','SettingRow',{'Control':'Switch'},texts={'Title':'工具调用','Description':'模型和 Agent 都需要支持。'},sets={'fill':ref('color.transparent')},width='fill',height='hug'),
            ],align='start'),
        ],width='fill',height='hug',padding=0,gap=ref('space.3'),fill=ref('color.transparent'),radius=ref('radius.ctrl')),
        row('Defaults',[field('Reasoning','思考程度默认值','跟随模型默认',kind='Select'),field('Visibility','对话选择器','启用',kind='Select')]),
        notice('Metadata','声明不创造模型能力','价格与限制未提供时显示未知，不按模型名字推断。',visible=False),
    ],width=672,height='hug',gap=ref('space.4'),fill=ref('color.surface')),{'Mode':['Add','Edit']},[
        {'when':{'Mode':'Edit'},'set':{
            'Identity.ModelID.props':{'State':'ReadOnly'},'Identity.ModelID.Control.props':{'Kind':'Text','State':'ReadOnly'},'Identity.ModelID.Control.Value.text':'model-id-1',
            'Identity.ModelID.Hint.text':'已有模型的身份保持不变。','Identity.DisplayName.Control.Value.text':'GPT',
            'Limits.Context.Control.Value.text':'128000','Capabilities.Vision.Control.props':{'State':'On'},'Capabilities.Tools.Control.props':{'State':'On'},
        }},
    ],'Models','One model form for add/edit; stable identity is read-only when editing.'))
    complete_common_states(defs)
    defs.extend(content_definitions())
    # Contextual editor surfaces share controls; lifecycle states do not multiply
    # permission/model values or popover visibility into a Cartesian product.
    select_option=next(d for d in defs if d['name']=='Alva SelectOption')
    option_icon=icon('Icon','Shield',small=True)
    option_icon['visible']=False
    select_option['base']['children'].insert(0,option_icon)
    from composer_states import definitions as composer_definitions
    defs.extend(composer_definitions())
    defs.extend(provider_definitions())
    for definition in defs:
        name=definition['name'].removeprefix('Alva ')
        recipe=('list' if name in ('MenuItem','ConnectionRow','ModelRow','ResourceRow','TaskRow','CompanionRow') else
            'navigation' if name in ('Sidebar','SettingsNav') else
            'card' if name in ('Approval','Question') else None)
        if recipe:definition['base'].update(block(recipe))
        # Menus keep the shared vertical rhythm without the wider content-list gutter.
        if name=='MenuItem':definition['base']['padding']=[ref('space.1'),ref('space.2')]
    return defs


def crosses_instance(base,path):
    names=path.split('.')[:-1]
    def find(node,name):
        if node['name']==name:return node
        if node.get('use'):return None
        for child in node.get('children',[]):
            found=find(child,name)
            if found:return found
        return None
    node=base
    for index,name in enumerate(names):
        node=find(node,name)
        if not node:return False
        if node.get('use') and index<len(names)-1:return True
    return False
