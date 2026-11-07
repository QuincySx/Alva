# INPUT: Saved SmallPen package and reviewed Codex, Claude and Cursor workspace patterns
# OUTPUT: Four workspace wireframes with shared composer and a tabbed right-side workspace
# POS: Framework-stage UI authoring; preserves existing page identities and reads App edits first.
import json
import argparse

from author import PRODUCT, ROOT, cli

CANVAS = "00 工作台线稿"
PAGES = ["01 新任务", "02 对话进行中", "03 成果展开", "04 模型与连接"]
OLD = ["01 应用全景", "02 主窗口分区", "03 页面与设置", "04 隐藏与弹出"]


def t(name, value, size=14, muted=False, **extra):
    return dict(name=name, text=value, fontFamily="{font.family}", fontSize=size,
                fontWeight=400, color="#666666" if muted else "#252525",
                width="fill") | extra


def box(name, children=(), layout="column", **extra):
    return dict(name=name, children=list(children), layout=layout, **extra)


def row(name, children, **extra):
    return box(name, children, "row", **(dict(width="fill", align="center", gap=12) | extra))


def action(name, label, width="hug", primary=False, quiet=False):
    return box(name, [t(name+"Text", label, 13, width="hug", **({"color": "#FFFFFF"} if primary else {}))],
               layout="row", width=width, padding=[8,12], radius=6, align="center", justify="center",
               **({"fill": "#252525"} if primary else {} if quiet else
                  {"fill": "#FFFFFF", "stroke": "#D7D7D7"}))


def nav_item(name, label, selected=False, indent=False):
    return box(name, [t(name+"Text", label)], width="fill", padding=[9,16 if indent else 10],
               radius=6, **({"fill": "#E5E5E5"} if selected else {}))


def sidebar(active=False):
    return box("Sidebar", [
        row("Brand", [t("BrandName", "Alva", 19), action("CollapseSidebar", "收起", quiet=True)], padding=[6,4]),
        action("NewTask", "+  新任务", width="fill"),
        nav_item("Search", "搜索"),
        nav_item("Automations", "自动化"),
        nav_item("Customize", "自定义"),
        nav_item("Companions", "伙伴与群聊"),
        box("ProjectGroup", [
            row("ProjectsLabel", [t("ProjectsLabelText", "项目", 12, muted=True), action("AddProject", "+", quiet=True)]),
            nav_item("CurrentProject", "v  alva-agent"),
            nav_item("CurrentThread", "整理桌面工作台", selected=active, indent=True),
            nav_item("SecondThread", "复查插件加载", indent=True),
            nav_item("AnotherProject", ">  我的资料"),
        ], width="fill", padding=[12,0], gap=4),
        box("RecentGroup", [t("RecentLabel", "最近", 12, muted=True),
            nav_item("RecentResearch", "整理本周资料"), nav_item("RecentModels", "测试模型表现")],
            width="fill", gap=4, padding=[8,0]),
        box("SidebarSpace", width="fill", height="fill"),
        row("Account", [t("AccountName", "Smallraw", 13), action("OpenSettings", "设置", quiet=True)], padding=4),
    ], width=244, height="fill", padding=14, gap=6, fill="#F2F2F2")


def composer(running=False):
    return box("Composer", [
        t("Prompt", "发送补充说明，或调整下一步…" if running else "说明你要完成的事，@ 引用文件或资料", 15, muted=True),
        box("ComposerSpace", width="fill", height="fill"),
        row("ComposerControls", [
            action("Attach", "+", quiet=True),
            action("Permissions", "每次询问 v", quiet=True),
            box("ComposerControlSpace", width="fill", height=1),
            action("SelectAgentModel", "ALVA · 默认模型 v", quiet=True),
            action("Stop" if running else "Send", "停止" if running else "发送", primary=True),
        ])
    ], width="fill", height=150, padding=16, gap=14, fill="#FFFFFF", stroke="#CFCFCF", radius=10)


def header(title, details=False):
    return row("ThreadHeader", [
        t("ThreadTitle", title, 14),
        action("ProjectInfo", "alva-agent v", quiet=True),
        action("ShowArtifact", "面板", quiet=True),
        action("MoreActions", "…", quiet=True),
    ], height=58, padding=[0,24], fill="#FFFFFF")


def shell(index, main):
    return dict(page=PAGES[index], module="框架", canvas=CANVAS, platform="desktop",
                width=1440, height=960, layout="row", fill="#FFFFFF",
                children=[sidebar(index in (1,2)), main])


