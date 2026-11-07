# INPUT: Saved formal pages and the user-approved boundary candidate at its recorded revisions.
# OUTPUT: Isolated native candidate and guarded public CLI batches preserving formal node identities.
# POS: Design-only propagation of shared visual rules; no application code or business SPEC changes.
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import re
import shutil

import author
from formal_refresh import apply_batch, read, run


VISUAL_FIELDS = (
    'fills', 'strokes', 'shadow', 'cornerRadius', 'layout', 'layout-flex-dir',
    'layout-gap', 'layout-padding', 'layout-padding-type', 'layout-align-items',
    'layout-justify-content', 'textStyle', 'tokenBindings',
)
PAGES = set(range(21, 44))


def plan(foundation, product, approved):
    source = approved / foundation.name
    old_components = {c['id']: c for c in read(foundation / 'components/components.json')['componentSets']}
    new_components = {c['id']: c for c in read(source / 'components/components.json')['componentSets']}
    old_sets = {s['id']: s for s in read(foundation / 'tokens/tokens.json')['sets']}
    new_sets = {s['id']: s for s in read(source / 'tokens/tokens.json')['sets']}
    if old_components.keys() != new_components.keys() or old_sets.keys() != new_sets.keys():
        raise ValueError('Approved baseline must reuse the existing component and Token inventory')
    f_ops = []
    for ident, value in new_sets.items():
        if {t['id'] for t in value['tokens']} != {t['id'] for t in old_sets[ident]['tokens']}:
            raise ValueError('Token identities changed')
        if value != old_sets[ident]:
            f_ops.append({'type': 'put-token-set', 'set': value})
    for ident, value in new_components.items():
        before = old_components[ident]
        if value['axes'] != before['axes'] or {v['id'] for v in value['variants']} != {v['id'] for v in before['variants']}:
            raise ValueError('Component property or variant identities changed: ' + value['name'])
        for variant in value['variants']:
            original = next(v for v in before['variants'] if v['id'] == variant['id'])
            if variant['nodes'].keys() != original['nodes'].keys():
                raise ValueError('Component node identities changed: ' + value['name'])
        if value != before:
            f_ops.append({'type': 'put-component-set', 'componentSet': value})
    pkg = read(foundation / 'manifest.json')['packageId']
    tokens = {t['name']: t for s in new_sets.values()
              if s['name'] in ('Theme/Default', 'Appearance/Light') for t in s['tokens']}

    def binding(role):
        return {'packageId': pkg, 'assetId': tokens['ui.' + role]['id']}

    def paint(node, role):
        node['fills'] = [{'type': 'solid', 'color': tokens['ui.' + role]['value']}]
        node.setdefault('tokenBindings', {})['fill'] = binding(role)

    def no_outline(node):
        node['strokes'] = []
        node.pop('shadow', None)
        for key in list(node.get('tokenBindings', {})):
            if key in ('stroke', 'strokeWidth', 'shadow') or key.startswith('strokes.'):
                del node['tokenBindings'][key]

    def radius(node, role='radius.card'):
        node['cornerRadius'] = tokens['ui.' + role]['value']
        node.setdefault('tokenBindings', {})['cornerRadius'] = binding(role)

    def spacing(node, values, padding=False):
        names = {4: '1', 8: '2', 12: '3', 16: '4', 24: '6', 32: '8'}
        if padding:
            values = [values] * 4 if isinstance(values, int) else values
            node['layout-padding'] = dict(zip(('p1', 'p2', 'p3', 'p4'), values))
            node['layout-padding-type'] = 'multiple'
            for side, value in zip(('Top', 'Right', 'Bottom', 'Left'), values):
                key = 'padding' + side
                node.setdefault('tokenBindings', {}).pop(key, None)
                if value:
                    node['tokenBindings'][key] = binding('space.' + names[value])
        else:
            node['layout-gap'] = {'row-gap': values, 'column-gap': values}
            node.setdefault('tokenBindings', {}).pop('itemSpacing', None)
            if values:
                node['tokenBindings']['itemSpacing'] = binding('space.' + names[values])

    # Settings navigation has the same window-background role as the accepted Sidebar.
    settings_nav = deepcopy(new_components['cmp_alva_settingsnav'])
    for variant in settings_nav['variants']:
        root = variant['nodes'][variant['rootId']]
        paint(root, 'color.canvas')
        no_outline(root)
    if settings_nav != old_components[settings_nav['id']]:
        f_ops = [op for op in f_ops if op.get('componentSet', {}).get('id') != settings_nav['id']]
        f_ops.append({'type': 'put-component-set', 'componentSet': settings_nav})

    template_screen = next(read(p) for p in (approved / product.name / 'screens').glob('*.json')
                           if read(p)['name'].endswith(' / 工作台候选'))
    template = template_screen['presentations'][0]['nodes']
    templates = {n['name']: n for n in template.values()}
    p_ops, changed_pages = [], []
    for file in sorted((product / 'screens').glob('*.json')):
        original = read(file)
        match = re.search(r' / (\d+) ', original['name'])
        if not match or int(match[1]) not in PAGES:
            continue
        number = int(match[1])
        screen = deepcopy(original)
        for pres in screen['presentations']:
            nodes = pres['nodes']
            by = {n['name']: n for n in nodes.values()}

            def children(node):
                return [nodes[i] for i in reversed(node.get('children', []))]

            def arrange(node, items):
                node['children'] = [n['id'] for n in reversed(items)]

            def visual(target, source_name):
                source_node = templates[source_name]
                for key in VISUAL_FIELDS:
                    if key in source_node:
                        target[key] = deepcopy(source_node[key])
                    elif key in ('strokes', 'shadow'):
                        target.pop(key, None)

            def frame(name, items=(), direction='column'):
                ident = f'node_{number}_boundary_{name.lower()}'
                if ident in nodes:
                    raise ValueError('Candidate already has this grouping; read its latest revision')
                n = {'id': ident, 'name': name, 'type': 'FRAME', 'x': 0, 'y': 0,
                     'width': 1, 'height': 1, 'children': [], 'fills': [], 'layout': 'flex',
                     'layout-flex-dir': direction, 'layout-item-h-sizing': 'fill',
                     'layout-item-v-sizing': 'auto'}
                arrange(n, items)
                nodes[ident] = n
                by[name] = n
                return n

            def clone(source_name, name=None, existing=None):
                source_node = templates[source_name]
                n = existing if existing is not None else deepcopy(source_node)
                if existing is not None:
                    visual(n, source_name)
                    if n['type'] == 'INSTANCE':
                        n['instance']['overrides'].update(deepcopy(source_node['instance'].get('overrides', {})))
                else:
                    n['id'] = f'node_{number}_boundary_{(name or source_name).lower()}'
                    n['name'] = name or source_name
                    if n['id'] in nodes:
                        raise ValueError('Duplicate candidate element: ' + n['id'])
                    nodes[n['id']] = n
                    by[n['name']] = n
                if source_node.get('children'):
                    arrange(n, [clone(template[i]['name'], existing=by['Artifact'] if template[i]['name'] == 'Artifact' else None)
                                for i in reversed(source_node['children'])])
                return n

            def user_turn():
                bubble = by['UserMessage']
                visual(bubble, 'UserMessage')
                bubble['layout-item-max-w'] = 560
                turn = frame('UserTurn', [bubble], 'row')
                turn['layout-item-max-w'] = 760
                turn['layout-justify-content'] = 'end'
                owner = next(n for n in nodes.values() if n['id'] != turn['id'] and bubble['id'] in n.get('children', []))
                owner['children'] = [turn['id'] if i == bubble['id'] else i for i in owner['children']]

            def reply(name, header_name, items, initial):
                owner = by[name]
                header = by[header_name]
                header['instance'].setdefault('overrides', {})['node_alva_messageheader_messageheader_avatar:visible'] = False
                avatar = clone('AssistantReplyAvatar', name + 'Avatar')
                avatar['instance']['overrides']['node_alva_avatar_avatar_label:text'] = initial
                content = frame(name + 'Content', [header, *items])
                spacing(content, 12)
                owner['layout-flex-dir'] = 'row'
                owner['layout-align-items'] = 'start'
                spacing(owner, 12)
                arrange(owner, [avatar, content])

            root = nodes[pres['rootId']]
            paint(root, 'color.canvas')
            for item in nodes.values():
                if item.get('instance', {}).get('component', {}).get('assetId') == 'cmp_alva_sidebar':
                    overrides = item['instance'].get('overrides', {})
                    key = 'node_alva_sidebar_sidebar_currentthread__node_alva_navitem_navitem_label:tokenBindings'
                    if key in overrides:
                        overrides[key]['fill'] = binding('color.text')
            if 'Main' in by or 'SettingsRegion' in by:
                spacing(root, 12, padding=True)
                spacing(root, 12)
                for name in ('Main', 'SettingsRegion'):
                    if name in by:
                        paint(by[name], 'color.surface')
                        no_outline(by[name])
                        radius(by[name])
            if 'Dialog' in by:
                no_outline(by['Dialog'])
                radius(by['Dialog'])
                by['Dialog']['shadow'] = deepcopy(tokens['ui.shadow.popup']['value'])
                by['Dialog'].setdefault('tokenBindings', {})['shadow'] = binding('shadow.popup')
            if number in (22, 27, 29, 40):
                user_turn()
            if number == 22:
                visual(by['Body'], 'WorkspaceBody')
                visual(by['Conversation'], 'Conversation')
                visual(by['RightWorkspace'], 'ResourcePanel')
                panel = by['PanelHeader']['instance'].setdefault('overrides', {})
                tile = 'node_alva_panelheader_panelheader_filetab__node_alva_tab_tab_icon'
                glyph = tile + '__node_alva_icon_icon_glyph'
                panel.pop(tile + ':fills', None)
                panel.pop(tile + ':cornerRadius', None)
                for field in ('fill', 'cornerRadius'):
                    panel.get(tile + ':tokenBindings', {}).pop(field, None)
                panel.pop(glyph + ':fills', None)
                panel.setdefault(glyph + ':tokenBindings', {})['fill'] = binding('color.secondary')
                panel['node_alva_panelheader_panelheader_filetab__node_alva_tab_tab_label:text'] = '工作台方案.md'
                # This sample has one actual resource; the shared Tab capability remains intact.
                panel['node_alva_panelheader_panelheader_tasktab:visible'] = False
                visual(by['Document'], 'Document')
                visual(by['DocSection3'], 'DocSection3')
                for name in ('DocumentTitle', 'DocumentLead', 'DocH1', 'DocP1', 'DocH2', 'DocP2', 'DocH3', 'DocP3'):
                    visual(by[name], name)
                # Preserve the original path and Find action, which the sample omitted.
                spacing(by['FilePath'], [0, 24, 0, 24], padding=True)
                by['DocumentType']['visible'] = False
                by['AssistantBody']['text'] = templates['AssistantReplyBody']['text']
                by['AssistantSummary']['visible'] = False
                by['Completion']['visible'] = False
                by['Artifact']['instance']['overrides'] = deepcopy(templates['Artifact']['instance']['overrides'])
                artifact = clone('ArtifactPreview')
                working = by['ToolActivity']
                visual(working, 'Working')
                overrides = deepcopy(templates['Working']['instance'].get('overrides', {}))
                overrides.pop('node_alva_toolactivity_toolactivity_chevron:visible', None)
                working['instance'].setdefault('overrides', {}).update(overrides)
                process = frame('ActivitySummary', [clone('ReadDone'), clone('ConnectionDone'), working])
                spacing(process, 0)
                result = frame('ExecutionToArtifact', [process, artifact])
                spacing(result, 12)
                reply('Assistant', 'AssistantHeader', [by['AssistantBody'], result, by['MessageActions']], 'a')
                # Retain all three original message actions and their identities.
                arrange(by['Transcript'], [by['UserTurn'], by['Assistant'], by['AssistantSummary'], by['MessageSpace']])
                spacing(by['Transcript'], 24)
                by['Composer']['instance'].setdefault('overrides', {}).update(
                    deepcopy(templates['Composer']['instance'].get('overrides', {})))
            if number in (27, 29):
                no_outline(by['Roster'])
                paint(by['Roster'], 'color.canvas')
                no_outline(by['PartnerHeader'])
                for name, header, body, initial in [('ResearchMessage', 'ResearchMessageHeader', 'ResearchMessageBody', '研'),
                                                    ('ReviewMessage', 'ReviewMessageHeader', 'ReviewMessageBody', '审')]:
                    if name in by:
                        items = [by[body]]
                        if name == 'ResearchMessage':
                            items.extend(by[n] for n in ('UsedSkills', 'ResearchResult') if n in by)
                        elif 'RoundDone' in by:
                            items.append(by['RoundDone'])
                        reply(name, header, items, initial)
                turn_names = ['UserTurn', 'ResearchMessage', 'ReviewMessage', 'ChatSpace']
                arrange(by['Transcript'], [by[n] for n in turn_names if n in by])
                spacing(by['Transcript'], 24)
            if number == 23:
                no_outline(by['Manager'])
                spacing(by['Manager'], 24)
                paint(by['Connections'], 'color.subtle')
                radius(by['Connections'])
                paint(by['Models'], 'color.transparent')
            # Existing fields, selectable rows and modal/popover shadows retain their functional edges.
            if not set(original['presentations'][0]['nodes']).issubset(nodes):
                raise ValueError('An existing node was removed')
        if screen != original:
            p_ops.append({'type': 'put-screen', 'entry': 'screens/' + file.name, 'screen': screen})
            changed_pages.append(screen['name'])
    return f_ops, p_ops, changed_pages


