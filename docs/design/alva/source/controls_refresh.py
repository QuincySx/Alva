# INPUT: Reviewed Foundation/Product revisions and the native control definitions
# OUTPUT: Public CLI compiled variants and guarded updates preserving existing instance overrides/navigation
# POS: Design-only authoring; scratch compilation never hand-edits canonical packages.
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import shutil
import tempfile

import author
import design_system as ds

ROOT=author.ROOT
TARGETS={'Alva Icon','Alva Button','Alva IconButton','Alva ToggleButton','Alva Input'}


def import_icons(package,file):
    assets=json.loads((package/'assets'/'assets.json').read_text())
    font=next(f for f in assets['fonts'] if f['family']=='Alva Interface Icons')
    expected=hashlib.sha256(file.read_bytes()).hexdigest()
    current=next((v for v in font['variants'] if v['weight']==400 and v['style']=='normal'),None)
    if current and current['files']['woff']['sha256']==expected:return
    if current:author.cli('asset','font','delete',package,'--font','Alva Interface Icons','--variant',current['name'],label='controls-replace-icon-variant')
    author.cli('asset','font','import',package,'--family','Alva Interface Icons','--name','Regular','--file',file,label='controls-icon-font')


def batch(package,operations,label):
    if not operations:return
    current=author.cli('project','show',package,label=label+'-base')
    digest=hashlib.sha256(json.dumps(operations,sort_keys=True).encode()).hexdigest()[:16]
    payload={'baseRevision':current['revision'],'batchId':label+'-'+current['revision'][:12]+'-'+digest,'operations':operations}
    path=author.INTENTS/(label+'-'+digest+'.json');path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(payload,ensure_ascii=False,indent=2)+'\n')
    return author.cli('advanced','apply',package,'--batch',path,label=label)


def compile_controls(work,targets=None):
    scratch=work/'alva-foundation.smallpen'
    shutil.copytree(author.FOUNDATION,scratch)
    shutil.copytree(ROOT/'fonts',work/'fonts')
    (work/'source'/'intents').mkdir(parents=True)
    saved=(author.INTENTS,author.LOG,ds.ROOT,ds.FOUNDATION)
    author.INTENTS=work/'source'/'intents';author.LOG=work/'source'/'authoring-log.jsonl'
    ds.ROOT=work;ds.FOUNDATION=scratch
    try:
        existing=json.loads((scratch/'components'/'components.json').read_text())['componentSets']
        batch(scratch,[{'type':'delete-component-set','componentId':c['id']} for c in existing
            if c['name'].startswith('Alva ') and c['name']!='Alva Icon'],'controls-compiler-clear')
        import_icons(scratch,work/'fonts'/'AlvaInterfaceIcons-Regular.woff')
        ds.write(targets or TARGETS)
        return json.loads((scratch/'components'/'components.json').read_text())['componentSets']
    finally:
        author.INTENTS,author.LOG,ds.ROOT,ds.FOUNDATION=saved


def migrate_instances(components,screens):
    # Native node IDs stay stable. Content remains a variant; sizing belongs to
    # the shared height Token and the instance's content-driven layout.
    all_nodes={n['id']:n for c in components for v in c['variants'] for n in v['nodes'].values()}
    for screen in screens:
        for p in screen['presentations']:all_nodes.update(p['nodes'])
    icon_key='node_alva_button_button_icon:visible'
    label_key='node_alva_button_button_label:visible'
    def selection(node,selected):
        result=deepcopy(selected)
        if node.get('instance',{}).get('component',{}).get('assetId')=='cmp_alva_button':
            overrides=node['instance'].get('overrides',{})
            content='Icon' if overrides.get(label_key) is False else 'Text' if overrides.get(icon_key) is False else 'Leading'
            result.setdefault('axis_content',content);result.pop('axis_size',None)
        return result
    def migrate(node):
        if node.get('type')!='INSTANCE':return False
        before=deepcopy(node['instance']);inst=node['instance'];overrides=inst.get('overrides',{})
        inst['variant']=selection(node,inst['variant'])
        for path,value in list(overrides.items()):
            target_id,field=path.rsplit(':',1)
            if field=='variant':
                target=all_nodes[target_id.split('__')[-1]]
                overrides[path]=selection(target,value)
            if path.endswith(icon_key) and isinstance(value,bool):del overrides[path]
        if inst['component']['assetId']=='cmp_alva_button' and inst['variant']['axis_content']=='Icon':
            overrides.pop(label_key,None)
        return before!=inst
    c_ops=[];p_ops=[]
    for component in components:
        changed=deepcopy(component)
        touched=False
        for variant in changed['variants']:
            for node in variant['nodes'].values():touched=migrate(node) or touched
        if touched:c_ops.append({'type':'put-component-set','componentSet':changed})
    for screen in screens:
        for presentation in screen['presentations']:
            for node in presentation['nodes'].values():
                changed=deepcopy(node)
                if migrate(changed):
                    location={'screenId':screen['id'],'presentationId':presentation['id'],'nodeId':node['id']}
                    if node['instance']['variant']!=changed['instance']['variant']:
                        p_ops.append({'type':'select-instance-variant',**location,'selection':changed['instance']['variant']})
                    before=node['instance'].get('overrides',{});after=changed['instance'].get('overrides',{})
                    for path,value in after.items():
                        if path not in before or before[path]!=value:p_ops.append({'type':'set-instance-override',**location,'overridePath':path,'value':value})
                    for path in before:
                        if path not in after:p_ops.append({'type':'clear-instance-override',**location,'overridePath':path})
    return c_ops,p_ops