def attachment(name="ArtifactCard"):
    return box(name, [row(name+"Contents", [
        box(name+"FileMark", [t(name+"FileType", "MD", 12)], width=44, height=48,
            padding=10, fill="#F0F0F0", radius=4),
        box(name+"Info", [t(name+"Title", "工作台方案.md", 15),
            t(name+"Status", "文档 · 草稿更新中", 12, muted=True)], width="fill", gap=5),
        t(name+"Open", ">", 18, width=16),
    ])], width="fill", padding=14, radius=8, fill="#FFFFFF", stroke="#D5D5D5")


def user_message(narrow=False):
    return row("UserMessageRow", [box("UserMessage", [
        t("UserPrompt", "把这个项目的桌面界面整理成一套清楚、好用的工作台。", 15)
    ], width=480 if narrow else 560, padding=[14,18], fill="#F0F0F0", radius=10)], justify="end")


def conversation(narrow=False):
    return box("Conversation", [
        user_message(narrow),
        t("AssistantIntro", "我会先整理会话、任务和成果的关系，再检查设置与扩展的入口。", 15),
        box("ToolGroup", [
            t("ToolGroupHeading", "v  已读取 3 份资料", 13, muted=True),
            t("ToolSources", "README.md   ·   docs/ARCHITECTURE.md   ·   docs/USAGE.md", 12, muted=True),
        ], width="fill", padding=[12,14], gap=8, fill="#F7F7F7", radius=6),
        t("AssistantReason", "工作区以当前对话为中心。项目与历史放在左边，成果在需要时于右侧展开。", 15),
        attachment(),
        box("Progress", [
            t("ProgressTitle", "正在整理交互流程…", 14),
            t("ProgressDetail", "布局与入口已整理，继续补充等待、审批和恢复状态。", 13, muted=True),
        ], width="fill", gap=8),
        box("TranscriptSpace", width="fill", height="fill"),
        composer(True),
        t("ComposerHint", "ALVA · 当前项目 · 运行中", 12, muted=True),
    ], width="fill", height="fill", gap=24)


PANEL_TYPES = [
    ("Explorer", "文件浏览器", "浏览与打开项目文件", "F"),
    ("Review", "审查", "查看文件变更", "+"),
    ("Subagent", "子任务", "Subagent · 进度与结果", "A"),
    ("Background", "后台任务", "查看任务与结果", "="),
    ("Browser", "浏览器", "打开网页", "W"),
    ("Terminal", "终端", "当前项目的终端", ">"),
    ("Inspector", "运行详情", "查看模型与工具记录", "I"),
]


def panel_action(name, label):
    return box(name, [t(name+"Text",label,11,width="hug")],
        layout="row",width="hug",height=28,padding=[0,6],align="center",justify="center",radius=4)


def panel_tab(key, label, selected=False):
    return row("PanelTab"+key,[t("PanelTab"+key+"Text",label,11,width="hug"),
        panel_action("Close"+key+"Tab","x")],width="hug",height=30,padding=[0,5],gap=3,radius=5,
        **({"fill":"#FFFFFF","stroke":"#C9C9C9"} if selected else {}))


def panel_menu(opened=("Explorer","Subagent","Background")):
    return box("AddPanelMenu",[
        t("AddPanelMenuTitle","添加标签",12,muted=True),
        *[row("AddPanel"+key,[
            t("AddPanel"+key+"Mark",mark,12,muted=True,width=18),
            t("AddPanel"+key+"Label",label,13),
            t("AddPanel"+key+"State","已打开" if key in opened else "",11,muted=True,width=40),
        ],height=36,gap=8) for key,label,_,mark in PANEL_TYPES],
    ],width=248,height="hug",padding=14,gap=4,fill="#FFFFFF",stroke="#CBCBCB",radius=8)


