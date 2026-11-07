# INPUT: Shared workbench panel shell and reviewed right-side workspace reference screenshots
# OUTPUT: One component board with empty, tabbed-background and add-panel-menu examples
# POS: UI design only; closing or moving a panel does not own task execution or permissions.
import argparse
import json
from pathlib import Path

from author import PRODUCT, ROOT, cli
from workbench import CANVAS, box, panel_menu, right_workspace

PAGE = "07 右侧工作区"


def definition():
    return dict(page=PAGE,module="选择器",canvas=CANVAS,platform="desktop",
        width=1336,height=760,layout="row",padding=24,gap=20,align="start",fill="#EAEAEA",children=[
            right_workspace("empty",width=500,prefix="Empty"),
            right_workspace("jobs",width=500,prefix="Jobs"),
            box("AddPanelMenuAnchor",[
                box("AddPanelMenuOffset",width="fill",height=44),panel_menu(),
            ],width=248,height="fill",gap=0),
        ])


def connect():
    snapshot=cli("flow","list",PRODUCT,label="right-workspace-read-links")
    if snapshot.get("resultFile"):
        snapshot=json.loads(Path(snapshot["resultFile"]["path"]).read_text())
    existing=snapshot.get("text","")
    origin="选择器 / "+PAGE
    links=[("框架 / 03 成果展开","AddPanel",origin),
           (origin,"JobsCloseArtifact","框架 / 02 对话进行中"),
           (origin,"EmptyCloseArtifact","框架 / 02 对话进行中"),
           (origin,"EmptyOpenPanelExplorer","框架 / 03 成果展开"),
           (origin,"AddPanelExplorer","框架 / 03 成果展开"),
           ("框架 / 03 成果展开","CloseArtifact","框架 / 02 对话进行中")]
    for i,(source,element,target) in enumerate(links):
        if f"{source} (desktop) / {element} —click→ {target}" in existing:continue
        cli("flow","link",PRODUCT,"--from",source+" / "+element,"--to",target,"--on","click",
            "--batch-id",f"alva-right-workspace-link-{i}-v1",label="right-workspace-link")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--links",action="store_true")
    parser.add_argument("--since")
    args=parser.parse_args()
    if args.write or args.links:
        if not args.since:parser.error("Writing requires --since REVIEWED_REVISION")
        changes=cli("changes",PRODUCT,"--since",args.since,label="right-workspace-read-changes")
        if changes.get("changeCount",0):raise SystemExit("Inspect Web/App changes before authoring.")
    if args.write:
        cli("page","draw",PRODUCT,intent=definition(),label="right-workspace")
    elif args.links:
        connect()
    else:
        (ROOT/"source/intents/right-workspace.json").write_text(json.dumps(definition(),ensure_ascii=False,indent=2)+"\n")
        print("Prepared right workspace component states.")