def rebind_tokens(value,compiled_package):
    def index(package):
        data=json.loads((package/'tokens'/'tokens.json').read_text())
        return {(s['name'],t['name']):t['id'] for s in data['sets'] for t in s['tokens']}
    target=index(author.FOUNDATION)
    mapping={token:target[name] for name,token in index(compiled_package).items() if name in target}
    def visit(node):
        if isinstance(node,list):return [visit(v) for v in node]
        if isinstance(node,dict):return {k:mapping.get(v,v) if k=='assetId' else visit(v) for k,v in node.items()}
        return node
    return visit(value)


def refresh(foundation_revision,product_revision,compiled=None):
    for package,revision in [(author.FOUNDATION,foundation_revision),(author.PRODUCT,product_revision)]:
        current=author.cli('project','show',package,label='controls-reviewed-base')
        if current['revision']!=revision:raise SystemExit('Package changed after review; inspect before refreshing controls.')
    work=Path(tempfile.mkdtemp(prefix='alva-controls-compile-'))
    print('Compiler workspace:',work,flush=True)
    if compiled:
        controls=json.loads(compiled.read_text())['componentSets']
    else:
        controls=compile_controls(work)
        compiled=work/'alva-foundation.smallpen'/'components'/'components.json'
    foundation=json.loads((author.FOUNDATION/'components'/'components.json').read_text())['componentSets']
    screens=[json.loads(p.read_text()) for p in (author.PRODUCT/'screens').glob('*.json')]
    c_ops,p_ops=migrate_instances(foundation,screens)
    by_id={op['componentSet']['id']:op for op in c_ops}
    for control in controls:
        if control['name'] in TARGETS:by_id[control['id']]={'type':'put-component-set','componentSet':control}
    # Recheck both bases after compilation; do not supersede edits made in the App.
    for package,revision in [(author.FOUNDATION,foundation_revision),(author.PRODUCT,product_revision)]:
        if author.cli('project','show',package,label='controls-final-guard')['revision']!=revision:raise SystemExit('Concurrent package edit; nothing applied.')
    author.cli('token','set',author.FOUNDATION,intent={'tokens':ds.token_definitions()},label='controls-semantic-tokens-'+foundation_revision[:12])
    operations=rebind_tokens(list(by_id.values()),compiled.parent.parent)
    for package in (author.FOUNDATION,author.PRODUCT):
        import_icons(package,ROOT/'fonts'/'AlvaInterfaceIcons-Regular.woff')
    batch(author.FOUNDATION,operations,'controls-component-variants')
    batch(author.PRODUCT,p_ops,'controls-instance-selections')
    print('Migrated',len(c_ops),'dependent component definitions with',len(p_ops),'page selection/override operations; existing nodes and interactions retained.',flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--foundation-revision',required=True);parser.add_argument('--product-revision',required=True)
    parser.add_argument('--compiled',type=Path,help='Already reviewed isolated compiler component file')
    args=parser.parse_args();refresh(args.foundation_revision,args.product_revision,args.compiled)
