# INPUT: Existing Alva components, two reviewed workbench pages and current package revisions
# OUTPUT: A two-page reading/wayfinding pilot with native instances, preserved identities and guarded CLI writes
# POS: Design-only reading/wayfinding composition using shared Token roles; no runtime scrolling or animation claims.
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import shutil

import author
import design_system as ds

PAGES={'21 新任务','22 对话与面板'}
READING_SIZE=ds.ref('font.size.subtitle')


def find(tree,name):
    if tree['name']==name:return tree
    for child in tree.get('children',[]):
        found=find(child,name)
        if found:return found
    return None


def decorate(definition):
    if definition['page'] not in PAGES:return definition
    d=deepcopy(definition)
    # The page root has page/module rather than an element name.
    tree={'name':'Root','children':d['children']}
    def node(name):
        found=find(tree,name)
        if found is None:raise ValueError('Missing pilot element: '+name)
        return found
    if d['page']=='21 新任务':
        welcome=node('WelcomeContent')
        badge=ds.instance('WelcomeBadge','Avatar',{'Kind':'Agent'},texts={'Label':'a'},width=40,height=40)
        welcome['children'][0]=ds.row('WelcomeIdentity',[badge,ds.text('WelcomeIdentityLabel','ALVA · 你的工作伙伴','body','secondary',width='hug',visible=False)],width='hug',gap=12,visible=True)
        welcome.update(maxWidth=760,gap=ds.ref('space.3'))
        node('Welcome').update(padding=ds.ref('space.6'))
        node('WelcomeTitle').update(text='今天，想完成什么？',margin=[0,0,4,0])
        node('WelcomeDescription').update(text='把目标告诉我，一起把想法变成看得见的成果。',fontSize=READING_SIZE,lineHeight=ds.ref('line.reading'),margin=[0,0,8,0],visible=False)
        for name in ('ReadProject','Research','Review'):
            node(name)['props']['Kind']='Secondary'
        node('Suggestions').update(gap=12,margin=[0,0,0,0])
        node('LocalHint').update(text='文件、模型和权限，都可以在输入区找到。',fontSize=ds.ref('font.size.body'))
        node('Hints').update(gap=8,margin=[4,0,0,0],visible=False)
    else:
        node('UserText').update(fontSize=READING_SIZE,lineHeight=ds.ref('line.reading'))
        node('UserMessage').update(fill=ds.ref('color.accentSoft'),padding=16)
        node('AssistantBody').update(text='我会把方案分成模型连接、伙伴协作和执行反馈三个部分。\n\n选择模型时，一起说明连接来源、执行 Agent 和可用参数；伙伴保留自己的职责与 Skills，需要你确认的操作会单独提示。',fontSize=READING_SIZE,lineHeight=ds.ref('line.reading'))
        node('AssistantSummaryBody').update(text='主体结构已完成，正在补充交互细节。你可以先看右侧草稿。',fontSize=READING_SIZE,lineHeight=ds.ref('line.reading'))
        node('AssistantSummaryHeader')['visible']=False
        node('AssistantSummary').update(gap=0)
        node('ToolActivity')['props']['State']='Running'
        node('ToolActivity')['text']['Label']='正在补充交互细节…'
        node('Artifact')['set']['Copy.Description.text']='草稿 · 3 个章节'
        node('Completion').update(text='草稿已保存',fontSize=ds.ref('font.size.body'),visible=False)
        conversation=node('Conversation')
        items=conversation['children']
        transcript=ds.box('Transcript',items[:-2],width='fill',height='fill',minHeight=0,gap=ds.ref('space.4'))
        conversation['children']=[transcript,*items[-2:]]
        conversation.update(padding=ds.ref('space.6'),gap=ds.ref('space.4'))
        node('Composer')['props']['State']='Running'
        node('ThreadState').update(text='正在整理 · 可补充说明',fontSize=ds.ref('font.size.body'),visible=False)
        document=node('Document')
        sections=[]
        for i in range(1,4):
            heading=node('DocH'+str(i));body=node('DocP'+str(i))
            heading.update(fontSize=READING_SIZE,lineHeight=ds.ref('line.normal'))
            body.update(fontSize=READING_SIZE,lineHeight=ds.ref('line.reading'))
            sections.append(ds.box('DocSection'+str(i),[heading,body],width='fill',height='hug',gap=8))
        document['children']=[node('DocumentType'),node('DocumentTitle'),node('DocumentLead'),*sections,node('DocumentSpace'),node('DocumentFooter')]
        document.update(gap=ds.ref('space.6'),padding=ds.ref('space.6'))
        node('DocumentLead').update(text='以对话为中心，让资料、执行与成果各有位置。',fontSize=READING_SIZE,lineHeight=ds.ref('line.reading'))
        node('DocumentType')['visible']=False
        node('DocumentSaved').update(text='已保存',fontSize=ds.ref('font.size.body'))
        node('FilePath').update(layout='row',align='center',gap=8,padding=[8,16])
        node('FilePath')['children'].append(ds.icon_button('FindInDocument','Search'))
    return d


