# INPUT: Current saved Foundation definitions and adjacent Product consumers.
# OUTPUT: Shared-input style candidate and exact guarded public CLI batch outside canonical packages.
# POS: Design-only input-family refinement; no new Tokens, component axes, page copies or application code.
import argparse
from copy import deepcopy
import json
from pathlib import Path
import shutil
import tempfile

import author
from formal_refresh import apply_batch, read, run


def plan(foundation):
    components = read(foundation / 'components/components.json')['componentSets']
    pkg = read(foundation / 'manifest.json')['packageId']
    tokens = {t['name']: t for s in read(foundation / 'tokens/tokens.json')['sets']
              if s['name'] in ('Theme/Default', 'Appearance/Light') for t in s['tokens']}

    def bind(role):
        return {'packageId': pkg, 'assetId': tokens['ui.' + role]['id']}

    def fill(node, role):
        node['fills'] = [{'type': 'solid', 'color': tokens['ui.color.' + role]['value']}]
        node.setdefault('tokenBindings', {})['fill'] = bind('color.' + role)

    def edge(node, role):
        node['strokes'] = [{'type': 'solid', 'color': tokens['ui.color.' + role]['value'],
                            'width': 1, 'alignment': 'inner'}]
        node.setdefault('tokenBindings', {})['stroke'] = bind('color.' + role)

    operations = []
    for original in components:
        if original['name'] not in ('Alva Input', 'Alva TextArea', 'Alva Field', 'Alva Composer'):
            continue
        component = deepcopy(original)
        for variant in component['variants']:
            by = {n['name']: n for n in variant['nodes'].values()}
            state = variant['selection']['axis_state']
            root = variant['nodes'][variant['rootId']]
            if component['name'] in ('Alva Input', 'Alva TextArea'):
                if state in ('Default', 'Loading', 'ReadOnly'):
                    fill(root, 'subtle')
                    edge(root, 'border')
                elif state == 'Disabled':
                    fill(root, 'canvas')
                    edge(root, 'border')
                    root['opacity'] = 0.6
                # Retain the existing clear Focus/Error/ErrorFocus paint and outline widths.
            elif component['name'] == 'Alva Field':
                title = by['Title']
                title.setdefault('textStyle', {})['fontWeight'] = 400
                title.setdefault('tokenBindings', {})['fontWeight'] = bind('font.weight.regular')
                if state == 'Disabled':
                    fill(title, 'secondary')
            else:
                editor = by['Editor']
                editor.pop('shadow', None)
                editor.get('tokenBindings', {}).pop('shadow', None)
                if state != 'Focus':
                    # A writing surface contains other controls: keep their subtle fills distinct.
                    fill(editor, 'canvas' if state == 'Disabled' else 'surface')
                    edge(editor, 'border')
                # Setup-disabled editors still expose active model/permission triggers.
                # Disabled child variants own their opacity; never dim the whole container.
        if component != original:
            operations.append({'type': 'put-component-set', 'componentSet': component})
    return operations


def prepare(work):
    work.mkdir(parents=True, exist_ok=True)
    candidate = Path(tempfile.mkdtemp(prefix='alva-input-review-'))
    bases = {}
    for kind, package in [('foundation', author.FOUNDATION), ('product', author.PRODUCT)]:
        bases[kind] = run('project', 'show', package)['revision']
        shutil.copytree(package, candidate / package.name)
    operations = plan(candidate / author.FOUNDATION.name)
    (work / 'plan.json').write_text(json.dumps({'bases': bases, 'candidateRoot': str(candidate),
        'components': [op['componentSet']['name'] for op in operations],
        'canonicalChanged': False}, ensure_ascii=False, indent=2) + '\n')
    apply_batch(candidate / author.FOUNDATION.name, operations, bases['foundation'], 'input-family', work)
    print(json.dumps({'candidateRoot': str(candidate), 'sharedComponents': len(operations),
                      'canonicalChanged': False}, ensure_ascii=False), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--work', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.work)