def file_panel_content():
    return box("FilePanelContent",[
        row("ArtifactHeader",[t("ArtifactTitle","工作台方案.md",14),
            panel_action("OpenArtifactFile","打开文件")],height=44,padding=[6,18]),
        row("ArtifactTabs",[action("PreviewTab","预览",quiet=True),action("SourceTab","来源",quiet=True)],padding=[0,18]),
        box("Document",[
            t("DocumentTitle","桌面工作台方案",25),
            t("DocumentMeta","由 ALVA 生成 · 当前会话",12,muted=True),
            t("DocumentIntro","把任务发起、执行过程和结果查看放在同一个工作区。",15),
            t("DocumentHeading1","1. 项目与会话",19),
            t("DocumentSection1","左侧保存项目和会话的层级。新建、搜索和切换始终就近可用。",15),
            t("DocumentHeading2","2. 当前任务",19),
            t("DocumentSection2","对话保留完整上下文。工具活动在消息中折叠，输入框支持补充说明和停止。",15),
            t("DocumentHeading3","3. 成果与来源",19),
            t("DocumentSection3","点击成果后在右侧文件标签中展开。其他面板与对话可以继续保留。",15),
            t("DocumentHeading4","4. 设置与扩展",19),
            t("DocumentSection4","连接、插件、规则和存储在设置中集中管理。模型与权限保留当前会话的快捷入口。",15),
            box("DocumentSpace",width="fill",height="fill"),
        ],width="fill",height="fill",padding=[24,30],gap=20),
        row("ArtifactFooter",[t("SavedLocation","保存于当前项目",12,muted=True)],padding=18),
    ],width="fill",height="fill",gap=0)


def right_workspace(state="file", width=530, prefix=""):
    tabs=[] if state=="empty" else [panel_tab("Explorer","文件",state=="file"),
        panel_tab("Subagent","子任务"),panel_tab("Background","后台任务",state=="jobs")]
    if state=="file":
        body=file_panel_content()
    elif state=="empty":
        body=box("RightWorkspaceStart",[
            t("RightWorkspaceLabel","右侧工作区",12,muted=True),
            t("RightWorkspaceTitle","开始一个面板",22),
            t("RightWorkspaceDescription","打开文件、审查或任务，与左侧对话并行工作。",13,muted=True),
            box("RightWorkspaceChoices",[
                row("OpenPanel"+key,[t("OpenPanel"+key+"Mark",mark,14,muted=True,width=24),
                    box("OpenPanel"+key+"Labels",[t("OpenPanel"+key+"Title",label,14),
                        t("OpenPanel"+key+"Description",hint,11,muted=True)],width="fill",gap=4),
                    t("OpenPanel"+key+"More",">",14,muted=True,width=12),
                ],height=52,gap=10,stroke={"color":"#E5E5E5","width":[0,0,1,0]})
                for key,label,hint,mark in PANEL_TYPES
            ],width="fill",gap=0),
            box("RightWorkspaceStartSpace",width="fill",height="fill"),
            t("RightWorkspaceStartHint","也可以通过 + 添加面板",11,muted=True),
        ],width="fill",height="fill",padding=[28,24],gap=12)
    else:
        body=box("BackgroundPanelContent",[
            t("BackgroundEmptyMark","=",28,muted=True,width="hug"),
            t("BackgroundEmptyTitle","本任务暂无后台作业",16,width="hug"),
            t("BackgroundEmptyHint","后台任务启动后，进度和结果会显示在这里",12,muted=True,width="hug"),
        ],width="fill",height="fill",align="center",justify="center",gap=14,padding=24)
    result=box("ArtifactPanel",[
        row("PanelTabBar",[
            *tabs,panel_action("AddPanel","+"),
            box("PanelTabBarSpace",width="fill",height=1),
            panel_action("DetachArtifact","弹出"),panel_action("FocusRightWorkspace","专注"),
            panel_action("CloseArtifact","收起"),
        ],height=44,padding=[6,8],gap=4,fill="#F3F3F3",stroke={"color":"#DDDDDD","width":[0,0,1,0]}),
        body,
    ],width=width,height="fill",gap=0,fill="#FAFAFA",stroke="#DDDDDD")
    if prefix:
        def rename(node):
            node["name"]=prefix+node["name"]
            for child in node.get("children",[]):rename(child)
        rename(result)
    return result