def finish(definition,revision):
    if definition['page'] not in PAGES:return revision
    import product_pages as pages
    sets,tokens,package_id=ds.native_context()
    screen=next(json.loads(p.read_text()) for p in (pages.PRODUCT/'screens').glob('*.json') if json.loads(p.read_text())['name']==definition['module']+' / '+definition['page'])
    operations=[]
    for pres in screen['presentations']:
        def instance(name):return next(n for n in pres['nodes'].values() if n['name']==name)
        def target(owner,path):
            current=owner;prefix=[]
            c=sets[current['instance']['component']['assetId']]
            variant=next(v for v in c['variants'] if v['selection']==current['instance']['variant'])
            if not path:return variant['nodes'][variant['rootId']],variant['rootId']
            parts=path.split('.')
            for index,name in enumerate(parts):
                matches=[n for n in variant['nodes'].values() if n['name']==name]
                if len(matches)!=1:raise ValueError((owner['name'],path,name,len(matches)))
                current=matches[0]
                key='__'.join([*prefix,current['id']])
                if index<len(parts)-1 and current['type']=='INSTANCE':
                    prefix.append(current['id'])
                    c=sets[current['instance']['component']['assetId']]
                    selection=owner['instance'].get('overrides',{}).get(key+':variant',current['instance']['variant'])
                    variant=next(v for v in c['variants'] if v['selection']==selection)
            return current,key
        def change(name,path,**changes):
            owner=instance(name);source,key=target(owner,path)
            overrides=owner['instance'].setdefault('overrides',{})
            bindings=deepcopy(overrides.get(key+':tokenBindings',source.get('tokenBindings',{})))
            for field,value in changes.items():
                if field=='tokenBindings':
                    bindings.update(value)
                    continue
                if field=='fills':bindings.pop('fill',None)
                elif field=='strokes':bindings.pop('stroke',None)
                elif field=='textStyle':
                    style=deepcopy(overrides.get(key+':textStyle',source.get('textStyle',{})))
                    style.update(value);value=style
                    for prop in changes['textStyle']:bindings.pop(prop,None)
                else:bindings.pop(field,None)
                overrides[key+':'+field]=value
            overrides[key+':tokenBindings']=bindings
        def fill(color):return [{'type':'solid','color':color}]
        def themed(name,path,**roles):
            changes={};bindings={};style={}
            for field,role in roles.items():
                token=tokens['ui.'+role]
                bindings[field]={'packageId':package_id,'assetId':token['id']}
                if field=='fill':changes['fills']=fill(token['value'])
                elif field=='stroke':changes['strokes']=[{'type':'solid','color':token['value'],'width':1,'alignment':'inner'}]
                elif field in ('fontFamily','fontSize','fontWeight','lineHeight'):style[field]=token['value']
                else:changes[field]=token['value']
            if style:changes['textStyle']=style
            changes['tokenBindings']=bindings
            change(name,path,**changes)
        # Identity uses the same theme-aware roles as the action controls.
        themed('Sidebar','BrandMark',fill='color.accent')
        change('Sidebar','Mark.Glyph',text='a')
        themed('Sidebar','Mark.Glyph',fill='color.onAccent',fontFamily='font.family',fontSize='font.size.subtitle',fontWeight='font.weight.strong')
        change('Sidebar','Search.Label',text='搜索任务与资料')
        change('Sidebar','CurrentThread.Label',text='整理桌面工作台')
        change('Composer','Prompt',text='补充说明，或调整下一步…' if definition['page']=='22 对话与面板' else '告诉我你要完成什么，或 @ 引用资料…')
        themed('Composer','Prompt',fontSize='font.size.subtitle',lineHeight='line.reading')
        if definition['page']=='21 新任务':
            themed('WelcomeBadge','',fill='color.accent',cornerRadius='radius.ctrl')
            themed('WelcomeBadge','Label',fill='color.onAccent',fontSize='font.size.heading',fontWeight='font.weight.strong')
            change('Composer','Space',height=20)
        else:
            change('Composer','Space',height=20)
            change('Composer','Controls.Send.Label',text='停止')
            for name in ('AssistantHeader','AssistantSummaryHeader'):
                themed(name,'Avatar',fill='color.accent',cornerRadius='radius.ctrl')
                change(name,'Avatar.Label',text='a')
                themed(name,'Avatar.Label',fill='color.onAccent',fontSize='font.size.subtitle')
                themed(name,'Author',fontSize='font.size.body')
            change('ToolActivity','Icon',variant={'axis_name':'Loader'})
            change('ToolActivity','Icon.Glyph',fills=fill(tokens['ui.color.secondary']['value']),tokenBindings={'fill':{'packageId':package_id,'assetId':tokens['ui.color.secondary']['id']}})
            themed('ToolActivity','Label',fontSize='font.size.body')
            themed('Artifact','',stroke='color.border')
            themed('Artifact','Copy.Title',fontSize='font.size.subtitle')
            themed('Artifact','Copy.Description',fontSize='font.size.body')
            themed('Artifact','Icon',fill='color.accent',cornerRadius='radius.small')
            themed('Artifact','Icon.Glyph',fill='color.onAccent',fontSize='font.size.iconSmall')
            themed('PanelHeader','FileTab.Icon',fill='color.accent',cornerRadius='radius.small')
            themed('PanelHeader','FileTab.Icon.Glyph',fill='color.onAccent',fontSize='font.size.iconSmall')
            change('PanelHeader','FileTab.Label',visible=True,fills=fill(tokens['ui.color.text']['value']),tokenBindings={'fill':{'packageId':package_id,'assetId':tokens['ui.color.text']['id']}})
            change('PanelHeader','TaskTab.Label',visible=True)
            # Explicit viewport for the transcript; real scrolling belongs to the product.
            transcript=instance('Transcript')
            operations.append({'type':'update-presentation-node','screenId':screen['id'],'presentationId':pres['id'],'nodeId':transcript['id'],'changes':{'show-content':False}})
        for node in pres['nodes'].values():
            if node['type']!='INSTANCE':continue
            for key,value in node['instance'].get('overrides',{}).items():
                operations.append({'type':'set-instance-override','screenId':screen['id'],'presentationId':pres['id'],'nodeId':node['id'],'overridePath':key,'value':value})
    return batch(pages.PRODUCT,operations,'reading-page-'+definition['page'][:2],revision,pages.ROOT)


