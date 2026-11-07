# INPUT: Revision-bound SmallPen native exports and declared/actual prototype links
# OUTPUT: Offline image/hotspot preview and explicit graph verification records
# POS: Review aid only; does not redraw the design, save configuration or execute requests.
from pathlib import Path
import json
import subprocess

from author import PRODUCT, ROOT
from flows import PAGES, OVERLAYS, definitions

LABELS = {
    "Overview": "工作区",
    "Conversation": "对话",
    "WaitingQuestion": "对话 · 等待回答",
    "Focus": "对话 · 专注模式",
    "Details": "对话 · 停靠详情",
    "Running": "对话 · 执行中",
    "ApprovalPending": "对话 · 待审批",
    "Stopped": "对话 · 已停止",
    "Error": "对话 · 失败",
    "Plugins": "插件与能力",
    "Library": "资料与记忆",
    "Team": "协作",
    "Automations": "自动化",
    "Waits": "等待任务",
    "AnsweredWaits": "等待任务 · 回答已保存",
    "General": "设置 · 外观与布局",
    "Routes": "设置 · 模型与路由",
    "Capabilities": "设置 · 工具与连接",
    "Rules": "设置 · 规则与 Skills",
    "Storage": "设置 · 资料与存储",
    "Security": "设置 · 权限与安全",
    "Gateway": "设置 · Gateway",
    "Advanced": "设置 · 高级与诊断",
    "IntegrationTests": "开发者 · 接入测试",
    "Evaluation": "开发者 · 模型评估",
    "Question": "提问 · 未选择",
    "QuestionSelected": "提问 · 已选择 A",
    "QuestionSelectedB": "提问 · 已选择 B",
    "QuestionOther": "提问 · Other",
    "Approval": "权限审批",
    "Inspector": "运行详情",
    "CommandPalette": "快速打开",
    "NewSession": "创建对话",
    "Artifact": "展开动态内容",
}

