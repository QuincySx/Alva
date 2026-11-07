# INPUT: User companion/group brief, reviewed Cindy source and existing workspace/model/Skill contracts
# OUTPUT: Companion conversation, editable profile, group conversation and create-group dialog wireframes
# POS: Product-layer identity and collaboration design; reuses execution/plugin ports without extending the kernel.
import argparse
import hashlib
import json
from pathlib import Path

from author import PRODUCT, ROOT, cli
from workbench import CANVAS, action, box, row, sidebar, t
from provider_setup import field

PAGES=["12 伙伴与群聊", "13 伙伴设置", "14 伙伴群聊", "15 新建群聊"]


def roster(group=False):
    return box("CompanionRoster",[
        row("CompanionRosterHeader",[t("CompanionRosterTitle","伙伴",17),action("NewCompanion","+",quiet=True)]),
        box("CompanionSearch",[t("CompanionSearchText","搜索伙伴和群聊…",12,muted=True)],width="fill",padding=10,stroke="#D6D6D6",radius=6),
        *[box(key,[t(key+"Name",name,14),t(key+"Hint",hint,11,muted=True)],width="fill",padding=12,gap=6,radius=6,
            **({"fill":"#E7E7E7"} if active else {})) for key,name,hint,active in [
                ("ResearchCompanion","研究伙伴","资料整理 · 2 项 Skills",not group),
                ("ReviewCompanion","审查伙伴","代码审查 · 1 项 Skill",False),
                ("ProductGroup","产品讨论组","你 + 2 位伙伴",group),
        ]],
        action("NewGroup","+  新建群聊",width="fill"),
        box("RosterSpace",width="fill",height="fill"),
        t("RosterScopeHint","伙伴身份与模型、Agent 分开管理",11,muted=True),
    ],width=220,height="fill",padding=16,gap=12,fill="#FAFAFA",stroke={"color":"#DDDDDD","width":[0,1,0,0]})


def message(key,author,text,detail=None):
    return box(key,[t(key+"Author",author,12,muted=True),t(key+"Text",text,15),
        *([t(key+"Detail",detail,11,muted=True)] if detail else [])],width="fill",padding=[16,0],gap=9)


def chat_input(group=False):
    return box("CompanionComposer",[
        t("CompanionPrompt","@ 选择伙伴，或发一条消息…" if group else "和研究伙伴说说你要做的事…",14,muted=True),
        box("CompanionComposerSpace",width="fill",height="fill"),
        row("CompanionComposerControls",[action("CompanionAttach","+",quiet=True),action("CompanionPermission","每次询问 v",quiet=True),
            box("CompanionControlSpace",width="fill",height=1),
            action("GroupReplyPolicy","仅 @ 回应 v",quiet=True) if group else action("CompanionSelectModel","GPT · ALVA v",quiet=True),
            action("CompanionSend","发送",primary=True)]),
    ],width="fill",height=132,padding=16,gap=12,stroke="#CFCFCF",fill="#FFFFFF",radius=9)


def conversation(group=False):
    msgs=[message("GroupUser","你","@研究伙伴 @审查伙伴 一起看看模型添加流程，先说各自的建议。"),
        message("GroupResearch","研究伙伴 · GPT / ALVA","我先核对连接和模型目录。服务没有模型列表时，也应该允许手动添加准确的模型 ID。","使用 Skill：资料研究 · 本轮第 1 位"),
        message("GroupReview","审查伙伴 · Sonnet / Claude","我来检查错误与保存边界。连接失败时保留草稿，凭据不会写进群消息。","使用 Skill：代码审查 · 本轮第 2 位"),
        t("GroupRoundEnded","本轮结束 · 等你继续",12,muted=True)] if group else [
        message("CompanionUser","你","帮我整理模型供应商的连接方案，按我们约定的格式。"),
        message("CompanionReply","研究伙伴","我会按你的资料整理方式，先列来源与依据，再给出需要你判断的差异。","已挂载：资料研究、文档整理 · 示例会话"),
        box("CompanionSkillReceipt",[t("CompanionSkillReceiptTitle","这次使用的能力",13),
            t("CompanionSkillReceiptText","资料研究 v1 · 来自共享 Skills 库\n文档整理 v2 · 仅此伙伴",12,muted=True)],width="fill",padding=16,gap=8,fill="#F5F5F5",radius=6),
    ]
    return box("CompanionConversation",[
        *msgs,box("CompanionMessagesSpace",width="fill",height="fill"),chat_input(group),
        t("CompanionScopeHint","群聊拥有独立记录；不会自动带入伙伴私聊与私人记忆。" if group else "长期对话保留记录；上下文按需压缩，伙伴身份和 Skills 保持。",11,muted=True),
    ],width="fill",height="fill",padding=[24,32],gap=14)


