# INPUT: Reviewed SmallPen package, shared workspace composer and the reference add-menu interaction
# OUTPUT: One add-menu wireframe anchored above the composer, plus representative open/close links
# POS: Design-only menu for draft attachments and optional modes; does not execute tasks or grant access.
import argparse
import json
from pathlib import Path

from author import PRODUCT, ROOT, cli
from workbench import CANVAS, action, box, composer, row, t

PAGE = "06 添加与模式"


def menu_item(name, mark, label, hint):
    return row(name, [
        box(name+"Icon", [t(name+"Mark", mark, 14, width="hug")],
            width=26, height=26, align="center", justify="center"),
        box(name+"Labels", [t(name+"Label", label, 14), t(name+"Hint", hint, 11, muted=True)],
            width="fill", gap=4),
        t(name+"More", ">", 12, muted=True, width="hug"),
    ], height=52, padding=[6,8], gap=10, radius=5)


def definition():
    return dict(page=PAGE, module="选择器", canvas=CANVAS, platform="desktop",
        width=800, height=580, layout="column", padding=32, gap=10, fill="#EAEAEA", children=[
            box("AddMenu", [
                row("AddMenuHeader", [t("AddMenuTitle", "添加", 12, muted=True),
                    action("CloseAddMenu", "关闭", quiet=True)], height=32),
                menu_item("AddFiles", "+", "添加文件、图片或视频…", "添加到当前消息"),
                menu_item("CreateGoal", "O", "新建目标", "填写目标后再创建"),
                menu_item("EnablePlan", "=", "计划模式", "先规划，确认后执行"),
                menu_item("EnableCollaboration", "&", "协作模式", "选择成员与分工"),
                box("AddMenuDivider", width="fill", height=1, fill="#D8D8D8"),
                menu_item("AddDirectory", "+", "添加目录…", "查看范围与权限"),
            ], width=400, height="hug", padding=12, gap=4, fill="#FFFFFF", stroke="#CFCFCF", radius=9),
            composer(),
        ])


def connect():
    snapshot=cli("flow", "list", PRODUCT, label="composer-menu-read-links")
    if snapshot.get("resultFile"):
        snapshot=json.loads(Path(snapshot["resultFile"]["path"]).read_text())
    existing=snapshot.get("text", "")
    origin="选择器 / "+PAGE
    links=[("框架 / 01 新任务", "Attach", origin),
           (origin, "CloseAddMenu", "框架 / 01 新任务")]
    for i,(source,element,target) in enumerate(links):
        if f"{source} (desktop) / {element} —click→ {target}" in existing:
            continue
        cli("flow", "link", PRODUCT, "--from", source+" / "+element, "--to", target, "--on", "click",
            "--batch-id", f"alva-composer-menu-link-{i}-v1", label="composer-menu-link")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--links", action="store_true")
    parser.add_argument("--since")
    args=parser.parse_args()
    if args.write or args.links:
        if not args.since:parser.error("Writing requires --since REVIEWED_REVISION")
        changes=cli("changes", PRODUCT, "--since", args.since, label="composer-menu-read-changes")
        if changes.get("changeCount",0):raise SystemExit("Inspect Web/App changes before authoring.")
    if args.write:
        cli("page", "draw", PRODUCT, intent=definition(), label="composer-menu")
    elif args.links:
        connect()
    else:
        (ROOT/"source/intents/composer-menu.json").write_text(json.dumps(definition(),ensure_ascii=False,indent=2)+"\n")
        print("Prepared one add-menu wireframe.")
