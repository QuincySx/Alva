# INPUT: Reviewed Foundation/Product revisions, shared block recipes and an immutable local baseline
# OUTPUT: Isolated CLI compilation and guarded Token/component/page consolidation with identity checks
# POS: Design maintenance only; preserves all historical pages, assets, variants and prototype navigation.
import argparse
from copy import deepcopy
import json
from pathlib import Path
import shutil

import author
import controls_refresh as controls
import design_system as ds
import product_pages as pages
import quiet_refresh as quiet
import reading_pilot as pilot


def load(path):return json.loads(path.read_text())


def prune_tokens(compiled):
    foundation=compiled/'alva-foundation.smallpen';product=compiled/'alva.smallpen'
    sets=load(foundation/'tokens'/'tokens.json')['sets']
    by_key={(s['name'],t['name']):t for s in sets for t in s['tokens']}
    scopes={};defaults={'Appearance':'Light','Density':'Comfortable','Viewport':'Desktop'}
    definitions={t['name']:t for t in ds.token_definitions()}
    for token in ds.token_definitions():
        values=token.get('values',{})
        scope=set(values)
        scope.update(group+'/'+defaults[group] for group in {key.split('/')[0] for key in values})
        scopes[token['name']]=scope or {'Theme/Default'}
    # token set updates existing locations; dropping an override in the intent
    # does not relocate an old themed Token. Move its base identity explicitly.
    for name,wanted in scopes.items():
        for group in wanted:
            if (group,name) in by_key:continue
            if len(wanted)!=1:raise ValueError('New theme location requires explicit identity: '+name)
            candidates=[t for (s,n),t in by_key.items() if n==name and s in {'Theme/Default','Appearance/Light','Density/Comfortable','Viewport/Desktop'}]
            if len(candidates)!=1:raise ValueError('Ambiguous Token base identity: '+name)
            by_key[(group,name)]=deepcopy(candidates[0])
    remap={}
    for group in sets:
        for token in group['tokens']:
            wanted=scopes.get(token['name'])
            if wanted and group['name'] not in wanted:
                if len(wanted)!=1:raise ValueError('Ambiguous Token scope migration')
                remap[token['id']]=by_key[(next(iter(wanted)),token['name'])]['id']
    def migrate(value):
        if isinstance(value,list):return [migrate(v) for v in value]
        if isinstance(value,dict):return {k:remap.get(v,v) if k=='assetId' else migrate(v) for k,v in value.items()}
        return value
    components=load(foundation/'components'/'components.json')['componentSets']
    fops=[{'type':'put-component-set','componentSet':migrate(c)} for c in components if migrate(c)!=c]
    for group in sets:
        group['tokens']=[t for t in group['tokens'] if not t['name'].startswith('ui.')]
        for name,wanted in scopes.items():
            if group['name'] not in wanted:continue
            token=deepcopy(by_key[(group['name'],name)]);definition=definitions[name]
            token['value']=definition.get('values',{}).get(group['name'],definition['value'])
            group['tokens'].append(token)
        fops.append({'type':'put-token-set','set':group})
    pops=[]
    for file in (product/'screens').glob('*.json'):
        before=load(file);after=migrate(before)
        if after!=before:pops.append({'type':'put-screen','screen':after})
    controls.batch(foundation,fops,'rules-prune-tokens')
    controls.batch(product,pops,'rules-token-scope-bindings')


def prepare(work):
    bases=load(work/'reviewed-bases.json')
    quiet.guard(bases['foundation'],bases['product'])
    compiled=work/'compiled';compiled.mkdir(exist_ok=False)
    controls.compile_controls(compiled,{d['name'] for d in ds.definitions()})
    shutil.copytree(work/'baseline'/'alva.smallpen',compiled/'alva.smallpen')
    saved=(author.INTENTS,author.LOG,ds.FOUNDATION,ds.ROOT,pages.PRODUCT,pages.ROOT)
    author.INTENTS=compiled/'source'/'intents';author.LOG=compiled/'source'/'authoring-log.jsonl'
    ds.FOUNDATION=compiled/'alva-foundation.smallpen';ds.ROOT=compiled
    pages.PRODUCT=compiled/'alva.smallpen';pages.ROOT=compiled
    try:
        pages.write()
        formal={d['module']+' / '+d['page'] for d in pages.definitions()}
        operations=[]
        for file in sorted((pages.PRODUCT/'screens').glob('*.json')):
            after=load(file)
            if after['name'] not in formal:continue
            before=load(work/'baseline'/'alva.smallpen'/'screens'/file.name)
            restored=pilot.retain_identity(before,deepcopy(after))
            quiet.preserve_navigation(before,restored)
            if restored!=after:operations.append({'type':'put-screen','screen':restored})
        controls.batch(pages.PRODUCT,operations,'rules-retain-identities')
        prune_tokens(compiled)
    finally:
        author.INTENTS,author.LOG,ds.FOUNDATION,ds.ROOT,pages.PRODUCT,pages.ROOT=saved
    print('Prepared',compiled,flush=True)


def apply(work):
    bases=load(work/'reviewed-bases.json')
    quiet.guard(bases['foundation'],bases['product'])
    compiled=work/'compiled';foundation=compiled/'alva-foundation.smallpen'
    definitions=load(foundation/'components'/'components.json')['componentSets']
    before_sets={c['id']:c for c in load(author.FOUNDATION/'components'/'components.json')['componentSets']}
    fops=[]
    for component in definitions:
        previous=before_sets[component['id']]
        if not component['name'].startswith('Alva '):
            if previous!=component:raise ValueError('Historical component changed')
            continue
        if {(v['id'],tuple(sorted(v['nodes']))) for v in component['variants']}!={(v['id'],tuple(sorted(v['nodes']))) for v in previous['variants']}:
            raise ValueError('Component or variant identity changed: '+component['name'])
        if component!=previous:fops.append({'type':'put-component-set','componentSet':component})
    fops.extend(quiet.token_operations(foundation))
    pops=[];formal={d['module']+' / '+d['page'] for d in pages.definitions()}
    for file in sorted((compiled/'alva.smallpen'/'screens').glob('*.json')):
        after=load(file);before=load(author.PRODUCT/'screens'/file.name)
        if after['name'] not in formal:
            if after!=before:raise ValueError('Historical page changed: '+before['name'])
            continue
        quiet.preserve_navigation(before,after)
        if before!=after:pops.append({'type':'put-screen','screen':after})
    # Complete both mutations before the first write. Each package has its own
    # guard; a second-package failure is reported, never called atomic success.
    pilot.batch(author.FOUNDATION,fops,'rules-foundation',bases['foundation'],author.ROOT)
    pilot.batch(author.PRODUCT,pops,'rules-product',bases['product'],author.ROOT)
    print('Applied',len(fops),'Foundation operations and',len(pops),'formal pages.',flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--work',type=Path,required=True)
    parser.add_argument('--apply',action='store_true');args=parser.parse_args()
    (apply if args.apply else prepare)(args.work)