def screens():
    new = shell(0, box("Main", [header("新任务"),
        box("WelcomeArea", [
            box("WelcomeContent", [
                t("WelcomeTitle", "开始一项新任务", 28),
                t("WelcomeProject", "在 alva-agent 中工作", 14, muted=True),
                composer(),
                row("Suggestions", [action("SuggestProject", "整理项目结构", width="fill"),
                    action("SuggestResearch", "整理项目资料", width="fill"),
                    action("SuggestFile", "检查一份文件", width="fill")]),
            ], width=720, gap=20),
        ], width="fill", height="fill", align="center", justify="center", padding=[0,24,120,24]),
    ], width="fill", height="fill", fill="#FFFFFF"))

    chat = shell(1, box("Main", [header("整理桌面工作台", True),
        box("ConversationArea", [box("ConversationColumn", [conversation()], width=760, height="fill")],
            width="fill", height="fill", align="center", padding=[32,32,22,32]),
    ], width="fill", height="fill", fill="#FFFFFF"))

    artifact = right_workspace()
    detail = shell(2, box("Main", [header("整理桌面工作台", True),
        row("ChatAndArtifact", [box("ConversationArea", [conversation(True)], width="fill", height="fill", padding=[28,26,20,26]), artifact],
            height="fill", gap=0),
    ], width="fill", height="fill", fill="#FFFFFF"))

    settings_nav=box("SettingsSidebar", [
        action("BackToWorkspace", "<  返回工作台", width="fill", quiet=True),
        box("SettingsHeading", [t("SettingsTitle", "设置", 23)], width="fill", padding=[8,10,18,10]),
        *[nav_item("SettingsCategory"+str(i), title, selected=i==1) for i,title in enumerate([
            "通用", "模型与连接", "插件与工具", "规则与 Skills", "数据与存储", "权限与安全", "Gateway", "开发者"
        ])],
        box("SettingsNavSpace", width="fill", height="fill"),
        action("ActiveTask", "1 个任务运行中 · 查看", width="fill"),
        t("SettingsOwner", "Smallraw", 13, muted=True),
    ], width=244, height="fill", padding=[24,16], gap=5, fill="#F2F2F2")

    from provider_setup import settings_main
    settings=dict(page=PAGES[3],module="框架",canvas=CANVAS,platform="desktop",width=1440,height=960,layout="row",fill="#FFFFFF",children=[
        settings_nav,
        settings_main(),
    ])
    return [new,chat,detail,settings]


def write(only=None, setup=False):
    if setup:
        for old,new in zip(OLD,PAGES):
            cli("page","rename",PRODUCT,"--page","框架 / "+old,"--to",new,
                "--batch-id","alva-workbench-rename-"+new[:2]+"-v1",label="workbench-rename")
        cli("canvas","rename",PRODUCT,"--canvas","00 框架规划（先看）","--to",CANVAS,
            "--batch-id","alva-workbench-canvas-v1",label="workbench-canvas")
        cli("canvas","rename",PRODUCT,"--canvas","Pages","--to","旧视觉草稿（暂不评审）",
            "--batch-id","alva-workbench-old-canvas-v1",label="workbench-old-canvas")
    for i,d in enumerate(screens()):
        if only is None or d["page"]==only:
            cli("page","draw",PRODUCT,intent=d,label="workbench-"+str(i))
    if not setup:
        return
    links=[]
    for page in PAGES[:3]:
        links += [(page,"NewTask",PAGES[0]),(page,"CurrentThread",PAGES[1]),(page,"OpenSettings",PAGES[3])]
    links += [(PAGES[0],"Send",PAGES[1]),(PAGES[1],"ArtifactCard",PAGES[2]),
              (PAGES[1],"ShowArtifact",PAGES[2]),(PAGES[2],"CloseArtifact",PAGES[1]),
              (PAGES[3],"BackToWorkspace",PAGES[1]),(PAGES[3],"ActiveTask",PAGES[1])]
    for i,(origin,element,target) in enumerate(links):
        cli("flow","link",PRODUCT,"--from",f"框架 / {origin} / {element}","--to","框架 / "+target,
            "--batch-id",f"alva-workbench-link-{i}-v1",label="workbench-link")
    # The renamed first board inherits the existing prototype start.


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--setup",action="store_true",help="Initial transition only; never replay after App edits")
    parser.add_argument("--page",choices=PAGES)
    parser.add_argument("--since",help="Revision whose design changes have already been reviewed")
    args=parser.parse_args()
    if args.write:
        if not args.since:
            parser.error("--write requires --since REVIEWED_REVISION")
        changes=cli("changes",PRODUCT,"--since",args.since,label="workbench-read-app-changes")
        if changes.get("changeCount",0):
            raise SystemExit("Design changed since the reviewed revision; inspect changes before authoring.")
        write(args.page,args.setup)
    else:
        for i,d in enumerate(screens()):
            (ROOT/"source"/"intents"/("workbench-"+str(i)+".json")).write_text(json.dumps(d,ensure_ascii=False,indent=2)+"\n")
        print("Prepared four task-centered workspace intents.")
