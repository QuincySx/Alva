# INPUT: Shared Alva controls, the editor references and explicit execution/permission boundaries
# OUTPUT: Bounded Composer, voice, run-status and contextual-menu component definitions
# POS: SmallPen authoring only; state samples describe the runtime contract, not live task execution.
from design_system import box, row, text, icon, button, icon_button, instance, component, ref, block, popover
from copy import deepcopy


PERMISSION_HELP = {
    'Ask': '需要权限时先询问；仅批准本次所示操作。',
    'Auto': '工作区内写文件与安全命令自动执行；越界写、危险命令和 MCP 工具仍会询问。',
    'Full': '允许当前任务在所授范围内执行；沙箱与组织限制仍有效。',
}


def align_geometry(variant, sets, token_map, package):
    """Local Composer geometry; keep generic controls and every state selection."""
    def binding(role):
        return {'packageId': package, 'assetId': token_map['ui.' + role]['id']}

    def selected(node):
        component = sets[node['instance']['component']['assetId']]
        return next(v for v in component['variants'] if v['selection'] == node['instance']['variant'])

    for node in variant['nodes'].values():
        if node['name'] == 'Editor':
            node['layout-padding'] = {side: 8 for side in ('p1', 'p2', 'p3', 'p4')}
            for side in ('Top', 'Right', 'Bottom', 'Left'):
                node['tokenBindings']['padding' + side] = binding('space.2')
            # The writing surface stays in the page plane; child state variants
            # own disabled appearance because setup triggers can remain active.
            node.pop('shadow', None)
            node.get('tokenBindings', {}).pop('shadow', None)
        if node['name'] not in ('Attach', 'Permission', 'Model', 'Voice', 'Send'):
            continue
        node['height'] = 32
        node['tokenBindings']['height'] = binding('control.icon')
        source = selected(node)
        overrides = node['instance'].setdefault('overrides', {})
        root = source['nodes'][source['rootId']]
        root_bindings = deepcopy(root.get('tokenBindings', {}))
        root_bindings['height'] = binding('control.icon')
        overrides[root['id'] + ':height'] = 32
        if node['name'] == 'Send':
            node['width'] = 32
            node['tokenBindings']['width'] = binding('control.icon')
            root_bindings['width'] = binding('control.icon')
            root_bindings['cornerRadius'] = binding('radius.card')
            overrides[root['id'] + ':width'] = 32
            overrides[root['id'] + ':cornerRadius'] = 16
        elif node['name'] == 'Voice':
            transcription = next(n for n in source['nodes'].values() if n['name'] == 'Transcription')
            overrides[transcription['id'] + ':height'] = 32
            overrides[transcription['id'] + ':tokenBindings'] = deepcopy(transcription['tokenBindings']) | {'height': binding('control.icon')}
            button_variant = selected(transcription)
            button_root = button_variant['nodes'][button_variant['rootId']]
            key = transcription['id'] + '__' + button_root['id']
            # Nested component roots are replaced by the instance in Product
            # expansion; constrain the Transcription instance itself instead.
            overrides.pop(key + ':height', None)
            overrides.pop(key + ':tokenBindings', None)
        icon_sizes = ({'Icon': 18, 'Spinner': 18} if node['name'] in ('Attach', 'Send') else
                      {'Trailing': 14} if node['name'] in ('Permission', 'Model') else {})
        for child in source['nodes'].values():
            if child['name'] not in icon_sizes:
                continue
            size = icon_sizes[child['name']]
            overrides[child['id'] + ':width'] = size
            overrides[child['id'] + ':height'] = size
            overrides[child['id'] + ':tokenBindings'] = {}
            glyph_variant = selected(child)
            glyph = next(n for n in glyph_variant['nodes'].values() if n['name'] == 'Glyph')
            key = child['id'] + '__' + glyph['id']
            child_overrides = child['instance'].get('overrides', {})
            style = deepcopy(child_overrides.get(glyph['id'] + ':textStyle', glyph['textStyle']))
            style['fontSize'] = size
            glyph_bindings = deepcopy(child_overrides.get(glyph['id'] + ':tokenBindings', glyph.get('tokenBindings', {})))
            glyph_bindings.pop('fontSize', None)
            overrides[key + ':textStyle'] = style
            overrides[key + ':tokenBindings'] = glyph_bindings
        overrides[root['id'] + ':tokenBindings'] = root_bindings