def chat_page(group=False):
    header=row("CompanionHeader",[
        box("CompanionTitleBlock",[t("CompanionTitle","产品讨论组" if group else "研究伙伴",18),
            t("CompanionSubtitle","你 · 研究伙伴 · 审查伙伴" if group else "资料研究与整理 · 2 项 Skills",11,muted=True)],width="fill",gap=6),
        action("CompanionAddToGroup","添加到群",quiet=True) if not group else action("GroupContinue","继续讨论",quiet=True),
        action("CompanionSettings","群设置" if group else "伙伴设置"),
    ],height=74,padding=[12,24],stroke={"color":"#DDDDDD","width":[0,0,1,0]})
    main=[header]
    if group:
        main.append(row("GroupPolicySummary",[t("GroupPolicyText","仅 @ 的伙伴回答 · 按成员顺序 · 每位 1 次 · 本轮最多 3 次回复",12,muted=True)],height=40,padding=[0,24],fill="#F5F5F5"))
    main.append(conversation(group))
    return dict(page=PAGES[2] if group else PAGES[0],module="伙伴",canvas=CANVAS,platform="desktop",width=1440,height=960,
        layout="row",fill="#FFFFFF",children=[sidebar(),roster(group),box("CompanionMain",main,width="fill",height="fill",gap=0)])


def profile():
    return dict(page=PAGES[1],module="伙伴",canvas=CANVAS,platform="desktop",width=940,height=880,layout="column",padding=24,fill="#ECECEC",children=[
        box("CompanionProfileDialog",[
            row("ProfileHeader",[t("ProfileTitle","伙伴资料 · 研究伙伴",22),action("ProfileClose","x",quiet=True)]),
            row("ProfileSections",[action("ProfileIdentityTab","身份与能力"),action("ProfileMemoryTab","记忆",quiet=True),action("ProfileHistoryTab","历史",quiet=True)]),
            row("ProfileIdentity",[field("ProfileName","名字","研究伙伴"),field("ProfileModel","模型与 Agent","GPT · ALVA  v")]),
            field("ProfileRole","职责与做事方式","核对资料来源，整理依据；不确定时说明，不把推测写成事实。"),
            row("ProfileSkillsHeading",[t("ProfileSkillsTitle","这个伙伴的 Skills",16),action("ProfileAddSkill","+  添加 Skill")]),
            box("ProfileSkills",[
                row("ResearchSkill",[box("ResearchSkillLabels",[t("ResearchSkillName","资料研究",14),t("ResearchSkillOrigin","共享 Skills 库 · 固定 v1",11,muted=True)],width="fill",gap=6),action("ResearchSkillAction","移除",quiet=True)]),
                row("DocumentSkill",[box("DocumentSkillLabels",[t("DocumentSkillName","文档整理",14),t("DocumentSkillOrigin","伙伴自有 · v2 · 可编辑",11,muted=True)],width="fill",gap=6),action("DocumentSkillAction","编辑",quiet=True)]),
            ],width="fill",padding=16,gap=18,fill="#F6F6F6",stroke="#DDDDDD",radius=6),
            t("ProfileSkillHint","添加引用不复制共享源；需要改写时另存为伙伴自有 Skill。",12,muted=True),
            row("ProfileAccess",[field("ProfileTools","工具与 MCP","已选工具集  v"),field("ProfilePermissions","执行权限","每次询问  v")]),
            row("ProfileMemory",[field("ProfileMemoryMode","长期记忆","启用 · 可查看和删除  v"),field("ProfileLearning","新 Skill 建议","保存前确认  v")]),
            t("ProfileSafetyHint","Skill 说明不授予执行权限。群聊按参与范围加载能力与记忆。",12,muted=True),
            box("ProfileSpace",width="fill",height="fill"),
            row("ProfileFooter",[action("ProfileCancel","取消",quiet=True),t("ProfileSaveScope","保存后在下一轮生效",11,muted=True),action("ProfileSave","保存伙伴",primary=True)]),
        ],width="fill",height="fill",padding=24,gap=18,fill="#FFFFFF",stroke="#CBCBCB",radius=10)
    ])


