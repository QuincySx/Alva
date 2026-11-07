# INPUT: Reviewed SmallPen workspace package, model/provider sample data and the unified selector brief
# OUTPUT: One model-first picker with favorites, Provider filters and a left-side Agent/options panel
# POS: UI prototype authoring only; declares no production model availability or execution capability.
import argparse
import hashlib
import json
from pathlib import Path

from author import PRODUCT, ROOT, cli
from workbench import CANVAS, action, box, row, t

PROVIDERS = {
    "openai": ("O", "OpenAI · 主连接"),
    "anthropic": ("A", "Anthropic · 主连接"),
    "google": ("G", "Google · 主连接"),
    "relay": ("R", "中转 A · 工作连接"),
}
AGENTS = ("ALVA", "Codex", "Pi")  # Illustrative compatible set for the hovered route.
MODELS = [("GPT", "openai", "视觉", "高"), ("GPT Mini", "openai", "文本", "中"),
          ("Sonnet", "anthropic", "视觉", "中"), ("Opus", "anthropic", "视觉", "高"),
          ("Gemini Pro", "google", "视觉", "高"), ("Sonnet", "relay", "文本", "中")]
PAGE = "05 Agent 与模型"


def agent_item(agent, selected):
    return box("Agent"+agent,[t("Agent"+agent+"Label",agent,12,width="hug")],
        width="fill",height=30,align="center",justify="center",radius=5,
        fill="#E5E5E5" if selected else "#F5F5F5",**({"stroke":"#BDBDBD"} if selected else {}))


def filter_button(name, label, selected=False):
    return box(name,[t(name+"Text",label,11,width="hug")],
        width="fill",height=36,align="center",justify="center",radius=6,
        **({"fill":"#E4E4E4","stroke":"#C8C8C8"} if selected else {}))


def model_item(i, model, provider, capability, effort):
    controls=[]
    if i==0:
        controls=[box("ModelFavorite",[t("ModelFavoriteText","收藏",11,width="hug")],
                      width=30,height=26,align="center",justify="center",radius=4),
                  box("ModelQuickSettings",[t("ModelQuickSettingsText","设置",11,width="hug")],
                      width=30,height=26,align="center",justify="center",radius=4,stroke="#BDBDBD")]
    return row("Model"+str(i),[
        box("Model"+str(i)+"Icon",[t("Model"+str(i)+"Mark",PROVIDERS[provider][0],12,width="hug")],
            width=20,height=20,align="center",justify="center"),
        box("Model"+str(i)+"Labels",[
            t("Model"+str(i)+"Name",model,14),
            t("Model"+str(i)+"Source",PROVIDERS[provider][1]+" · "+capability,11,muted=True),
        ],width="fill",gap=4),
        *controls,
        t("Model"+str(i)+"EffectiveAgent","ALVA · "+effort,11,muted=True,width=58),
    ],height=58,padding=[8,8],gap=6,radius=6,
        **({"fill":"#E6E6E6"} if i==0 else {"fill":"#F3F3F3","stroke":"#CBCBCB"} if i==2 else {}))


def model_options():
    return box("ModelOptionsPopover",[
        row("ModelOptionsHeader",[
            box("ModelOptionsIdentity",[t("ModelOptionsName","GPT",16),
                t("ModelOptionsSource","OpenAI · 主连接",11,muted=True)],width="fill",gap=4),
            action("PopoverFavorite","收藏",quiet=True),
        ],height=42,gap=4),
        box("ReasoningOptions",[
            t("ReasoningTitle","思考程度",12),
            row("ReasoningChoices",[
                box("Effort"+key,[t("Effort"+key+"Text",label,12,width="hug")],
                    width="fill",height=26,align="center",justify="center",radius=4,
                    **({"fill":"#E1E1E1","stroke":"#BDBDBD"} if key=="High" else {"fill":"#F6F6F6"}))
                for key,label in [("Default","默认"),("Low","低"),("Medium","中"),("High","高")]
            ],gap=4),
        ],width="fill",gap=6),
        box("CompatibleAgents",[
            t("CompatibleAgentsTitle","可用 Agent",12),
            row("CompatibleAgentChoices",[agent_item(a,a=="ALVA") for a in AGENTS],gap=4),
        ],width="fill",gap=6),
        row("ContextOption",[t("ContextLabel","Context",12),action("ContextValue","128K v")],height=32),
        row("VisionOption",[t("VisionLabel","视觉",12),action("VisionValue","开启 v")],height=32),
        action("ModelSettings","模型设置 >",width="fill",quiet=True),
        t("ModelOptionsScope","当前会话 · 下一轮生效",11,muted=True),
    ],width=272,height="hug",padding=12,gap=8,fill="#FFFFFF",stroke="#BDBDBD",radius=9)