def batch(package,operations,label,revision,root):
    digest=hashlib.sha256(json.dumps(operations,sort_keys=True).encode()).hexdigest()[:16]
    path=root/'source'/'intents'/(label+'-'+digest+'.json');path.parent.mkdir(parents=True,exist_ok=True)
    payload={'baseRevision':revision,'batchId':label+'-'+revision[:12]+'-'+digest,'operations':operations}
    path.write_text(json.dumps(payload,ensure_ascii=False,indent=2)+'\n')
    return author.cli('advanced','apply',package,'--batch',path,label=label)['revision']


def retain_identity(before,after):
    if before['id']!=after['id']:raise ValueError('Page identity changed')
    for old,new in zip(before['presentations'],after['presentations'],strict=True):
        if old['id']!=new['id']:raise ValueError('Presentation identity changed')
        old_names={n['name']:n for n in old['nodes'].values()}
        new_names={n['name']:n for n in new['nodes'].values()}
        if len(old_names)!=len(old['nodes']) or len(new_names)!=len(new['nodes']):raise ValueError('Ambiguous names')
        if not old_names.keys()<=new_names.keys():raise ValueError('Existing elements removed')
        mapping={new_names[name]['id']:node['id'] for name,node in old_names.items()}
        nodes={}
        for node in new['nodes'].values():
            node['id']=mapping.get(node['id'],node['id'])
            node['children']=[mapping.get(c,c) for c in node['children']]
            previous=old_names.get(node['name'],{})
            if 'interactions' in previous:node['interactions']=deepcopy(previous['interactions'])
            if node.get('instance'):
                for key,value in previous.get('instance',{}).get('overrides',{}).items():
                    if key.endswith(':interactions'):node['instance'].setdefault('overrides',{})[key]=deepcopy(value)
            nodes[node['id']]=node
        new['nodes']=nodes;new['rootId']=mapping.get(new['rootId'],new['rootId'])
        for field in ('interactions','prototypeFlows'):
            if field in old:new[field]=deepcopy(old[field])
    return after