def prepare(work, approved):
    work.mkdir(parents=True, exist_ok=True)
    candidate = work / 'candidate'
    candidate.mkdir()
    approval = read(author.ROOT / 'exports/archive/coherent-2026-11-04/review-evidence.json')
    bases = {}
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        if run('project', 'show', approved / package.name)['revision'] != approval['candidateRevisions'][kind]:
            raise ValueError('Approved candidate changed; inspect before preparing')
        bases[kind] = run('project', 'show', package)['revision']
        shutil.copytree(package, candidate / package.name)
    f_ops, p_ops, pages = plan(candidate / author.FOUNDATION.name, candidate / author.PRODUCT.name, approved)
    record = {'bases': bases, 'approvedRevisions': approval['candidateRevisions'],
              'userDirection': '舒服 / 那你开始啊', 'pages': pages,
              'foundationOperations': f_ops, 'productOperations': p_ops}
    (work / 'plan.json').write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n')
    for kind, package, ops in [('foundation', author.FOUNDATION, f_ops), ('product', author.PRODUCT, p_ops)]:
        apply_batch(candidate / package.name, ops, bases[kind], 'candidate-' + kind, work)
    print(json.dumps({'pages': pages, 'foundationOperations': len(f_ops)}, ensure_ascii=False), flush=True)


def apply(work):
    data = read(work / 'plan.json')
    review = read(work / 'native-review.json')
    if not review['accepted'] or review['planSha256'] != hashlib.sha256((work / 'plan.json').read_bytes()).hexdigest():
        raise ValueError('Native review must refer to this exact plan')
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        if run('project', 'show', package)['revision'] != data['bases'][kind]:
            raise ValueError('Formal source changed; inspect and prepare again')
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        apply_batch(package, data[kind + 'Operations'], data['bases'][kind], 'formal-' + kind, work)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--approved', type=Path)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if args.apply:
        apply(args.work)
    else:
        if args.approved is None:
            parser.error('--approved is required for preparation')
        prepare(args.work, args.approved)