def group_settings():
    return dict(page=PAGES[3],module="伙伴",canvas=CANVAS,platform="desktop",width=760,height=760,layout="column",padding=24,fill="#ECECEC",children=[
        box("CreateGroupDialog",[
            row("CreateGroupHeader",[t("CreateGroupTitle","新建群聊",22),action("CreateGroupClose","x",quiet=True)]),
            field("GroupName","群名称","产品讨论组"),
            t("GroupMembersTitle","选择伙伴",15),
            *[row(key,[t(key+"Check","[x]",14,width=28),box(key+"Labels",[t(key+"Name",name,14),t(key+"Skills",skills,11,muted=True)],width="fill",gap=6)],height=60,padding=12,fill="#F4F4F4",radius=6)
              for key,name,skills in [("ChooseResearch","研究伙伴","资料研究 · 文档整理"),("ChooseReviewer","审查伙伴","代码审查")]],
            row("GroupReplyRules",[field("GroupReplyMode","谁回应","仅 @ 的伙伴  v"),field("GroupOrder","发言顺序","按成员顺序  v")]),
            row("GroupBudgetRules",[field("GroupRoundBudget","每轮回复上限","3 次"),field("GroupWorkspace","共享工作区","未选择  v")]),
            field("GroupPrivacy","向伙伴共享","本群消息与已授权文件 · 不带入私聊与私人记忆"),
            t("GroupPermissionsHint","每位伙伴保留自己的 Skills 和权限；成员消息不会自动开启下一轮。",12,muted=True),
            box("CreateGroupSpace",width="fill",height="fill"),
            row("CreateGroupFooter",[action("CreateGroupCancel","取消",quiet=True),box("CreateGroupFooterSpace",width="fill",height=1),action("CreateGroupConfirm","创建群聊",primary=True)]),
        ],width="fill",height="fill",padding=24,gap=18,fill="#FFFFFF",stroke="#CBCBCB",radius=10)
    ])


def definitions():
    return [chat_page(),profile(),chat_page(True),group_settings()]


def connect():
    snapshot=cli("flow","list",PRODUCT,label="companions-read-links")
    if snapshot.get("resultFile"):snapshot=json.loads(Path(snapshot["resultFile"]["path"]).read_text())
    existing=snapshot.get("text","")
    p=["伙伴 / "+v for v in PAGES]
    links=[*[("框架 / "+page,"Companions",p[0]) for page in ("01 新任务","02 对话进行中","03 成果展开")],
        (p[0],"CompanionSettings",p[1]),(p[0],"NewCompanion",p[1]),(p[0],"NewGroup",p[3]),
        (p[0],"CompanionAddToGroup",p[3]),(p[0],"ProductGroup",p[2]),
        (p[1],"ProfileModelInput","选择器 / 05 Agent 与模型"),
        *[(p[1],control,p[0]) for control in ("ProfileClose","ProfileCancel","ProfileSave")],
        (p[2],"ResearchCompanion",p[0]),(p[2],"NewGroup",p[3]),
        (p[3],"CreateGroupConfirm",p[2]),(p[3],"CreateGroupCancel",p[0]),(p[3],"CreateGroupClose",p[0]),
        *[(page,element,target) for page in (p[0],p[2]) for element,target in [
            ("NewTask","框架 / 01 新任务"),("CurrentThread","框架 / 02 对话进行中"),
            ("OpenSettings","框架 / 04 模型与连接"),("Companions",p[0])]]]
    for i,(source,element,target) in enumerate(links):
        if f"{source} (desktop) / {element} —click→ {target}" in existing:continue
        identity=hashlib.sha256((source+" / "+element+" -> "+target).encode()).hexdigest()[:16]
        cli("flow","link",PRODUCT,"--from",source+" / "+element,"--to",target,"--on","click",
            "--batch-id",f"alva-companion-link-{identity}-v2",label="companions-link")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--links",action="store_true")
    parser.add_argument("--since")
    args=parser.parse_args()
    if args.write or args.links:
        if not args.since:parser.error("Writing requires --since REVIEWED_REVISION")
        changes=cli("changes",PRODUCT,"--since",args.since,label="companions-read-app-changes")
        if changes.get("changeCount",0):raise SystemExit("Inspect App changes before authoring.")
    if args.write:
        for d in definitions():cli("page","draw",PRODUCT,intent=d,label="companions-"+d["page"][:2])
    elif args.links:connect()
    else:
        for d in definitions():(ROOT/"source"/"intents"/("companions-"+d["page"][:2]+".json")).write_text(json.dumps(d,ensure_ascii=False,indent=2)+"\n")
        print("Prepared companion/group definitions; no package changes.")
