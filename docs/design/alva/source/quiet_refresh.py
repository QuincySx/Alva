# INPUT: Reviewed Foundation/Product revisions and the restrained native design definitions
# OUTPUT: Isolated CLI compilation, guarded semantic-token pruning and component/page updates
# POS: Design-only refresh; preserves wireframes, package identities and prototype navigation.
import argparse
from copy import deepcopy
import json
from pathlib import Path
import shutil

import author
import controls_refresh as controls
import design_system as ds
import product_pages as pages


def guard(foundation_revision,product_revision):
    for package,revision in [(author.FOUNDATION,foundation_revision),(author.PRODUCT,product_revision)]:
        if author.cli('project','show',package,label='quiet-reviewed-base')['revision']!=revision:
            raise SystemExit('Package changed after review; inspect the App edits before refreshing.')


def prepare(work,foundation_revision,product_revision):
    guard(foundation_revision,product_revision)
    compiled=work/'compiled'
    compiled.mkdir(parents=True,exist_ok=False)
    controls.compile_controls(compiled,{s['name'] for s in ds.definitions()})
    shutil.copytree(author.PRODUCT,compiled/'alva.smallpen')
    baseline=json.loads((author.FOUNDATION/'components'/'components.json').read_text())['componentSets']
    screens=[json.loads(p.read_text()) for p in (author.PRODUCT/'screens').glob('*.json')]
    _,migration=controls.migrate_instances(baseline,screens)
    saved=(author.INTENTS,author.LOG,ds.FOUNDATION,ds.ROOT,pages.PRODUCT,pages.ROOT)
    author.INTENTS=compiled/'source'/'intents';author.LOG=compiled/'source'/'authoring-log.jsonl'
    ds.FOUNDATION=compiled/'alva-foundation.smallpen';ds.ROOT=compiled
    pages.PRODUCT=compiled/'alva.smallpen';pages.ROOT=compiled
    try:
        controls.batch(pages.PRODUCT,migration,'quiet-remove-button-size-axis')
        controls.batch(ds.FOUNDATION,token_operations(ds.FOUNDATION),'quiet-prune-unused-tokens')
        pages.write()
    finally:
        author.INTENTS,author.LOG,ds.FOUNDATION,ds.ROOT,pages.PRODUCT,pages.ROOT=saved
    (work/'reviewed-bases.json').write_text(json.dumps({'foundation':foundation_revision,'product':product_revision},indent=2)+'\n')
    print('Prepared native packages:',compiled,flush=True)


def token_operations(foundation):
    wanted={t['name'] for t in ds.token_definitions()}
    token_sets=json.loads((foundation/'tokens'/'tokens.json').read_text())['sets']
    operations=[]
    for token_set in token_sets:
        token_set['tokens']=[t for t in token_set['tokens'] if not t['name'].startswith('ui.') or t['name'] in wanted]
        operations.append({'type':'put-token-set','set':token_set})
    return operations


def preserve_navigation(before,after):
    if before['id']!=after['id']:raise ValueError('Screen identity changed')
    original={p['id']:p for p in before['presentations']}
    for presentation in after['presentations']:
        old=original[presentation['id']]
        if set(old['nodes'])!=set(presentation['nodes']):raise ValueError('Node identities changed: '+before['name'])
        for field in ('interactions','prototypeFlows'):
            if field in old:presentation[field]=deepcopy(old[field])
        for node_id,node in presentation['nodes'].items():
            previous=old['nodes'][node_id]
            if 'interactions' in previous:node['interactions']=deepcopy(previous['interactions'])
            if node.get('instance'):
                for path,value in previous.get('instance',{}).get('overrides',{}).items():
                    if path.endswith(':interactions'):
                        node['instance'].setdefault('overrides',{})[path]=deepcopy(value)
    return after


def apply(work):
    bases=json.loads((work/'reviewed-bases.json').read_text())
    guard(bases['foundation'],bases['product'])
    compiled=work/'compiled';foundation=compiled/'alva-foundation.smallpen'
    definitions=json.loads((foundation/'components'/'components.json').read_text())['componentSets']
    operations=[{'type':'put-component-set','componentSet':c} for c in definitions if c['name'].startswith('Alva ')]
    # All retained token identities were inherited by the compiler. Replace only
    # the current ui.* values; earlier draft definitions keep their stored IDs and values.
    operations.extend(token_operations(foundation))
    product_ops=[]
    formal={d['module']+' / '+d['page'] for d in pages.definitions()}
    for file in (compiled/'alva.smallpen'/'screens').glob('*.json'):
        after=json.loads(file.read_text())
        if after['name'] not in formal:continue
        before=json.loads((author.PRODUCT/'screens'/file.name).read_text())
        after=preserve_navigation(before,after)
        if before!=after:product_ops.append({'type':'put-screen','screen':after})
    # No shared package is changed until the complete page migration is ready.
    guard(bases['foundation'],bases['product'])
    controls.batch(author.FOUNDATION,operations,'quiet-foundation')
    controls.batch(author.PRODUCT,product_ops,'quiet-product-pages')
    print('Applied',sum(c['name'].startswith('Alva ') for c in definitions),'shared components and',len(product_ops),'formal pages.',flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--work',type=Path,required=True)
    parser.add_argument('--apply',action='store_true')
    parser.add_argument('--foundation-revision');parser.add_argument('--product-revision')
    args=parser.parse_args()
    if args.apply:apply(args.work)
    else:
        if not args.foundation_revision or not args.product_revision:parser.error('Preparation requires both reviewed revisions.')
        prepare(args.work,args.foundation_revision,args.product_revision)
