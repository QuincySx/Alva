# INPUT: Saved Foundation/Product and the authorized native-inset shell contract.
# OUTPUT: Scoped public CLI batches and an isolated two-state shell review copy.
# POS: Design-only header/sidebar/return migration; never writes canonical JSON directly.
from copy import deepcopy
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'exports/archive/native-shell-2026-11-05'
WORK = Path('/tmp/alva-native-shell-20261105')
SOURCE_REVISIONS = {
    'foundation': '843480b8e9e0ca197c929aff59707cb88c3f0d7d601d302cdb1bbf468a35349d',
    'product': '0c285b1d5e710f87559a6b84c6bb8b312ba1cbe26a73981a6ed23e8e488411f0',
}


def read(path):
    return json.loads(path.read_text())


def show(path):
    return json.loads(subprocess.check_output(['smallpen', 'project', 'show', str(path), '--json']))


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def prepare():
    for role, name in [('foundation', 'alva-foundation.smallpen'), ('product', 'alva.smallpen')]:
        if show(ROOT / name)['revision'] != SOURCE_REVISIONS[role]:
            raise ValueError('This one-time migration has a different source revision; inspect current definitions instead of rerunning it.')
    OUT.mkdir(exist_ok=True)
    WORK.mkdir(exist_ok=True)
    for name in ('alva.smallpen', 'alva-foundation.smallpen'):
        if not (WORK / name).exists():
            shutil.copytree(ROOT / name, WORK / name)
    foundation = ROOT / 'alva-foundation.smallpen'
    product = ROOT / 'alva.smallpen'
    f_revision, p_revision = show(foundation)['revision'], show(product)['revision']
    components = read(foundation / 'components/components.json')['componentSets']
    sets = read(foundation / 'tokens/tokens.json')['sets']
    pkg = read(foundation / 'manifest.json')['packageId']
    tokens = {t['name']: t for s in sets if s['name'] in ('Theme/Default', 'Appearance/Light') for t in s['tokens']}

    def binding(name):
        return {'packageId': pkg, 'assetId': tokens['ui.' + name]['id']}

    def dimension(node, field, name, value):
        node[field] = value
        node.setdefault('tokenBindings', {})[field] = binding(name)

    def gap(node, value=8):
        node['layout-gap'] = {'row-gap': value, 'column-gap': value}
        node.setdefault('tokenBindings', {})['itemSpacing'] = binding('space.2')

    def padding(node, values):
        node['layout-padding-type'] = 'multiple'
        node['layout-padding'] = dict(zip(('p1', 'p2', 'p3', 'p4'), values))
        for side, value in zip(('Top', 'Right', 'Bottom', 'Left'), values):
            key = 'padding' + side
            node.setdefault('tokenBindings', {}).pop(key, None)
            if value in (12, 24):
                node['tokenBindings'][key] = binding('space.' + ('3' if value == 12 else '6'))

    def frame(ident, name, width='fill', height=32):
        return {'id': ident, 'name': name, 'type': 'FRAME', 'x': 0, 'y': 0,
                'width': 1 if width == 'fill' else width, 'height': height,
                'layout-item-h-sizing': 'fill' if width == 'fill' else 'fix',
                'layout-item-v-sizing': 'fix', 'children': [], 'fills': []}

    by = {c['name']: c for c in components}
    icon = deepcopy(by['Alva Icon'])
    panel = next(v for v in icon['variants'] if v['selection']['axis_name'] == 'Panel')
    left = deepcopy(panel)
    left['id'] = 'var_alva_icon_panelleft'
    left['selection'] = {'axis_name': 'PanelLeft'}
    next(n for n in left['nodes'].values() if n['name'] == 'Glyph')['flipX'] = True
    icon['axes'][0]['domain'].append('PanelLeft')
    icon['variants'].append(left)

    header = deepcopy(by['Alva PageHeader'])
    header['axes'] = [{'id': 'axis_navigation', 'name': 'Navigation',
                       'domain': ['Expanded', 'Collapsed'], 'role': 'configuration'}]
    original_header = header['variants'][0]
    base_nodes = original_header['nodes']
    original_panel = next(n for n in base_nodes.values() if n['name'] == 'Panel')

    def toggle(ident):
        node = deepcopy(original_panel)
        node['id'], node['name'] = ident, 'SidebarToggle'
        node['instance']['overrides'] = {
            'node_alva_iconbutton_iconbutton_icon:variant': {'axis_name': 'PanelLeft'}}
        dimension(node, 'width', 'control.icon', 32)
        dimension(node, 'height', 'control.icon', 32)
        return node

    root = base_nodes[original_header['rootId']]
    dimension(root, 'height', 'control.height', 36)
    padding(root, [0, 24, 0, 24])
    gap(root)
    root['layout-align-items'] = 'center'
    copy = next(n for n in base_nodes.values() if n['name'] == 'Copy')
    copy['layout-flex-dir'] = 'row'
    copy['layout-align-items'] = 'center'
    gap(copy)
    title = next(n for n in base_nodes.values() if n['name'] == 'Title')
    title['layout-item-h-sizing'] = 'fix'
    title['growType'] = 'auto-width'
    desc = next(n for n in base_nodes.values() if n['name'] == 'Description')
    desc['layout-item-h-sizing'] = 'fill'
    reserve = frame('node_alva_pageheader_native_safe_area', 'NativeSafeArea', 50)
    toggle_node = toggle('node_alva_pageheader_sidebar_toggle')
    base_nodes[reserve['id']] = reserve
    base_nodes[toggle_node['id']] = toggle_node
    root['children'].extend([toggle_node['id'], reserve['id']])
    original_header['selection'] = {'axis_navigation': 'Expanded'}
    reserve['visible'], toggle_node['visible'] = False, False
    collapsed = deepcopy(original_header)
    collapsed['id'] = 'var_alva_pageheader_collapsed'
    collapsed['selection'] = {'axis_navigation': 'Collapsed'}
    collapsed['nodes'][reserve['id']]['visible'] = True
    collapsed['nodes'][toggle_node['id']]['visible'] = True
    header['variants'].append(collapsed)

    sidebar = deepcopy(by['Alva Sidebar'])
    for variant in sidebar['variants']:
        nodes = variant['nodes']
        root = nodes[variant['rootId']]
        padding(root, [0, 12, 12, 12])
        brand = next(n for n in nodes.values() if n['name'] == 'Brand')
        dimension(brand, 'height', 'control.height', 36)
        padding(brand, [0, 0, 0, 0])
        for child in brand['children']:
            nodes[child]['visible'] = False
            nodes[child].get('tokenBindings', {}).pop('visible', None)
        reserve = frame('node_alva_sidebar_native_safe_area', 'NativeSafeArea', 62)
        spacer = frame('node_alva_sidebar_top_spacer', 'TopSpacer')
        control = toggle('node_alva_sidebar_sidebar_toggle')
        nodes.update({n['id']: n for n in (reserve, spacer, control)})
        brand['children'] = [control['id'], spacer['id'], *brand['children'], reserve['id']]
        gap(brand)

    settings = deepcopy(by['Alva SettingsNav'])
    original_return = None
    for variant in settings['variants']:
        nodes = variant['nodes']
        root = nodes[variant['rootId']]
        padding(root, [0, 12, 12, 12])
        back = next(n for n in nodes.values() if n['name'] == 'BackToWork')
        original_return = original_return or deepcopy(back)
        back['visible'] = False
        title = next(n for n in nodes.values() if n['name'] == 'Title')
        title['textStyle']['fontSize'] = 16
        title['tokenBindings']['fontSize'] = binding('font.size.subtitle')
        title['height'] = 24
        title['layout-item-h-sizing'] = 'fix'
        title['growType'] = 'auto-width'
        reserve = frame('node_alva_settingsnav_native_safe_area', 'NativeSafeArea', 62)
        spacer = frame('node_alva_settingsnav_top_spacer', 'TopSpacer')
        top = frame('node_alva_settingsnav_top_row', 'TopRow', height=36)
        top.update({'layout': 'flex', 'layout-flex-dir': 'row', 'layout-align-items': 'center'})
        dimension(top, 'height', 'control.height', 36)
        gap(top)
        top['children'] = [spacer['id'], title['id'], reserve['id']]
        nodes.update({n['id']: n for n in (reserve, spacer, top)})
        root['children'] = [i for i in root['children'] if i != title['id']]
        root['children'].append(top['id'])

    f_ops = [{'type': 'put-component-set', 'componentSet': c} for c in (icon, sidebar, header, settings)]
    for token_set in sets:
        if token_set['name'] == 'Viewport/Narrow':
            token_set = deepcopy(token_set)
            next(t for t in token_set['tokens'] if t['name'] == 'ui.size.sidebar')['value'] = 220
            next(t for t in token_set['tokens'] if t['name'] == 'ui.view.sidebarLabels')['value'] = True
            next(t for t in token_set['tokens'] if t['name'] == 'ui.size.navigation')['value'] = 208
            f_ops.append({'type': 'put-token-set', 'set': token_set})

    p_ops, moved, affected = [], {}, []
    for path in sorted((product / 'screens').glob('*.json')):
        screen = read(path)
        modified = False
        for pres in screen['presentations']:
            nodes = pres['nodes']
            shell = [n for n in nodes.values() if n.get('instance', {}).get('component', {}).get('assetId') in ('cmp_alva_sidebar', 'cmp_alva_settingsnav')]
            if shell:
                padding(nodes[pres['rootId']], [8, 12, 12, 12])
                modified = True
            for node in list(nodes.values()):
                asset = node.get('instance', {}).get('component', {}).get('assetId')
                if asset == 'cmp_alva_pageheader':
                    node['instance']['variant'] = {'axis_navigation': 'Expanded'}
                    dimension(node, 'height', 'control.height', 36)
                    modified = True
                if asset != 'cmp_alva_settingsnav':
                    continue
                old_key = original_return['id'] + ':interactions'
                interactions = node['instance'].get('overrides', {}).pop(old_key, [])
                owner = next(n for n in nodes.values() if n['name'] == 'SettingsRegion')
                back = deepcopy(original_return)
                back['id'] = 'node_' + screen['id'][4:] + '_shell_back_to_work'
                back['name'], back['visible'] = 'HeaderReturn', True
                back['interactions'] = interactions
                back['layout-item-h-sizing'] = 'auto'
                dimension(back, 'height', 'control.height', 36)
                bar = frame('node_' + screen['id'][4:] + '_shell_return_row', 'ReturnRow', height=36)
                bar.update({'layout': 'flex', 'layout-flex-dir': 'row', 'layout-align-items': 'center'})
                dimension(bar, 'height', 'control.height', 36)
                padding(bar, [0, 24, 0, 24])
                bar['children'] = [back['id']]
                nodes.update({n['id']: n for n in (back, bar)})
                owner['children'].append(bar['id'])
                moved[(screen['id'], pres['id'], node['id'])] = back['id']
                modified = True
            wizard = next((n for n in nodes.values() if n['name'] == 'WizardBack'), None) if any(
                x in screen['name'] for x in (' / 24 ', ' / 25 ', ' / 26 ')) else None
            if wizard is not None:
                dialog = next(n for n in nodes.values() if n['name'] == 'Dialog')
                dialog['children'].remove(wizard['id'])
                reserve = frame('node_' + screen['id'][4:] + '_sheet_native_safe_area', 'NativeSafeArea', 50)
                bar = frame('node_' + screen['id'][4:] + '_sheet_return_row', 'SheetReturnRow', height=36)
                bar.update({'layout': 'flex', 'layout-flex-dir': 'row', 'layout-align-items': 'center'})
                dimension(bar, 'height', 'control.height', 36)
                padding(bar, [0, 24, 0, 24])
                gap(bar)
                bar['children'] = [wizard['id'], reserve['id']]
                body = frame('node_' + screen['id'][4:] + '_sheet_body', 'SheetBody', height=1)
                body.update({'layout': 'flex', 'layout-flex-dir': 'column', 'layout-align-items': 'center',
                             'layout-justify-content': 'center', 'layout-item-v-sizing': 'fill'})
                body['children'] = [dialog['id']]
                padding(body, [12, 12, 12, 12])
                nodes.update({n['id']: n for n in (reserve, bar, body)})
                root = nodes[pres['rootId']]
                padding(root, [8, 12, 12, 12])
                root.update({'layout-align-items': 'stretch', 'layout-justify-content': 'start',
                             'layout-gap': {'row-gap': 0, 'column-gap': 0}})
                root.get('tokenBindings', {}).pop('itemSpacing', None)
                root['children'] = [body['id'], bar['id']]
                modified = True
        if modified:
            p_ops.append({'type': 'put-screen', 'screen': screen})
            affected.append(screen['name'])

    navigation = read(product / 'requirements/navigation.json')
    requirement = next(r for r in navigation['requirements'] if r['id'] == 'req_alva_navigation_settings')
    for link in requirement['links']:
        key = (link['screen']['assetId'], link['presentationId'], link['nodeId'])
        if key in moved:
            link['nodeId'] = moved[key]
    requirement['markdown'] = requirement['markdown'].replace('SettingsNav.BackToWork', '主设置页头 HeaderReturn（复用 Ghost/Leading Button）').replace('触发：左上箭头+返回工作台，位于设置标题之前；', '触发：主设置页头左上箭头+返回工作台，高36，与原生标题栏按钮中心同线；')
    wizard_requirement = next(r for r in navigation['requirements'] if r['id'] == 'req_alva_navigation_wizard')
    wizard_requirement['markdown'] += '\n窗口壳：24/25/26复用req_alva_navigation_native_shell的native-safe顶部36返回行，保留原WizardBack节点ID和既有返回目标，移出居中卡片而非复制按钮；卡片在返回行下的剩余区域居中。运行端pending问答也在该行之后，不能覆盖系统按钮区。保存中沿原owner禁用返回/关闭。\n'
    navigation['requirements'].append({'id': 'req_alva_navigation_native_shell', 'title': '原生标题栏与可隐藏导航壳',
        'markdown': '桌面窗口使用 MyGo 原生 TitleBarHiddenInset，关闭/最小化/缩放属于 OS/window owner，不自绘系统按钮。读取 c.TitleBar.Left/Right/Height 的实时事实；没有原生标题栏事实时沿用顶部12外距。原生高度有效时，36工具行顶部为 max(0,(max(Height,36)-36)/2)，其余左右/底部及区域间隔12。52高原生栏样例得到顶部8；样例Left86、rootX12、页头基础左右24时，collapsed有效左内距max(24,Left+8-rootX)=82，32侧栏开关实际x94。样例数值不成为新Token，OS变化重新计算。\n展开：Sidebar宽220，顶部36与主PageHeader36同Y；侧栏顶部先避让系统左区、再Grow、最后PanelLeft32开关，不保留独立品牌行或自绘空标题条。收起：整棵Sidebar及其分隔器退出布局、命中和焦点序列，主区左移；开关在同一个主PageHeader最左，仍避让原生Left/Right。不是72图标rail。切换仅改变视图，不Stop、不换会话、不丢输入草稿。\n主PageHeader.Navigation=Expanded/Collapsed只表达开关所在位置的局部结构；标题16、来源12横排，文字行框与图标框双轴居中，控件内距对称。窄宽时标题/来源受可用宽度约束、优先保留开关和动作，运行端使用单行截断而非抬高工具行；真实完整值由已有内容/提示读取。资源PanelHeader保持52，不跟着主工具行改高。\n设置侧栏208保持，顶部36避让原生区，标题16；返回工作台作为主设置页头HeaderReturn的唯一有效入口，原SettingsNav内部旧按钮保留ID但隐藏、移除旧触发，原requirement链接迁到新视觉节点。返回行为与草稿政策仍由req_alva_navigation_settings维护。\n验收：明暗、展开/收起、代表窄宽；按钮位置不随侧栏切换改变，两个工具行中心一致，系统窗口按钮可用。静态设计不证明OS拖动、焦点或实际运行通过。',
        'links': []})
    for ann in navigation.get('annotations', []):
        target = ann.get('target', {})
        if target.get('kind') == 'node':
            key = (target.get('screen', {}).get('assetId'), target.get('presentationId'), target.get('nodeId'))
            if key in moved:
                target['nodeId'] = moved[key]
                ann['description'] = ann['description'].replace('SettingsNav.BackToWork', 'HeaderReturn')
    product_pkg = read(product / 'manifest.json')['packageId']
    native_rule = navigation['requirements'][-1]
    for operation in p_ops:
        screen = operation['screen']
        for pres in screen['presentations']:
            for node in pres['nodes'].values():
                if node['name'] in ('HeaderReturn', 'WizardBack') or (node['name'] == 'Header' and any(
                        name in screen['name'] for name in (' / 21 ', ' / 22 '))):
                    link = {'kind': 'node', 'screen': {'packageId': product_pkg, 'assetId': screen['id']},
                            'presentationId': pres['id'], 'nodeId': node['id']}
                    native_rule['links'].append(link)
                    if node['name'] == 'Header':
                        navigation['annotations'].append({'id': 'ann_native_shell_' + screen['id'][4:],
                            'role': 'behavior-contract', 'description': 'req_alva_navigation_native_shell — 36工具行、动态native-safe避让、导航真隐藏与PanelLeft开关位置。',
                            'tags': ['navigation', 'native-shell'], 'target': link})
    p_ops.append({'type': 'put-requirement-file', 'entry': 'requirements/navigation.json', 'requirementFile': navigation})
    write(OUT / 'foundation.batch.json', {'baseRevision': f_revision, 'batchId': 'alva-native-shell-foundation-20261105-v3', 'operations': f_ops})
    write(OUT / 'product.batch.json', {'baseRevision': p_revision, 'batchId': 'alva-native-shell-product-20261105-v3', 'operations': p_ops})
    write(OUT / 'plan.json', {'sourceRevisions': {'foundation': f_revision, 'product': p_revision}, 'affectedPages': affected,
                            'movedReturnVisualNodes': [{'screenId': k[0], 'presentationId': k[1], 'oldSettingsNav': k[2], 'newHeaderReturn': v} for k, v in moved.items()],
                            'nativeSample': {'Height': 52, 'Left': 86, 'Right': 0}, 'toolbarHeight': 36, 'actionSize': 32,
                            'representativeExpandedSidebarWidth': 220, 'settingsNavWidth': 208})
    print('prepared scoped public batches', len(f_ops), len(p_ops), 'screens', len(affected), 'returns', len(moved))


if __name__ == '__main__':
    prepare()