HTML = """<!-- INPUT: SmallPen native PNGs, measured geometry and verified prototype links -->
<!-- OUTPUT: Offline review navigation and illustrative overlays, without backend calls -->
<!-- POS: Design review artifact; the editable source remains the adjacent SmallPen packages. -->
<!doctype html><html lang="zh-CN"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Alva · SmallPen 原型</title>
<style>
*{box-sizing:border-box}body{margin:0;background:#e8edf5;color:#17243a;font:14px system-ui,sans-serif}
header{display:flex;align-items:center;gap:12px;flex-wrap:wrap;padding:12px 18px;background:#fff;border-bottom:1px solid #cbd5e2;position:sticky;top:0;z-index:10}
header strong{font-size:16px;margin-right:8px}select,button,a.control{font:inherit;border:1px solid #c9d3e3;background:#fff;color:#17243a;border-radius:7px;padding:7px 10px;text-decoration:none;cursor:pointer}
select{max-width:290px}button:focus-visible,select:focus-visible,a:focus-visible{outline:3px solid #285eea;outline-offset:2px}
.spacer{flex:1}.hint{font-size:12px;color:#526477}.status{display:flex;gap:20px;justify-content:space-between;padding:9px 20px;font-size:12px;color:#526477}
#stage{padding:12px 20px 32px;overflow:auto;min-height:75vh}#screen{position:relative;margin:auto;box-shadow:0 12px 50px #162b4f1c;line-height:0}
.frame{position:relative;background:#fff}img.design{width:100%;display:block;height:auto}.hotspot{position:absolute;padding:0;border:0;border-radius:3px;background:transparent}
.hotspot:focus-visible{outline:3px solid #e36212;outline-offset:1px;background:#285eea18}.hints .hotspot{outline:1px dashed #285eea80;background:#285eea12}
#overlay{position:fixed;inset:67px 0 0;z-index:5;display:none;align-items:center;justify-content:center;padding:24px;background:#0e1b376b}
#overlay.open{display:flex}#overlay.drawer{justify-content:flex-end;align-items:flex-start;padding-top:20px;background:transparent;pointer-events:none}#overlay.drawer #overlay-card{pointer-events:auto}#overlay-card{position:relative;box-shadow:0 18px 80px #09112566;border-radius:12px;overflow:auto;max-height:calc(100vh - 120px);line-height:0}
#overlay-card .frame{border-radius:12px;overflow:hidden}.footer{padding:0 20px 20px;color:#526477;font-size:12px}
.footer a{color:#244cad}#missing{padding:32px;background:#fff;line-height:1.6}
@media(max-width:900px){header{gap:7px}header strong{display:none}.status{display:block}.hint{display:none}#stage{padding:6px}#overlay{inset:120px 0 0;padding:12px}}
</style>
<header><strong>ALVA / 设计原型</strong><button id="back">返回</button><select id="page" aria-label="设计页面"></select>
<select id="theme" aria-label="预览主题"><option value="light">浅色</option><option value="dark">深色</option></select>
<select id="width" aria-label="预览宽度"><option value="desktop">1440 桌面</option><option value="narrow">1024 窄窗口</option></select>
<button id="fit">适应宽度</button><button id="actual">1:1</button><button id="hints" aria-pressed="false">显示热点</button>
<select id="tour" aria-label="流程起点"><option value="">流程演示…</option><option value="question">提问 / Other / 继续</option><option value="approval">审批 / 允许 / 拒绝</option><option value="running">执行 / 停止</option><option value="error">失败 / 重试</option></select>
<span class="spacer"></span><a class="control" href="DESIGN.md">界面契约</a><button id="system">Design System</button></header>
<div class="status"><span id="caption"></span><span>SmallPen 原生图与坐标热点；示例流程，不保存配置、不发送请求。</span></div>
<main id="stage"><div id="screen"></div></main><div id="overlay"><div id="overlay-card" role="dialog" aria-modal="true" aria-label="原型弹层"></div></div>
<div class="footer"><span>可编辑设计：相邻的 alva.smallpen 与 alva-foundation.smallpen</span> · <a href="review.json">实际检查记录</a> · Esc 关闭弹层，恢复原入口焦点。控件只展示样式时不会发起真实操作。</div>
<script>
'use strict';
const DATA=__PAYLOAD__;
const $=id=>document.getElementById(id);
let page='Workspace / Overview',overlay=null,history=[],fit=true,system=false,originFocus=null;
const choices=$('page'),theme=$('theme'),width=$('width');
DATA.pages.forEach(p=>{const o=document.createElement('option');o.value=p.page;o.textContent=p.label;choices.append(o)});
function combo(){return theme.value+'-'+width.value}
function item(p){return DATA.frames[p]&&DATA.frames[p][combo()]}
function label(p){const x=DATA.pages.find(x=>x.page===p);return x?x.label:p}
function setFrame(host,p){
 host.replaceChildren();const f=item(p);if(!f){const msg=document.createElement('p');msg.id='missing';msg.textContent='此组合没有导出，请查看 review.json。';host.append(msg);return}
 const img=document.createElement('img');img.className='design';img.src=f.png;img.alt=label(p)+' · '+combo();img.width=f.width;img.height=f.height;host.append(img);
 f.hotspots.forEach(h=>{const b=document.createElement('button');b.className='hotspot';b.type='button';b.dataset.element=h.element;b.dataset.owner=p;b.setAttribute('aria-label',h.label);b.title=h.label+' → '+(h.toPage?label(h.toPage):'返回');
 b.style.left=(100*h.x/f.width)+'%';b.style.top=(100*h.y/f.height)+'%';b.style.width=(100*h.width/f.width)+'%';b.style.height=(100*h.height/f.height)+'%';b.onclick=()=>activate(h,b);host.append(b)});
}
function render(){
 const root=$('screen');root.replaceChildren();const f=item(page);const available=$('stage').clientWidth-40;
 if(system){const d=DATA.systems[theme.value];const img=document.createElement('img');img.className='design';img.src=d.png;img.alt='SmallPen 自动生成的 Design System';root.style.width=(fit?Math.min(available,1200):d.width)+'px';root.append(img)}
 else if(f){root.style.width=(fit?Math.min(available,f.width):f.width)+'px';const frame=document.createElement('div');frame.className='frame';root.append(frame);setFrame(frame,page)}
 if(overlay&&!system){const data=item(overlay),card=$('overlay-card');card.style.width=Math.min(data?data.width:560,window.innerWidth-48)+'px';setFrame(card,overlay);$('overlay').classList.add('open');$('overlay').classList.toggle('drawer',overlay==='Overlay / Inspector');card.setAttribute('role',overlay==='Overlay / Inspector'?'complementary':'dialog');card.setAttribute('aria-modal',overlay==='Overlay / Inspector'?'false':'true');if(overlay==='Overlay / Inspector'){const r=root.getBoundingClientRect(),scale=r.width/f.width;card.style.position='fixed';card.style.top=(r.top+100*scale)+'px';card.style.right=Math.max(0,window.innerWidth-r.right)+'px';card.style.width=(Math.min(data.width,f.width-64)*scale)+'px';card.style.maxHeight=Math.max(160,Math.min(r.bottom,window.innerHeight-16)-(r.top+100*scale))+'px'}else{card.style.position='relative';card.style.top='';card.style.right='';card.style.maxHeight=''}}
 else{$('overlay').classList.remove('open');$('overlay-card').replaceChildren()}
 choices.value=overlay||page;$('caption').textContent=(system?'Design System':label(overlay||page))+' · '+combo()+' · revision '+DATA.revision.slice(0,12);
}
function remember(){history.push({page,overlay,system})}
function showPage(target){remember();page=target;overlay=null;system=false;render();$('stage').scrollTop=0}
function openOverlay(target,source){if(!overlay){originFocus=source?{element:source.dataset.element,owner:source.dataset.owner,id:source.id}:null;remember()}overlay=target;system=false;render();requestAnimationFrame(()=>{$('overlay-card').querySelector('button')?.focus()})}
function closeOverlay(){if(!overlay)return;overlay=null;render();if(history.length&&history[history.length-1].page===page)history.pop();const restored=originFocus&&(originFocus.element?document.querySelector('[data-owner="'+CSS.escape(originFocus.owner)+'"][data-element="'+CSS.escape(originFocus.element)+'"]'):document.getElementById(originFocus.id));(restored||$('page')).focus()}
function activate(h,source){
 if(h.action==='back'){if(overlay){closeOverlay();return}goBack();return}
 if(h.action==='overlay'||(h.toPage&&h.toPage.startsWith('Overlay / '))){openOverlay(h.toPage,source);return}
 showPage(h.toPage);
}
function goBack(){if(overlay){closeOverlay();return}const previous=history.pop();if(previous){page=previous.page;overlay=previous.overlay;system=previous.system;render()}}
choices.onchange=()=>{const target=choices.value;if(target.startsWith('Overlay / ')){page=target.includes('Question')?'Chat / WaitingQuestion':target.endsWith('Approval')?'Chat / ApprovalPending':'Chat / Conversation';openOverlay(target,choices)}else showPage(target)};
theme.onchange=render;width.onchange=render;$('fit').onclick=()=>{fit=true;render()};$('actual').onclick=()=>{fit=false;render()};$('back').onclick=goBack;
$('hints').onclick=()=>{document.body.classList.toggle('hints');$('hints').setAttribute('aria-pressed',document.body.classList.contains('hints')?'true':'false')};
$('system').onclick=()=>{remember();overlay=null;system=true;render()};
$('tour').onchange=()=>{const target={question:'Chat / WaitingQuestion',approval:'Chat / ApprovalPending',running:'Chat / Running',error:'Chat / Error'}[$('tour').value];if(target)showPage(target);$('tour').value=''};
$('overlay').onclick=e=>{if(e.target===$('overlay'))closeOverlay()};
document.addEventListener('keydown',e=>{
 if(e.key==='Escape'&&overlay){e.preventDefault();closeOverlay();return}
 if(e.key==='Tab'&&overlay&&overlay!=='Overlay / Inspector'){const nodes=[...$('overlay-card').querySelectorAll('button')];if(!nodes.length)return;const first=nodes[0],last=nodes[nodes.length-1];
 if(e.shiftKey&&document.activeElement===first){e.preventDefault();last.focus()}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first.focus()}}
});
window.addEventListener('resize',render);render();
</script></html>
"""