def dialog():
    return dict(page=PAGE,module="选择器",canvas=CANVAS,platform="desktop",width=880,height=560,
                layout="row",padding=20,gap=8,align="start",fill="#EAEAEA",children=[
        box("ModelOptionsAnchor",[
            box("ModelOptionsOffset",width="fill",height=80),model_options(),
        ],width=272,height="fill",gap=0),
        box("AgentModelDialog",[
            row("DialogHeader",[
                box("ModelSearch",[t("ModelSearchPlaceholder","搜索模型…",14,muted=True)],
                    width="fill",height=34,padding=[6,8]),
                action("CloseDialog","关闭",quiet=True),
            ],height=52,padding=[8,12],stroke={"color":"#E4E4E4","width":[0,0,1,0]}),
            row("DialogBody",[
                box("ModelFilters",[
                    filter_button("FavoritesFilter","收藏"),
                    filter_button("AllModelsFilter","全部",selected=True),
                    box("FilterDivider",width="fill",height=1,fill="#D8D8D8"),
                    *[filter_button("Provider"+p,PROVIDERS[p][0]) for p in PROVIDERS],
                    box("FilterRailSpace",width="fill",height="fill"),
                ],width=48,height="fill",padding=[10,5],gap=6,fill="#F5F5F5"),
                box("ModelPane",[
                    box("ModelList",[model_item(i,*m) for i,m in enumerate(MODELS)],width="fill",height="fill",gap=4),
                ],width="fill",height="fill",padding=10,gap=0),
            ],height="fill",gap=0),
            row("DialogFooter",[
                action("ManageConnections","+  配置模型",quiet=True),
            ],height=44,padding=[0,12],stroke={"color":"#DDDDDD","width":[1,0,0,0]}),
        ],width=560,height=484,fill="#FFFFFF",stroke="#D0D0D0",radius=10),
    ])


def definitions():
    return [dialog()]


def connect():
    snapshot=cli("flow","list",PRODUCT,label="model-first-read-links")
    if snapshot.get("resultFile"):
        snapshot=json.loads(Path(snapshot["resultFile"]["path"]).read_text())
    existing=snapshot.get("text","")
    origin="选择器 / "+PAGE
    legacy=f"{origin} (desktop) / ModelSettings —click→ 框架 / 04 模型与连接"
    if legacy in existing:
        cli("flow","unlink",PRODUCT,"--from",origin+" / ModelSettings","--to","框架 / 04 模型与连接","--on","click",
            "--batch-id","alva-model-settings-replace-generic-target-v1",label="model-settings-replace-generic-target")
    links=[("框架 / 01 新任务","SelectAgentModel",origin),
           (origin,"CloseDialog","框架 / 01 新任务"),
           (origin,"ManageConnections","框架 / 04 模型与连接"),
           (origin,"ModelSettings","选择器 / 11 模型配置")]
    for i,(source,element,target) in enumerate(links):
        if f"{source} (desktop) / {element} —click→ {target}" in existing:
            continue
        identity=hashlib.sha256((source+" / "+element+" -> "+target).encode()).hexdigest()[:16]
        cli("flow","link",PRODUCT,"--from",source+" / "+element,"--to",target,"--on","click",
            "--batch-id",f"alva-model-first-picker-{identity}-v2",label="model-first-picker-link")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--links",action="store_true")
    parser.add_argument("--page",choices=[d["page"] for d in definitions()])
    parser.add_argument("--since")
    args=parser.parse_args()
    if args.write or args.links:
        if not args.since:parser.error("Writing requires --since REVIEWED_REVISION")
        changes=cli("changes",PRODUCT,"--since",args.since,label="picker-read-app-changes")
        if changes.get("changeCount",0):raise SystemExit("Inspect App changes before authoring.")
    if args.write:
        for i,d in enumerate(definitions()):
            if args.page is None or d["page"]==args.page:
                cli("page","draw",PRODUCT,intent=d,label="single-picker")
    elif args.links:
        connect()
    else:
        for i,d in enumerate(definitions()):
            (ROOT/"source"/"intents"/"single-picker.json").write_text(json.dumps(d,ensure_ascii=False,indent=2)+"\n")
        print("Prepared one Agent/model dialog wireframe.")