def definitions():
    definitions = []
    voice = component('VoiceInput', row('VoiceInput', [
        icon_button('Microphone', 'Microphone'),
        row('Record', [box('Dot', width=8, height=8, fill=ref('color.danger'), radius=ref('radius.pill')),
            text('Timer', '0:03', width='hug'), icon('StopRecord', 'Stop', small=True)],
            width='hug', height=ref('control.icon'), padding=[0, 12], gap=8,
            justify='center', fill=ref('color.subtle'), stroke=ref('color.transparent'), radius=ref('radius.pill'), visible=False),
        button('Transcription', '转写中', 'Ghost', content='Leading', state='Loading', visible=False),
        icon_button('Retry', 'Refresh'),
    ], width='hug', height=ref('control.height'), justify='center', gap=0),
        {'State': ['Idle', 'Hover', 'Focus', 'Recording', 'RecordingHover', 'Transcribing', 'Error', 'Disabled']}, [
            {'when': {}, 'set': {'Retry.visible': False}},
            *[{'when': {'State': state}, 'set': {'Microphone.props': {'State': state}}} for state in ['Hover', 'Focus', 'Disabled']],
            *[{'when': {'State': state}, 'set': {'Microphone.visible': False, 'Record.visible': True}} for state in ['Recording', 'RecordingHover']],
            {'when': {'State': 'RecordingHover'}, 'set': {'Record.fill': ref('color.hover')}},
            {'when': {'State': 'Transcribing'}, 'set': {'Microphone.visible': False, 'Transcription.visible': True}},
            {'when': {'State': 'Error'}, 'set': {'Microphone.visible': False, 'Retry.visible': True, 'Retry.Icon.Glyph.color': ref('color.danger')}},
        ], 'Conversation', 'Recording stops capture, never the task. Timer is content. Transcription preserves the existing draft; retry follows failure only. Microphone denial is explained by a tooltip/status. Runtime owns capture, timer and transcription.')
    definitions.append(voice)
    definitions.append(component('RunStatus', row('RunStatus', [icon('Activity', 'Sparkles', small=True, color='accent'),
        text('Label', '正在工作…', color='accent', strong=True),
        text('Metrics', '', 'caption', 'secondary', width='hug', visible=False),
        button('Retry', '重试', 'Ghost', symbol='Refresh', visible=False)],
        width=720, height='hug', padding=[0, 8], gap=8),
        {'State': ['Working', 'Waiting', 'Stopping', 'Stopped', 'Done', 'Error']}, [
            {'when': {'State': 'Working'}, 'set': {'Activity.props': {'Name': 'Loader'}, 'Label.color': ref('color.secondary'), 'Label.fontWeight': ref('font.weight.regular')}},
            {'when': {'State': 'Waiting'}, 'set': {'Activity.props': {'Name': 'Help'}, 'Label.text': '等待你的确认', 'Label.color': ref('color.warning'), 'Metrics.visible': False}},
            {'when': {'State': 'Stopping'}, 'set': {'Activity.props': {'Name': 'Loader'}, 'Label.text': '正在停止…', 'Metrics.visible': False}},
            {'when': {'State': 'Stopped'}, 'set': {'Activity.props': {'Name': 'Stop'}, 'Label.text': '已停止', 'Label.color': ref('color.secondary'), 'Metrics.visible': False}},
            {'when': {'State': 'Done'}, 'set': {'Activity.props': {'Name': 'Check'}, 'Label.text': '已完成', 'Label.color': ref('color.success')}},
            {'when': {'State': 'Error'}, 'set': {'Activity.props': {'Name': 'Warning'}, 'Label.text': '执行失败', 'Label.color': ref('color.danger'), 'Metrics.visible': False, 'Retry.visible': True}},
        ], 'Execution', 'Independent task status. Show metrics only when measured; elapsed time and rate are optional content. Stop requests cancellation; Stopped requires actual owner cleanup. Retry is explicit, never automatic replay. Reduced motion keeps readable status.'))
    permission_rules = []
    for mode in ['Ask', 'Auto', 'Full']:
        for key in ['Ask', 'Auto', 'Full']:
            permission_rules.append({'when': {'Mode': mode}, 'set': {key+'.props': {'State': 'Selected' if key == mode else 'Default'},
                key+'.Copy.Label.color': ref('color.accent' if key == mode else 'color.text'),
                key+'.Icon.Glyph.color': ref('color.accent' if key == mode else 'color.secondary')}})
    definitions.append(component('PermissionMenu', popover('PermissionMenu', [
        instance(key, 'SelectOption', {'State': 'Selected' if key == 'Ask' else 'Default'},
            texts={'Copy.Label': label}, sets={'Icon.visible': True, 'Icon.props': {'Name': symbol}}, width='fill', height='hug')
        for key, label, symbol in [('Ask', '每次询问', 'Shield'), ('Auto', '自动审批', 'Sparkles'), ('Full', '完全访问', 'Warning')]
    ], width=240, height='hug'),
        {'Mode': ['Ask', 'Auto', 'Full']}, permission_rules, 'Conversation',
        'Single-select task permission menu. Selection updates the trigger and closes the menu. Full requires an explicit scope confirmation before becoming effective. This design grants no permission. Hover/focus explanation: '+str(PERMISSION_HELP)))

    def item(name, label, symbol, state='Default'):
        return instance(name, 'MenuItem', {'State': state}, texts={'Label': label, 'Shortcut': ''},
            sets={'Copy.Description.visible': False, 'Shortcut.visible': False, 'Icon.props': {'Name': symbol}}, width='fill', height='hug')
    def separator(name):
        return box(name, width='fill', height=1, fill=ref('color.border'))
    menu = component('ComposerMenu', row('ComposerMenu', [
        popover('List', [
            box('Options', [item('First', '文件、图片或视频…', 'Paperclip', 'Hover'), item('Second', '新建目标', 'Target'),
                item('Third', '计划模式', 'List'), item('Fourth', '协同模式', 'Users'),
                separator('Divider'), item('Fifth', '添加目录…', 'Folder')], width='fill', height='hug', gap=0),
            row('Message', [icon('MessageIcon', 'Search'), text('MessageText', '没有匹配的结果', color='secondary')],
                padding=ref('space.2'), visible=False),
            separator('FooterLine'), text('Query', '输入 @ 搜索文件、任务或伙伴', 'caption', 'secondary', margin=[ref('space.1'), ref('space.2')]),
        ], width='fill', height='hug'),
        popover('Detail', [row('DetailHeader', [text('Title', 'plan', 'subtitle', strong=True), icon_button('OpenDetail', 'External')]),
            text('Description', '阅读项目资料，整理计划；确认后执行。', color='secondary')],
            width='fill', height='hug', padding=ref('space.3'), gap=ref('space.2'), visible=False),
    ], width=352, height='hug', align='start', gap=8), {'View': ['Add', 'References', 'Commands', 'Empty', 'Loading', 'Error']}, [
        {'when': {'View': 'References'}, 'set': {
            'First.Copy.Label.text': 'ARCHITECTURE.md', 'First.Icon.props': {'Name': 'File'},
            'Second.Copy.Label.text': '模型连接流程', 'Second.Icon.props': {'Name': 'Activity'},
            'Third.Copy.Label.text': '研究伙伴', 'Third.Icon.props': {'Name': 'Users'},
            'Fourth.visible': False, 'Fifth.visible': False, 'Divider.visible': False, 'Query.text': '@ 搜索当前项目'}},
        {'when': {'View': 'Commands'}, 'set': {'width': 640, 'Detail.visible': True,
            'First.Copy.Label.text': 'plan', 'First.Icon.props': {'Name': 'List'},
            'Second.Copy.Label.text': 'review', 'Second.Icon.props': {'Name': 'Code'},
            'Third.Copy.Label.text': '文档整理', 'Third.Icon.props': {'Name': 'File'},
            'Fourth.Copy.Label.text': '资料研究', 'Fourth.Icon.props': {'Name': 'Search'},
            'Fifth.visible': False, 'Divider.visible': False, 'Query.text': '/ 搜索命令与 Skills'}},
        *[{'when': {'View': view}, 'set': {'Options.visible': False, 'Message.visible': True, 'MessageIcon.props': {'Name': symbol},
            'MessageText.text': label, 'Query.text': query}} for view, symbol, label, query in [
                ('Empty', 'Search', '没有匹配的结果', '换个关键词试试'),
                ('Loading', 'Loader', '正在查找…', '@ 搜索当前项目'),
                ('Error', 'Warning', '读取失败，重新尝试', '草稿与已选引用仍保留')]],
    ], 'Conversation', 'One editor popover for +, @ and /. List selection drives the detail preview; samples are not installed capabilities. Runtime filters, scrolls and inserts only on confirmation. Up/Down selects, Enter inserts, Esc closes and returns focus. Constrain to viewport; below 480px stack detail below selection. No global permission is granted by attaching a directory.')
    definitions.append(menu)

    permission = button('Permission', '每次询问', 'Ghost', symbol='Shield')
    model = button('Model', 'GPT · ALVA', 'Ghost', symbol='Chip')
    for trigger in [permission, model]:
        trigger.setdefault('set', {})['Trailing.visible'] = True
    editor = box('Editor', [
        instance('Attachments', 'Attachment', {'State': 'Ready'}, width='fill', height='hug', visible=False),
        text('Prompt', '说明你要完成的事，@ 引用文件或伙伴', 'subtitle', 'secondary', lineHeight=ref('line.reading')),
        # Prompt's 28px line + 8px gap + 20px reserve = two 28px lines.
        box('Space', width='fill', height=20),
        row('Controls', [icon_button('Attach', 'Plus'), permission, box('ControlSpace', width='fill', height=1), model,
            instance('Voice', 'VoiceInput', {'State': 'Idle'}, width='hug', height=ref('control.height')),
            button('Send', '发送', 'Primary', state='Disabled', symbol='ArrowUp', content='Icon')]),
    ], width='fill', height='hug', fill=ref('color.surface'), stroke=ref('color.border'), **block('composer'))
    states = ['Idle', 'Focus', 'Ready', 'Attaching', 'Submitting', 'Running', 'Waiting', 'Stopping', 'Stopped', 'Complete', 'Failed', 'Recording', 'Transcribing', 'Disabled']
    rules = [
        {'when': {'State': 'Focus'}, 'set': {'Editor.stroke': ref('color.accent')}},
        {'when': {'State': 'Ready'}, 'set': {'Prompt.text': '梳理当前项目的模型连接流程。', 'Prompt.color': ref('color.text'), 'Send.props': {'State': 'Default'}}},
        {'when': {'State': 'Attaching'}, 'set': {'Attachments.visible': True, 'Attachments.props': {'State': 'Uploading'}, 'Prompt.text': '等附件准备好后发送…'}},
        {'when': {'State': 'Submitting'}, 'set': {'Prompt.text': '梳理当前项目的模型连接流程。', 'Prompt.color': ref('color.text'), 'Send.props': {'State': 'Loading'}, 'Voice.props': {'State': 'Disabled'}, 'Attach.props': {'State': 'Disabled'}}},
        {'when': {'State': 'Running'}, 'set': {'Status.visible': True, 'Prompt.text': '写下下一步…', 'Send.props': {'State': 'Default'}, 'Send.Icon.props': {'Name': 'Stop'}, 'Send.Label.text': '停止任务'}},
        {'when': {'State': 'Waiting'}, 'set': {'Status.visible': True, 'Status.props': {'State': 'Waiting'}, 'Prompt.text': '先完成上方提问或审批', 'Send.Icon.props': {'Name': 'Pause'}}},
        {'when': {'State': 'Stopping'}, 'set': {'Status.visible': True, 'Status.props': {'State': 'Stopping'}, 'Prompt.text': '补充内容会留在草稿中…', 'Send.props': {'State': 'Loading'}, 'Send.Label.text': '正在停止'}},
        {'when': {'State': 'Stopped'}, 'set': {'Status.visible': True, 'Status.props': {'State': 'Stopped'}, 'Prompt.text': '继续输入，开始下一轮…'}},
        {'when': {'State': 'Complete'}, 'set': {'Status.visible': True, 'Status.props': {'State': 'Done'}, 'Prompt.text': '继续聊聊…'}},
        {'when': {'State': 'Failed'}, 'set': {'Status.visible': True, 'Status.props': {'State': 'Error'}, 'Status.Label.text': '发送失败，草稿已保留', 'Prompt.text': '梳理当前项目的模型连接流程。', 'Prompt.color': ref('color.text'), 'Send.props': {'State': 'Default'}}},
        {'when': {'State': 'Recording'}, 'set': {'Voice.props': {'State': 'Recording'}, 'Prompt.text': '正在听…'}},
        {'when': {'State': 'Transcribing'}, 'set': {'Voice.props': {'State': 'Transcribing'}, 'Prompt.text': '正在转写，原有草稿会保留…'}},
        {'when': {'State': 'Disabled'}, 'set': {'Editor.fill': ref('color.canvas'), 'Prompt.text': '配置连接后即可开始', 'Voice.props': {'State': 'Disabled'}, 'Attach.props': {'State': 'Disabled'}}},
    ]
    # A high-level instance props replacement resets omitted axes. Keep the
    # action intent and Icon content explicit when changing Send's state.
    for rule in rules:
        if 'Send.props' in rule['set']:
            rule['set']['Send.props']={'Kind':'Primary','Content':'Icon',**rule['set']['Send.props']}
    definitions.append(component('Composer', box('Composer', [
        instance('Status', 'RunStatus', {'State': 'Working'}, width='fill', height='hug', visible=False), editor,
    ], width=720, height='hug', gap=8), {'State': states}, rules, 'Conversation',
        '14 visual samples, not one exclusive runtime enum. Draft, run, voice and overlay states are independent; running plus recording must retain task Stop. Failed represents a rejected submission with its draft retained; execution failure belongs to RunStatus Error. Permission/model are instance configuration. Runtime owns editable text, IME, keyboard behavior and motion; see Product requirements/home-composer.json.'))
    return definitions