def main():
    review_path = ROOT / "review.json"
    review = json.loads(review_path.read_text())
    native = json.loads(
        subprocess.check_output(
            ["smallpen", "flow", "list", str(PRODUCT), "--json", "--full", "--stdout"],
            text=True,
        )
    )
    actual = {
        (x["page"], x["element"].split(" / ")[-1], x["action"], x.get("to"))
        for x in native["links"]
    }
    links = definitions()
    for x in links:
        expected = (
            x["fromPage"],
            x["element"],
            "navigate" if x["action"] == "overlay" else x["action"],
            x["toPage"],
        )
        if expected not in actual:
            raise RuntimeError("Missing canonical link: " + repr(expected))
    if native["revision"] != review["packages"]["product"]["revision"]:
        raise RuntimeError("Prototype links and exports use different revisions")
    frames = {}
    for e in review["exports"]:
        evidence = json.loads((ROOT / e["evidence"]).read_text())
        if (
            evidence["revision"] != e["revision"]
            or evidence["snapshotId"] != e["layoutHash"]
        ):
            raise RuntimeError("Mismatched export evidence: " + e["evidence"])
        nodes = {n["name"]: n for n in evidence["nodes"]}
        hotspots = []
        for link in [x for x in links if x["fromPage"] == e["page"]]:
            n = nodes.get(link["element"])
            if n is None:
                raise RuntimeError("Missing source: " + repr(link))
            if not n["geometry"].get("visible", True):
                continue
            g = n["geometry"]["bounds"]
            name = next(
                (
                    o["value"]
                    for o in n.get("source", {}).get("overrides", [])
                    if o.get("element") == "Label" and o.get("field") == "text"
                ),
                n["name"],
            )
            hotspots.append({**link, **g, "label": name})
        frames.setdefault(e["page"], {})[e["combination"]] = {
            k: e[k] for k in ["png", "width", "height"]
        }
        frames[e["page"]][e["combination"]]["hotspots"] = hotspots
    tours = [
        (
            "create",
            [
                "Workspace / Overview",
                "CreateProject",
                "NewSessionCreate",
                "ToggleSidebar",
                "ExitFocus",
            ],
        ),
        (
            "other-save-resume-stop",
            [
                "Chat / WaitingQuestion",
                "AnswerQuestion",
                "OptionOther",
                "QuestionSubmit",
                "ResumeOriginal",
                "StopTask",
            ],
        ),
        ("approval-deny", ["Chat / ApprovalPending", "ReviewApproval", "ApprovalDeny"]),
        (
            "approval-allow-stop",
            ["Chat / ApprovalPending", "ReviewApproval", "ApprovalAllow", "StopTask"],
        ),
        (
            "tests-evaluation",
            ["Developer / IntegrationTests", "EvaluationTab", "IntegrationTab"],
        ),
    ]
    verified = []
    for name, steps in tours:
        page = steps[0]
        states = [page]
        for element in steps[1:]:
            matches = [
                x for x in links if x["fromPage"] == page and x["element"] == element
            ]
            if len(matches) != 1 or not matches[0]["toPage"]:
                raise RuntimeError(
                    "Invalid tour " + name + " at " + page + " / " + element
                )
            page = matches[0]["toPage"]
            states.append(page)
        verified.append(
            {
                "name": name,
                "states": states,
                "status": "graph-verified",
                "scope": "navigation only; not UI input or backend execution",
            }
        )
    payload = {
        "revision": native["revision"],
        "frames": frames,
        "pages": [{"page": page, "label": LABELS[key]} for key, page in PAGES.items()]
        + [{"page": "Overlay / " + key, "label": LABELS[key]} for key in OVERLAYS],
        "systems": {
            ("dark" if "dark" in d["combination"] else "light"): d
            for d in review["designSystem"]
        },
    }
    data = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).replace(
        "</", "<\\/"
    )
    (ROOT / "preview.html").write_text(HTML.replace("__PAYLOAD__", data))
    review["prototype"] = {
        "revision": native["revision"],
        "canonicalCounts": native["counts"],
        "declaredLinks": len(links),
        "canonicalLinkMatch": "all matched",
        "tours": verified,
        "nativeOverlayLimit": "Cross-Presentation overlays are represented by native navigate links; the offline image viewer demonstrates the intended overlay presentation.",
    }
    review_path.write_text(json.dumps(review, ensure_ascii=False, indent=2) + "\n")
    (ROOT / "exports" / "archive" / "prototype-links.json").write_text(
        json.dumps(native, ensure_ascii=False, indent=2) + "\n"
    )
    print(
        "Built offline preview; verified",
        len(links),
        "native links and",
        len(verified),
        "navigation tours",
    )


if __name__ == "__main__":
    main()
