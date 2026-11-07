# INPUT: Current saved SmallPen pages, Foundation definitions and reviewed package revisions.
# OUTPUT: Isolated formal-page candidate and guarded public typed deltas preserving existing identities.
# POS: Design-only layout/reading refresh; never writes canonical package JSON or runs application code.
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

import author


PAGES = {21, 22, 23, 27, 29, *range(31, 39), 40}


def read(path):
    return json.loads(path.read_text())


def run(*args):
    p = subprocess.run(['smallpen', *map(str, args), '--json'], text=True, capture_output=True)
    result = json.loads(p.stdout)
    if p.returncode:
        raise RuntimeError(json.dumps(result, ensure_ascii=False))
    return result


def apply_batch(package, operations, revision, label, work):
    if not operations:
        return {'revision': revision, 'changed': False}
    digest = hashlib.sha256(json.dumps(operations, sort_keys=True).encode()).hexdigest()[:16]
    batch = {'baseRevision': revision, 'batchId': f'{label}-{revision[:12]}-{digest}', 'operations': operations}
    path = work / (label + '-batch.json')
    path.write_text(json.dumps(batch, ensure_ascii=False, indent=2) + '\n')
    dry = run('advanced', 'apply', package, '--batch', path, '--dry-run')
    (work / (label + '-dry-run.json')).write_text(json.dumps(dry, ensure_ascii=False, indent=2) + '\n')
    detail = read(Path(dry['resultFile']['path'])) if dry.get('resultFile') else dry
    (work / (label + '-dry-run-full.json')).write_text(json.dumps(detail, ensure_ascii=False, indent=2) + '\n')
    # Whole-object writes also advise on unchanged sample measurements. Exact
    # numeric matches do not make an Avatar a Button, or text width a size Token.
    # Retain this advice; scoped layout/contrast and visual review remain required.
    allowed_advice = {'design_token_not_used', 'design_token_value_unmatched'}
    if any(w['code'] not in allowed_advice for w in detail.get('warnings', [])):
        raise ValueError('Unreviewed design warning: ' + str(path))
    result = run('advanced', 'apply', package, '--batch', path, '--inverse-out', work / (label + '-inverse.json'))
    (work / (label + '-apply.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(label, result['revision'], flush=True)
    return result


def plan(foundation, product):
    manifest = read(foundation / 'manifest.json')
    package_id = manifest['packageId']
    tokens = {t['name']: t for s in read(foundation / 'tokens/tokens.json')['sets']
              if s['name'] in ('Theme/Default', 'Appearance/Light') for t in s['tokens']}

    def bind(role):
        return {'packageId': package_id, 'assetId': tokens['ui.' + role]['id']}

    def themed(node, field, role):
        node.setdefault('tokenBindings', {})[field] = bind(role)
        value = tokens['ui.' + role]['value']
        if field == 'fill':
            node['fills'] = [{'type': 'solid', 'color': value}]
        elif field in ('fontSize', 'fontWeight', 'lineHeight'):
            node.setdefault('textStyle', {})[field] = value

    def spacing(node, amount, field='layout-gap'):
        if field == 'layout-padding':
            node[field] = {f'p{i}': amount for i in range(1, 5)}
            for side in ('Top', 'Right', 'Bottom', 'Left'):
                node.setdefault('tokenBindings', {})['padding' + side] = bind('space.' + {0: '1', 8: '2', 12: '3', 16: '4', 24: '6'}[amount])
                if amount == 0:
                    node['tokenBindings'].pop('padding' + side)
        else:
            node[field] = {'row-gap': amount, 'column-gap': amount}
            node.setdefault('tokenBindings', {})['itemSpacing'] = bind('space.' + {8: '2', 12: '3', 16: '4', 24: '6'}[amount])

    components = read(foundation / 'components/components.json')['componentSets']
    f_ops = []
    for component in components:
        for original in component['variants']:
            variant = deepcopy(original)
            nodes = {n['name']: n for n in variant['nodes'].values()}
            if component['id'] == 'cmp_alva_runstatus':
                nodes['Metrics']['visible'] = False
                nodes['Metrics']['text'] = ''
            elif component['id'] == 'cmp_alva_connectionrow':
                state = variant['selection']['axis_state']
                if state != 'Error':
                    status = nodes['Status']
                    status['instance']['variant'] = {'axis_name': 'Check' if state == 'Selected' else 'ChevronRight'}
                    overrides = status['instance'].setdefault('overrides', {})
                    glyph = 'node_alva_icon_icon_glyph'
                    role = 'color.accent' if state == 'Selected' else 'color.secondary'
                    overrides[glyph + ':fills'] = [{'type': 'solid', 'color': tokens['ui.' + role]['value']}]
                    overrides.setdefault(glyph + ':tokenBindings', {})['fill'] = bind(role)
                if state == 'Disconnected':
                    nodes['Description']['text'] = '未配置'
            elif component['id'] == 'cmp_alva_composer' and variant['selection']['axis_state'] == 'Running':
                nodes['Prompt']['text'] = '写下下一步…'
            if variant != original:
                f_ops.append({'type': 'put-variant', 'componentId': component['id'], 'variant': variant})

    p_ops, changed_pages = [], []
    for file in sorted((product / 'screens').glob('*.json')):
        original = read(file)
        short = original['name'].split(' / ', 1)[-1]
        if not short[:2].isdigit() or int(short[:2]) not in PAGES:
            continue
        screen = deepcopy(original)
        number = int(short[:2])
        for pres in screen['presentations']:
            by_name = {n['name']: n for n in pres['nodes'].values()}

            def node(name):
                return by_name[name]

            def new_frame(name, children, direction='column', **extra):
                ident = f'node_{number}_formal_{name.lower()}'
                value = dict(id=ident, name=name, type='FRAME', x=0, y=0, width=1, height=1,
                             children=[n['id'] for n in reversed(children)], fills=[], layout='flex',
                             **{'layout-flex-dir': direction, 'layout-item-h-sizing': 'fill',
                                'layout-item-v-sizing': 'auto'}) | extra
                pres['nodes'][ident] = value
                by_name[name] = value
                return value

            def display_children(owner, children):
                owner['children'] = [n['id'] for n in reversed(children)]

            def children(owner):
                return [pres['nodes'][ident] for ident in reversed(owner['children'])]

            def button(name, label, kind, state='Default'):
                ident = f'node_{number}_formal_{name.lower()}'
                value = dict(id=ident, name=name, type='INSTANCE', x=0, y=0, width=1, height=36,
                             children=[], **{'layout-item-h-sizing': 'auto', 'layout-item-v-sizing': 'fix'},
                             tokenBindings={'height': bind('control.height')},
                             instance={'component': {'packageId': package_id, 'assetId': 'cmp_alva_button'},
                                       'variant': {'axis_content': 'Text', 'axis_kind': kind, 'axis_state': state},
                                       'overrides': {'node_alva_button_button_label:text': label}})
                pres['nodes'][ident] = value
                by_name[name] = value
                return value

            # Business prose follows text measurement; fixed label widths do not belong to pages.
            for item in list(pres['nodes'].values()):
                if item['type'] == 'TEXT' and item.get('visible', True):
                    item['layout-item-v-sizing'] = 'fix'
                    item['growType'] = 'auto-height'
                if item['type'] == 'INSTANCE':
                    item['instance'].get('overrides', {}).pop('node_alva_button_button_label:width', None)
            if number == 21:
                themed(node('WelcomeTitle'), 'fontSize', 'font.size.heading')
                node('Composer')['instance']['overrides']['node_alva_composer_composer_prompt:text'] = '描述你要完成的任务…'
                for name in ('ReadProject', 'Research', 'Review'):
                    node(name)['instance']['variant']['axis_kind'] = 'Ghost'
                spacing(node('Suggestions'), 8)
            if number in (22, 27, 29, 40):
                region = node('Conversation' if number in (22, 40) else 'Chat')
                region['layout-align-items'] = 'center'
                for child in children(region):
                    if child['name'] not in ('Transcript', 'ChatSpace', 'MessageSpace') and child.get('layout-item-h-sizing') == 'fill':
                        child['layout-item-max-w'] = 760
                if 'Context' in by_name:
                    node('Context')['visible'] = False
                    node('ThreadFooter')['visible'] = False
                    node('ThreadFooter')['layout-item-v-sizing'] = 'fix'
                    node('ThreadFooter')['height'] = 0
                user = node('UserMessage')
                spacing(user, 12, 'layout-padding')
                themed(node('UserText'), 'fontSize', 'font.size.subtitle')
                themed(node('UserText'), 'lineHeight', 'line.reading')
                if number in (27, 29) and 'Transcript' not in by_name:
                    part = children(region)
                    transcript_items = [n for n in part if n['name'] not in ('Composer', 'MemoryScope')]
                    transcript = new_frame('Transcript', transcript_items, **{'layout-item-v-sizing': 'fill', 'show-content': False, 'layout-item-min-h': 0})
                    spacing(transcript, 16)
                    display_children(region, [transcript, node('Composer'), node('MemoryScope')])
                transcript = by_name.get('Transcript')
                if transcript:
                    transcript['layout-align-items'] = 'center'
                    for child in children(transcript):
                        if child.get('layout-item-h-sizing') == 'fill':
                            child['layout-item-max-w'] = 760
            if number == 22:
                node('Composer')['instance']['overrides']['node_alva_composer_composer_prompt:text'] = '写下下一步…'
                spacing(node('Document'), 16)
                node('DocP2')['text'] = '伙伴保留职责、记忆与 Skills，换模型不改变身份；群聊单独记录。'
                node('DocP3')['text'] = '可展开工具、审批与子任务。关闭面板不影响执行。'
            if number in (23, *range(31, 39)):
                main = node('SettingsMain')
                main['layout-item-max-w'] = 1120 if number == 23 else 960 if number in (32, 33, 37, 38) else 800
                spacing(main, 16)
                themed(node('SettingsRegion'), 'fill', 'color.surface')
                # Keep a shared left gutter instead of shifting each category to its own center.
                node('SettingsRegion')['layout-align-items'] = 'start'
                for child in children(main):
                    if child['type'] == 'FRAME' and child.get('fills') and child.get('cornerRadius'):
                        themed(child, 'fill', 'color.transparent')
                        child['cornerRadius'] = 0
                        child.get('tokenBindings', {}).pop('cornerRadius', None)
                        spacing(child, 0, 'layout-padding')
                        spacing(child, 12)
            if number == 23:
                node('Manager')['layout-item-v-sizing'] = 'auto'
                node('Connections')['layout-item-v-sizing'] = 'auto'
                node('Connections')['width'] = 240
                node('Connections').get('tokenBindings', {}).pop('width', None)
                node('Models')['layout-item-v-sizing'] = 'auto'
                node('ConnectionSpace')['visible'] = False
                node('ModelSpace')['visible'] = False
                for name in ('AddConnection', 'AddProvider'):
                    node(name)['instance']['overrides']['node_alva_button_button_label:text'] = '添加连接'
                node('Connected')['instance']['variant']['axis_tone'] = 'Neutral'
                node('Connected')['instance']['overrides']['node_alva_badge_badge_label:text'] = '已保存'
                for name in ('Gemini', 'Local'):
                    node(name)['instance']['overrides']['node_alva_connectionrow_connectionrow_copy_description:text'] = '未配置'
                node('EditConnection')['instance']['variant']['axis_kind'] = 'Ghost'
                toolbar = new_frame('CatalogToolbar', [node('SearchModels'), node('Refresh'), node('AddModel')], 'row', **{'layout-align-items': 'center'})
                spacing(toolbar, 8)
                display_children(node('CatalogHeading'), [node('Connected'), node('EnabledCount')])
                display_children(node('Models'), [node('ConnectionTitle'), node('CatalogHeading'), toolbar,
                                                  node('ModelKinds'), node('GPT'), node('GPTMini'), node('Custom'),
                                                  node('Availability'), node('ModelSpace'), node('CatalogHint')])
            if number == 31:
                notice = node('Notification')['instance']['overrides']
                notice['node_alva_settingrow_settingrow_copy_title:text'] = '桌面完成通知'
                notice['node_alva_settingrow_settingrow_copy_description:text'] = '任务完成时发送桌面通知。'
                actions = new_frame('PreferencesActions', [button('ReloadPreferences', '重新加载', 'Secondary'), button('SavePreferences', '保存偏好', 'Primary', 'Disabled')], 'row', **{'layout-justify-content': 'end', 'layout-align-items': 'center'})
                spacing(actions, 8)
                display_children(node('SettingsMain'), [*[c for c in children(node('SettingsMain')) if c['name'] != 'PreferencesActions'], actions])
        if screen != original:
            p_ops.append({'type': 'put-screen', 'entry': 'screens/' + file.name, 'screen': screen})
            changed_pages.append(screen['name'])
    preferences_before = read(product / 'requirements/preferences.json')
    preferences = deepcopy(preferences_before)
    prefs_screen = next((op['screen'] for op in p_ops if op['screen']['name'].endswith('31 通用')), None)
    if prefs_screen is None:
        prefs_screen = next(read(f) for f in (product / 'screens').glob('*.json') if read(f)['name'].endswith('31 通用'))
    pres = prefs_screen['presentations'][0]
    for name in ('SavePreferences', 'ReloadPreferences'):
        target = next(n for n in pres['nodes'].values() if n['name'] == name)
        annotation = {'id': 'ann_alva_preference_' + ('save' if name == 'SavePreferences' else 'reload'),
            'role': 'behavior-contract', 'tags': ['preferences', 'explicit-commit'],
            'description': 'req_alva_preference_choices — 本窗草稿由明确保存提交；重新加载读取已提交事实。共用Button的禁用/加载/焦点，实际CAS与回执由PreferencesService负责。',
            'target': {'kind': 'node', 'nodeId': target['id'], 'presentationId': pres['id'],
                       'screen': {'assetId': prefs_screen['id'], 'packageId': read(product / 'manifest.json')['packageId']}}}
        preferences['annotations'] = [a for a in preferences['annotations'] if a['id'] != annotation['id']]
        preferences['annotations'].append(annotation)
    choice = next(r for r in preferences['requirements'] if r['id'] == 'req_alva_preference_choices')
    if '正式页装配（2026-10-26）' not in choice['markdown']:
        choice['markdown'] += '\n正式页装配（2026-10-26）：31页尾使用共用Button提供重新加载/保存偏好。未编辑、读取失败或尚未读到真实revision时禁用保存；有可保存修改后启用。真实保存期间保持动作名/宽度/焦点并防重，错误保留草稿，不增加推挤操作行的重复进度文案。可选桌面完成通知不控制应用内错误、审批和待处理状态的可见性。\n'
    if preferences != preferences_before:
        p_ops.append({'type': 'put-requirement-file', 'entry': 'requirements/preferences.json', 'requirementFile': preferences})
    return f_ops, p_ops, changed_pages


def prepare(work):
    work.mkdir(parents=True, exist_ok=True)
    compiled = work / 'candidate'
    compiled.mkdir()
    bases = {}
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        bases[kind] = run('project', 'show', package)['revision']
        shutil.copytree(package, compiled / package.name)
    shutil.copytree(author.ROOT / 'fonts', compiled / 'fonts')
    f_ops, p_ops, pages = plan(compiled / author.FOUNDATION.name, compiled / author.PRODUCT.name)
    (work / 'plan.json').write_text(json.dumps({'bases': bases, 'pages': pages, 'foundationOperations': f_ops, 'productOperations': p_ops}, ensure_ascii=False, indent=2) + '\n')
    for kind, package, ops in [('foundation', author.FOUNDATION, f_ops), ('product', author.PRODUCT, p_ops)]:
        apply_batch(compiled / package.name, ops, bases[kind], 'candidate-' + kind, work)
    print(json.dumps({'pages': pages, 'foundationVariants': len(f_ops)}), flush=True)


def apply(work):
    plan_data = read(work / 'plan.json')
    review = read(work / 'review-accepted.json')
    if not review['accepted'] or review['planSha256'] != hashlib.sha256((work / 'plan.json').read_bytes()).hexdigest():
        raise ValueError('Candidate visual review is required')
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        if run('project', 'show', package)['revision'] != plan_data['bases'][kind]:
            raise ValueError('Saved package changed after preparation; inspect and prepare again')
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        apply_batch(package, plan_data[kind + 'Operations'], plan_data['bases'][kind], 'formal-' + kind, work)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    (apply if args.apply else prepare)(args.work)