def prepare(work):
    import product_pages as pages
    compiled=work/'compiled';compiled.mkdir()
    for name in ('alva.smallpen','alva-foundation.smallpen'):
        shutil.copytree(work/'baseline'/name,compiled/name)
    shutil.copytree(author.ROOT/'fonts',compiled/'fonts')
    (compiled/'source'/'intents').mkdir(parents=True)
    bases={}
    for kind,package in [('foundation',author.FOUNDATION),('product',author.PRODUCT)]:
        bases[kind]=author.cli('project','show',package,label='reading-base')['revision']
        copied=author.cli('project','show',compiled/package.name,label='reading-copy-base')['revision']
        if copied!=bases[kind]:raise ValueError('Baseline no longer current')
    (work/'bases.json').write_text(json.dumps(bases,indent=2)+'\n')
    author.INTENTS=compiled/'source'/'intents';author.LOG=compiled/'source'/'authoring-log.jsonl'
    pages.PRODUCT=compiled/'alva.smallpen';pages.ROOT=compiled
    pages.write(PAGES)
    product=compiled/'alva.smallpen'
    revision=author.cli('project','show',product,label='reading-preserve-base')['revision']
    ops=[]
    for f in (product/'screens').glob('*.json'):
        after=json.loads(f.read_text())
        if after['name'].split(' / ',1)[-1] not in PAGES:continue
        before=json.loads((work/'baseline'/'alva.smallpen'/'screens'/f.name).read_text())
        ops.append({'type':'put-screen','screen':retain_identity(before,after)})
    batch(product,ops,'reading-preserve-identities',revision,compiled)
    print('Prepared',compiled,flush=True)


def apply(work):
    bases=json.loads((work/'bases.json').read_text())
    for kind,package in [('foundation',author.FOUNDATION),('product',author.PRODUCT)]:
        if author.cli('project','show',package,label='reading-final-guard')['revision']!=bases[kind]:raise ValueError('Concurrent edit; inspect before applying')
    operations=[]
    for f in (work/'compiled'/'alva.smallpen'/'screens').glob('*.json'):
        screen=json.loads(f.read_text())
        if screen['name'].split(' / ',1)[-1] in PAGES:operations.append({'type':'put-screen','screen':screen})
    if len(operations)!=2:raise ValueError('Expected exactly two pilot pages')
    revision=batch(author.PRODUCT,operations,'reading-pilot',bases['product'],author.ROOT)
    print('Applied two pilot pages:',revision,flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--work',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    args=parser.parse_args()
    (apply if args.apply else prepare)(args.work)
