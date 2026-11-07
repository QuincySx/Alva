# INPUT: Current SmallPen revisions and existing workbench/component identities
# OUTPUT: Guarded compact-density deltas, preserving pages, components and behavior bindings
# POS: Design-only public typed operations; never rewrites canonical package files directly.
import argparse
from copy import deepcopy
import hashlib
import json

import author
from composer_states import align_geometry


DENSITY_NOTE = (
    '\n紧凑布局（2026-10-23）：主导航220、页头52、主区外边距24；'
    '首页限宽760，标题32，标题到输入区16、输入区到建议12。'
    '导航行36、条目间隔4、分组间隔12；输入区对称内边距8、间隔8、工具栏控件32，Send/Stop为32圆形。'
    '输入正文与连续阅读保持16/1.75，两行起始编辑空间56；'
    '会话组间隔16。空间靠布局收敛，不缩小阅读字号，不增加Token或变体。\n'
)


def load(path):
    return json.loads(path.read_text())


def plan():
    foundation = author.FOUNDATION
    product = author.PRODUCT
    bases = {p.name: author.cli('project', 'show', p, label='density-base')['revision']
             for p in (foundation, product)}
    package_id = load(foundation / 'manifest.json')['packageId']
    token_sets = load(foundation / 'tokens/tokens.json')['sets']
    tokens = {t['name']: t for s in token_sets for t in s['tokens']}

    def binding(role):
        return {'packageId': package_id, 'assetId': tokens['ui.' + role]['id']}

    def spacing(node, gap=None, padding=None, margin_top=None):
        changes = {'tokenBindings': deepcopy(node.get('tokenBindings', {}))}
        bindings = changes['tokenBindings']
        if gap is not None:
            changes['layout-gap'] = {'row-gap': gap, 'column-gap': gap}
            bindings['itemSpacing'] = binding({4: 'space.1', 8: 'space.2', 12: 'space.3', 16: 'space.4'}[gap])
        if padding is not None:
            changes['layout-padding'] = {p: padding for p in ('p1', 'p2', 'p3', 'p4')}
            for side in ('Top', 'Right', 'Bottom', 'Left'):
                bindings['padding' + side] = binding({8: 'space.2', 12: 'space.3', 24: 'space.6'}[padding])
        if margin_top is not None:
            changes['layout-item-margin'] = {'m1': margin_top, 'm2': 0, 'm3': 0, 'm4': 0}
            changes['layout-item-margin-type'] = 'multiple'
            bindings.pop('marginTop', None)
            if margin_top:
                bindings['marginTop'] = binding('space.2')
        return changes

    # The same name has separate canonical IDs in viewport/density theme sets.
    # Update each real owner; a name-only dict would select the last theme.
    foundation_ops = []
    for token_set in token_sets:
        for token in token_set['tokens']:
            value = {'ui.size.sidebar': 72 if token_set['name'] == 'Viewport/Narrow' else 220,
                     'ui.size.header': 52, 'ui.control.row': 36}.get(token['name'])
            if value is not None:
                foundation_ops.append({'type': 'set-token-value', 'tokenId': token['id'], 'value': value})
    components = load(foundation / 'components/components.json')['componentSets']
    component_sets = {c['id']: c for c in components}
    for component in components:
        if component['id'] not in ('cmp_alva_sidebar', 'cmp_alva_composer'):
            continue
        for variant in component['variants']:
            if component['id'] == 'cmp_alva_composer':
                changed = deepcopy(variant)
                align_geometry(changed, component_sets, tokens, package_id)
                if changed != variant:
                    foundation_ops.append({'type': 'put-variant', 'componentId': component['id'], 'variant': changed})
                continue
            for node in variant['nodes'].values():
                changes = None
                if component['id'] == 'cmp_alva_sidebar':
                    if node['id'] == variant['rootId']:
                        changes = spacing(node, gap=4)
                    elif node['name'] in ('ProjectTitle', 'RecentTitle'):
                        changes = spacing(node, margin_top=8)
                if changes:
                    foundation_ops.append({'type': 'update-component-node', 'componentId': component['id'],
                                           'variantId': variant['id'], 'nodeId': node['id'], 'changes': changes})
    product_ops = []
    for path in (product / 'screens').glob('*.json'):
        screen = load(path)
        if screen['name'].split(' / ', 1)[-1] not in ('21 新任务', '22 对话与面板'):
            continue
        for presentation in screen['presentations']:
            for node in presentation['nodes'].values():
                changes = None
                if node['name'] == 'Welcome':
                    changes = spacing(node, padding=24)
                elif node['name'] == 'WelcomeContent':
                    changes = spacing(node, gap=12) | {'layout-item-max-w': 760}
                elif node['name'] == 'WelcomeTitle':
                    changes = {'layout-item-margin': {'m1': 0, 'm2': 0, 'm3': 4, 'm4': 0},
                               'layout-item-margin-type': 'multiple',
                               'tokenBindings': deepcopy(node.get('tokenBindings', {})) | {'marginBottom': binding('space.1')}}
                elif node['name'] == 'Suggestions':
                    changes = spacing(node, margin_top=0)
                elif node['name'] == 'Composer':
                    for key in node.get('instance', {}).get('overrides', {}):
                        if key == 'node_alva_composer_composer_space:height':
                            product_ops.append({'type': 'set-instance-override', 'screenId': screen['id'],
                                                'presentationId': presentation['id'], 'nodeId': node['id'],
                                                'overridePath': key, 'value': 20})
                if changes:
                    product_ops.append({'type': 'update-presentation-node', 'screenId': screen['id'],
                                        'presentationId': presentation['id'], 'nodeId': node['id'], 'changes': changes})
    requirements = load(product / 'requirements/home-composer.json')
    requirement = next(r for r in requirements['requirements'] if r['id'] == 'req_alva_composer_state')
    if DENSITY_NOTE.strip() not in requirement['markdown']:
        requirement['markdown'] += DENSITY_NOTE
    product_ops.append({'type': 'put-requirement-file', 'entry': 'requirements/home-composer.json',
                        'requirementFile': requirements})
    return bases, [(foundation, foundation_ops), (product, product_ops)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--apply', action='store_true', help='Apply after the public CLI dry run succeeds')
    args = parser.parse_args()
    bases, plans = plan()
    for package, operations in plans:
        digest = hashlib.sha256(json.dumps(operations, sort_keys=True).encode()).hexdigest()[:16]
        path = author.INTENTS / ('density-' + package.stem + '-' + bases[package.name][:12] + '-' + digest + '.json')
        payload = {'baseRevision': bases[package.name],
                   'batchId': 'density-' + package.stem + '-' + bases[package.name][:12] + '-' + digest,
                   'operations': operations}
        path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + '\n')
        author.cli('advanced', 'apply', package, '--batch', path, '--dry-run', label='density-dry-run')
    if args.apply:
        for package, _ in plans:
            current = author.cli('project', 'show', package, label='density-final-guard')['revision']
            if current != bases[package.name]:
                raise ValueError('Concurrent design edit; inspect and re-plan before applying')
        for package, operations in plans:
            digest = hashlib.sha256(json.dumps(operations, sort_keys=True).encode()).hexdigest()[:16]
            path = author.INTENTS / ('density-' + package.stem + '-' + bases[package.name][:12] + '-' + digest + '.json')
            author.cli('advanced', 'apply', package, '--batch', path, label='density-apply')


if __name__ == '__main__':
    main()
