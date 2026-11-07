# INPUT: Installed SmallPen CLI 0.1.0-alpha.18 and explicit design intent files
# OUTPUT: Token-bound Foundation, shared components, Product pages and prototype links
# POS: Design authoring helper; uses public CLI operations and does not modify application code.
from pathlib import Path
import hashlib
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FOUNDATION = ROOT / "alva-foundation.smallpen"
PRODUCT = ROOT / "alva.smallpen"
INTENTS = ROOT / "source" / "intents"
LOG = ROOT / "source" / "authoring-log.jsonl"


def cli(*args, intent=None, label="write"):
    args = [str(x) for x in args]
    if intent is not None:
        INTENTS.mkdir(exist_ok=True, parents=True)
        data = json.dumps(intent, ensure_ascii=False, indent=2) + "\n"
        slug = "".join(c if c.isalnum() or c in "-_" else "-" for c in label)
        path = INTENTS / (slug + ".json")
        path.write_text(data)
        digest = hashlib.sha256(data.encode()).hexdigest()[:16]
        args += ["--intent", str(path), "--batch-id", f"alva-ds1-{slug}-{digest}"]
    p = subprocess.run(["smallpen", *args, "--json"], text=True, capture_output=True)
    try:
        result = json.loads(p.stdout)
    except ValueError:
        raise RuntimeError(p.stdout + p.stderr)
    with LOG.open("a") as f:
        f.write(
            json.dumps(
                {"argv": args, "exit": p.returncode, "result": result},
                ensure_ascii=False,
            )
            + "\n"
        )
    if p.returncode:
        raise RuntimeError(json.dumps(result, ensure_ascii=False))
    print(label, result.get("revision", result.get("status", "ok")), flush=True)
    return result


def token(name, kind, value, dark=None, values=None):
    r = {"name": name, "type": kind, "value": value}
    overrides = dict(values or {})
    if dark is not None:
        overrides["Appearance/Dark"] = dark
    if overrides:
        r["values"] = overrides
    return r


def tokens():
    for package in [FOUNDATION]:
        cli(
            "token",
            "theme",
            "add",
            package,
            "--theme",
            "Appearance/Light",
            "--theme",
            "Appearance/Dark",
            "--theme",
            "Density/Comfortable",
            "--theme",
            "Density/Compact",
            "--theme",
            "Viewport/Desktop",
            "--theme",
            "Viewport/Narrow",
            label="themes",
        )
    items = []
    colors = {
        "app": ("#F7F8FB", "#10151D"),
        "sidebar": ("#F0F3F8", "#151C27"),
        "surface": ("#FFFFFF", "#1B2432"),
        "subtle": ("#F4F6FA", "#222E3F"),
        "text": ("#182230", "#EDF2F9"),
        "muted": ("#536276", "#ADBBD0"),
        "line": ("#DDE4EE", "#39475B"),
        "primary": ("#285EEA", "#94B5FF"),
        "onPrimary": ("#FFFFFF", "#0F1D34"),
        "selected": ("#E8EFFF", "#253B60"),
        "selectedText": ("#214EBB", "#BED1FF"),
        "success": ("#166C46", "#8DE0B1"),
        "successBg": ("#E8F5ED", "#183B2B"),
        "warning": ("#8C4E0A", "#FFD38D"),
        "warningBg": ("#FFF2D8", "#49351B"),
        "danger": ("#AA2929", "#FFB5B5"),
        "dangerBg": ("#FDECEC", "#48282D"),
        "overlay": ("#E5EAF2", "#0B111A"),
    }
    for name, (light, dark) in colors.items():
        items += [
            token("palette.light." + name, "color", light),
            token("palette.dark." + name, "color", dark),
            token(
                "color." + name,
                "color",
                "{palette.light." + name + "}",
                "{palette.dark." + name + "}",
            ),
        ]
    items += [token("font.family", "font-family", "Alva Prototype Sans SC")]
    items += [
        token("label.appearance", "string", "浅色", dark="深色"),
        token("label.density", "string", "舒适", values={"Density/Compact": "紧凑"}),
    ]
    for name, value in [
        ("caption", 12),
        ("body", 14),
        ("label", 13),
        ("heading", 20),
        ("display", 28),
    ]:
        items.append(token("font.size." + name, "font-size", value))
    items += [
        token("font.weight.regular", "font-weight", 400),
        token("font.weight.strong", "font-weight", 600),
    ]
    for name, value in [
        ("xs", 4),
        ("sm", 8),
        ("md", 12),
        ("lg", 16),
        ("xl", 24),
        ("xxl", 32),
    ]:
        items.append(token("space." + name, "spacing", value))
    for name, value in [("sm", 6), ("md", 10), ("lg", 14), ("pill", 18)]:
        items.append(token("radius." + name, "border-radius", value))
    items += [
        token("border.thin", "stroke-width", 1),
        token("size.control", "sizing", 36, values={"Density/Compact": 32}),
        token("size.row", "sizing", 44, values={"Density/Compact": 36}),
        token("size.icon", "sizing", 20),
        token("size.sidebar", "sizing", 248, values={"Viewport/Narrow": 72}),
        token("size.windowBar", "sizing", 36),
        token("size.pageBar", "sizing", 64),
        token("size.inspector", "sizing", 344),
        token("view.navLabels", "boolean", True, values={"Viewport/Narrow": False}),
        token("view.navDetails", "boolean", True, values={"Viewport/Narrow": False}),
        token("view.navCompact", "boolean", False, values={"Viewport/Narrow": True}),
    ]
    cli("token", "set", FOUNDATION, intent={"tokens": items}, label="foundation-tokens")
    cli(
        "token",
        "set",
        FOUNDATION,
        intent={
            "tokens": [
                token(
                    "size.viewport.width",
                    "sizing",
                    1440,
                    values={"Viewport/Narrow": 1024},
                ),
                token("size.viewport.height", "sizing", 960),
                token("size.content.max", "sizing", 860),
                token("size.dialog", "sizing", 520),
                token("size.settingsNav", "sizing", 192),
            ]
        },
        label="foundation-shell-tokens",
    )
    for package in [FOUNDATION, PRODUCT]:
        current = cli("project", "show", package, label="theme-selection-base")
        batch = {
            "baseRevision": current["revision"],
            "batchId": "alva-activate-defaults-" + current["revision"][:20],
            "operations": [
                {
                    "type": "set-active-token-themes",
                    "themePaths": [
                        "Theme/Default",
                        "Appearance/Light",
                        "Density/Comfortable",
                        "Viewport/Desktop",
                    ],
                }
            ],
        }
        path = INTENTS / ("active-defaults-" + package.name + ".json")
        path.write_text(json.dumps(batch, ensure_ascii=False, indent=2) + "\n")
        cli("advanced", "apply", package, "--batch", path, label="active-defaults")


def text(name, value, size="body", color="text", strong=False, **extra):
    return dict(
        name=name,
        text=value,
        fontFamily="{font.family}",
        fontSize="{font.size." + size + "}",
        fontWeight="{font.weight." + ("strong" if strong else "regular") + "}",
        color="{color." + color + "}",
        **extra,
    )


def box(name, children=(), layout="column", **extra):
    return dict(name=name, layout=layout, children=list(children), **extra)


def use(name, component, label=None, props=None, **extra):
    heights = {
        "ActionButton": "{size.control}",
        "TextField": "{size.control}",
        "NavigationItem": "{size.row}",
        "Tab": "{size.control}",
        "StatusBadge": 28,
        "Toggle": 30,
    }
    if component in heights and "height" not in extra:
        extra["height"] = heights[component]
    widths = {
        "ActionButton": 108,
        "TextField": 280,
        "NavigationItem": 224,
        "Tab": 132,
        "Toggle": 64,
    }
    if component in widths and "width" not in extra:
        extra["width"] = widths[component]
    d = dict(name=name, use=component, **extra)
    if label is not None:
        d["text"] = {"Label": label}
    if props:
        d["props"] = props
    return d


def button(name, label, style="Secondary", state="Default", **extra):
    return use(name, "ActionButton", label, {"Style": style, "State": state}, **extra)


def badge(name, label, tone="Neutral"):
    return use(name, "StatusBadge", label, {"Tone": tone})


def heading(name, title, subtitle="", actions=()):
    info = [text(name + "Title", title, "heading", strong=True, width="fill")]
    if subtitle:
        info.append(text(name + "Subtitle", subtitle, "caption", "muted", width="fill"))
    return box(
        name,
        [box(name + "Text", info, gap="{space.xs}", width="fill"), *actions],
        "row",
        align="center",
        width="fill",
        gap="{space.lg}",
    )


def card(name, children, **extra):
    return box(
        name,
        children,
        width="fill",
        padding="{space.xl}",
        gap="{space.lg}",
        fill="{color.surface}",
        stroke="{color.line}",
        radius="{radius.lg}",
        **extra,
    )


def components(only=None):
    defs = [
        {
            "name": "ActionButton",
            "properties": {
                "Style": ["Primary", "Secondary", "Ghost", "Danger"],
                "State": ["Default", "Hover", "Focus", "Disabled", "Loading"],
            },
            "base": box(
                "Button",
                [text("Label", "操作", "label", strong=True)],
                "row",
                width=108,
                height="{size.control}",
                align="center",
                justify="center",
                padding=[0, "{space.lg}"],
                gap="{space.sm}",
                fill="{color.surface}",
                stroke="{color.line}",
                radius="{radius.sm}",
            ),
            "variants": [
                {
                    "when": {"Style": "Primary"},
                    "set": {
                        "fill": "{color.primary}",
                        "stroke": "{color.primary}",
                        "Label.color": "{color.onPrimary}",
                    },
                },
                {
                    "when": {"Style": "Ghost"},
                    "set": {"fill": "{color.app}", "stroke": "{color.app}"},
                },
                {
                    "when": {"Style": "Danger"},
                    "set": {
                        "fill": "{color.dangerBg}",
                        "stroke": "{color.dangerBg}",
                        "Label.color": "{color.danger}",
                    },
                },
                {"when": {"State": "Hover"}, "set": {"stroke": "{color.primary}"}},
                {
                    "when": {"State": "Focus"},
                    "set": {"stroke": {"color": "{color.primary}", "width": 2}},
                },
                {
                    "when": {"State": "Disabled"},
                    "set": {
                        "fill": "{color.subtle}",
                        "stroke": "{color.line}",
                        "Label.color": "{color.muted}",
                    },
                },
                {"when": {"State": "Loading"}, "set": {"Label.text": "处理中"}},
            ],
        },
        {
            "name": "TextField",
            "properties": {"State": ["Default", "Focused", "Error", "Disabled"]},
            "base": box(
                "Input",
                [text("Label", "输入内容", "body", "muted", width="fill")],
                "row",
                width=280,
                height="{size.control}",
                padding=[0, "{space.md}"],
                align="center",
                fill="{color.surface}",
                stroke="{color.line}",
                radius="{radius.sm}",
            ),
            "variants": [
                {
                    "when": {"State": "Focused"},
                    "set": {"stroke": {"color": "{color.primary}", "width": 2}},
                },
                {"when": {"State": "Error"}, "set": {"stroke": "{color.danger}"}},
                {"when": {"State": "Disabled"}, "set": {"fill": "{color.subtle}"}},
            ],
        },
        {
            "name": "StatusBadge",
            "properties": {"Tone": ["Neutral", "Info", "Success", "Warning", "Danger"]},
            "base": box(
                "Badge",
                [text("Label", "状态", "caption", "muted", textAlign="center")],
                "row",
                justify="center",
                padding=[4, 8],
                height=28,
                align="center",
                fill="{color.subtle}",
                radius="{radius.pill}",
            ),
            "variants": [
                {
                    "when": {"Tone": "Info"},
                    "set": {
                        "fill": "{color.selected}",
                        "Label.color": "{color.selectedText}",
                    },
                },
                {
                    "when": {"Tone": "Success"},
                    "set": {
                        "fill": "{color.successBg}",
                        "Label.color": "{color.success}",
                    },
                },
                {
                    "when": {"Tone": "Warning"},
                    "set": {
                        "fill": "{color.warningBg}",
                        "Label.color": "{color.warning}",
                    },
                },
                {
                    "when": {"Tone": "Danger"},
                    "set": {
                        "fill": "{color.dangerBg}",
                        "Label.color": "{color.danger}",
                    },
                },
            ],
        },
        {
            "name": "NavigationItem",
            "properties": {"State": ["Default", "Selected"]},
            "base": box(
                "Nav",
                [
                    text("Icon", "○", "body", "muted", width=20, textAlign="center"),
                    text(
                        "Label",
                        "导航",
                        "body",
                        width="fill",
                        visible="{view.navLabels}",
                    ),
                ],
                "row",
                width=224,
                height="{size.row}",
                gap="{space.md}",
                padding=[0, 12],
                align="center",
                fill="{color.sidebar}",
                radius="{radius.sm}",
            ),
            "variants": [
                {
                    "when": {"State": "Selected"},
                    "set": {
                        "fill": "{color.selected}",
                        "Label.color": "{color.selectedText}",
                        "Icon.color": "{color.selectedText}",
                    },
                }
            ],
        },
        {
            "name": "Toggle",
            "properties": {"State": ["Off", "On", "Disabled"]},
            "base": box(
                "Toggle",
                [text("Label", "关闭", "caption", "muted")],
                "row",
                width=64,
                height=30,
                align="center",
                justify="center",
                fill="{color.subtle}",
                stroke="{color.line}",
                radius="{radius.pill}",
            ),
            "variants": [
                {
                    "when": {"State": "On"},
                    "set": {
                        "fill": "{color.selected}",
                        "stroke": "{color.selected}",
                        "Label.text": "开启",
                        "Label.color": "{color.selectedText}",
                    },
                },
                {"when": {"State": "Disabled"}, "set": {"Label.text": "不可用"}},
            ],
        },
        {
            "name": "Tab",
            "properties": {"State": ["Default", "Selected"]},
            "base": box(
                "Tab",
                [text("Label", "页签", "label", "muted")],
                "row",
                width=132,
                height="{size.control}",
                padding=[0, 12],
                align="center",
                justify="center",
                fill="{color.app}",
                radius="{radius.sm}",
            ),
            "variants": [
                {
                    "when": {"State": "Selected"},
                    "set": {
                        "fill": "{color.surface}",
                        "Label.color": "{color.text}",
                        "Label.fontWeight": "{font.weight.strong}",
                    },
                }
            ],
        },
        {
            "name": "SettingRow",
            "properties": {},
            "base": box(
                "Setting",
                [
                    box(
                        "Copy",
                        [
                            text("Title", "配置项", "body", strong=True, width="fill"),
                            text(
                                "Description",
                                "作用域与何时生效",
                                "caption",
                                "muted",
                                width="fill",
                            ),
                        ],
                        gap=6,
                        width="fill",
                    ),
                    use("Switch", "Toggle", props={"State": "Off"}),
                ],
                "row",
                width=620,
                padding=[16, 0],
                gap=16,
                align="center",
            ),
        },
        {
            "name": "ToolActivity",
            "properties": {"State": ["Running", "Done", "Error"]},
            "base": box(
                "Tool",
                [
                    text("Symbol", ">", "body", "muted", width=20),
                    box(
                        "ToolCopy",
                        [
                            text(
                                "Title",
                                "读取工作区文件",
                                "body",
                                strong=True,
                                width="fill",
                            ),
                            text(
                                "Detail",
                                "src/agent.go · 只读访问",
                                "caption",
                                "muted",
                                width="fill",
                            ),
                        ],
                        gap=6,
                        width="fill",
                    ),
                    use("Status", "StatusBadge", "运行中", {"Tone": "Info"}),
                ],
                "row",
                width=620,
                padding=16,
                gap=12,
                align="center",
                fill="{color.subtle}",
                radius="{radius.md}",
            ),
            "variants": [
                {
                    "when": {"State": "Done"},
                    "set": {
                        "Status.text": {"Label": "已完成"},
                        "Status.props": {"Tone": "Success"},
                    },
                },
                {
                    "when": {"State": "Error"},
                    "set": {
                        "Status.text": {"Label": "失败"},
                        "Status.props": {"Tone": "Danger"},
                    },
                },
            ],
        },
    ]
    for d in defs:
        if only is None or d["name"] == only:
            cli(
                "component",
                "define",
                FOUNDATION,
                intent=d,
                label="component-" + d["name"],
            )


NAV = [
    ("NavConversation", "对话", "◉"),
    ("NavPlugins", "插件与能力", "◇"),
    ("NavLibrary", "资料与记忆", "▤"),
    ("NavTeam", "协作", "◎"),
    ("NavAutomations", "自动化", "◷"),
    ("NavWaits", "等待任务", "◌"),
]


def sidebar(active="NavConversation"):
    rows = []
    for name, label, icon in NAV:
        rows.append(
            use(
                name,
                "NavigationItem",
                label,
                {"State": "Selected" if name == active else "Default"},
                width="fill",
                set={"Icon.text": icon},
            )
        )
    return box(
        "Sidebar",
        [
            box(
                "Brand",
                [
                    text("Mark", "A", "heading", "primary", True, width=24),
                    text(
                        "BrandLabel",
                        "ALVA",
                        "heading",
                        strong=True,
                        width="fill",
                        visible="{view.navLabels}",
                    ),
                ],
                "row",
                width="fill",
                height=48,
                align="center",
                gap=12,
            ),
            button(
                "NewConversation",
                "新建对话",
                "Primary",
                width="fill",
                visible="{view.navLabels}",
            ),
            button(
                "NewConversationIcon",
                "＋",
                "Primary",
                width="fill",
                visible="{view.navCompact}",
            ),
            *rows,
            box(
                "Recent",
                [
                    text("RecentTitle", "最近对话", "caption", "muted"),
                    text("RecentOne", "整理 Agent 架构", "label", width="fill"),
                    text(
                        "RecentTwo", "模型协议接入检查", "label", "muted", width="fill"
                    ),
                    text(
                        "RecentThree",
                        "设计动态内容界面",
                        "label",
                        "muted",
                        width="fill",
                    ),
                ],
                gap=14,
                padding=[20, 8],
                visible="{view.navDetails}",
                width="fill",
            ),
            box("SidebarSpacer", [], height="fill", width="fill"),
            use(
                "NavDeveloper",
                "NavigationItem",
                "开发者选项",
                {"State": "Selected" if active == "NavDeveloper" else "Default"},
                width="fill",
                set={"Icon.text": "⌘"},
            ),
            use(
                "NavSettings",
                "NavigationItem",
                "设置",
                {"State": "Selected" if active == "NavSettings" else "Default"},
                width="fill",
                set={"Icon.text": "⚙"},
            ),
            box(
                "WorkspaceContext",
                [
                    text("ProjectLabel", "当前项目", "caption", "muted"),
                    text(
                        "ProjectName", "alva-agent", "label", strong=True, width="fill"
                    ),
                ],
                gap=6,
                padding=12,
                fill="{color.surface}",
                radius="{radius.md}",
                visible="{view.navDetails}",
                width="fill",
            ),
        ],
        width="{size.sidebar}",
        height="fill",
        padding=[8, 12, 12, 12],
        gap=6,
        fill="{color.sidebar}",
    )


def shell(
    page,
    content,
    active="NavConversation",
    title="Alva 工作区",
    subtitle="项目 · alva-agent",
    actions=(),
    module="Workspace",
    header_badge=None,
    navigation=True,
):
    top = box(
        "WindowChrome",
        [
            text("WindowControls", "● ● ●", "caption", "muted"),
            text("WindowCaption", "Alva", "caption", "muted", width="fill"),
            text("WindowMode", "设计原型", "caption", "muted"),
        ],
        "row",
        width="fill",
        height="{size.windowBar}",
        padding=[0, 16],
        align="center",
        gap=16,
        fill="{color.sidebar}",
    )
    bar = [
        button(
            "ToggleSidebar", "收起导航" if navigation else "展开导航", "Ghost", width=96
        ),
        box(
            "PageIdentity",
            [
                text("PageTitle", title, "body", strong=True, width="fill"),
                text("PageScope", subtitle, "caption", "muted", width="fill"),
            ],
            width="fill",
            gap=4,
        ),
    ]
    if header_badge:
        bar.append(header_badge)
    bar += list(actions)
    main = box(
        "Main",
        [
            box(
                "PageBar",
                bar,
                "row",
                height="{size.pageBar}",
                width="fill",
                padding=[0, 24],
                gap=12,
                align="center",
                fill="{color.surface}",
                stroke={"color": "{color.line}", "width": [0, 0, 1, 0]},
            ),
            *content,
        ],
        width="fill",
        height="fill",
        gap=0,
    )
    return dict(
        page=page,
        module=module,
        platform="desktop",
        width="{size.viewport.width}",
        height="{size.viewport.height}",
        layout="column",
        fill="{color.app}",
        children=[
            top,
            box(
                "Application",
                ([sidebar(active)] if navigation else []) + [main],
                "row",
                width="fill",
                height="fill",
                gap=0,
            ),
        ],
    )


def body(children, gap=20):
    return box(
        "PageContent", children, width="fill", height="fill", padding=28, gap=gap
    )


def setting(name, title, desc, on=False):
    return use(
        name,
        "SettingRow",
        width="fill",
        text={"Title": title, "Description": desc},
        set={"Switch.props": {"State": "On" if on else "Off"}},
    )


def field(name, label, value, width="fill"):
    return box(
        name,
        [
            text(name + "Label", label, "caption", "muted"),
            use(name + "Input", "TextField", value, {"State": "Default"}, width="fill"),
        ],
        gap=8,
        width=width,
    )


def row(name, *children, **extra):
    return box(name, children, "row", gap=12, align="center", width="fill", **extra)


def composer(mode="idle"):
    return box(
        "ComposerArea",
        [
            box(
                "Composer",
                [
                    text(
                        "DraftText",
                        "继续讨论，或者输入新的任务…",
                        "body",
                        "muted",
                        width="fill",
                    ),
                    row(
                        "ComposerTools",
                        button("Attach", "＋ 附件", "Ghost", width=84),
                        button("SelectModel", "主模型 ▾", "Ghost", width=112),
                        text(
                            "ComposerHint",
                            (
                                "先处理待办；草稿会保留"
                                if mode == "waiting"
                                else (
                                    "运行中可追加指令"
                                    if mode == "running"
                                    else "Enter 发送 · Shift + Enter 换行"
                                )
                            ),
                            "caption",
                            "muted",
                            width="fill",
                        ),
                        button(
                            "Send",
                            (
                                "等待中"
                                if mode == "waiting"
                                else "追加" if mode == "running" else "发送"
                            ),
                            "Primary",
                            "Disabled" if mode == "waiting" else "Default",
                            width=80,
                        ),
                    ),
                ],
                width="fill",
                maxWidth="{size.content.max}",
                padding=16,
                gap=20,
                fill="{color.surface}",
                stroke="{color.line}",
                radius="{radius.lg}",
            ),
            text(
                "ComposerPrivacy",
                "文件先保存为引用，只有你选择的内容才会投递。",
                "caption",
                "muted",
                width="fill",
                maxWidth="{size.content.max}",
            ),
        ],
        width="fill",
        align="center",
        padding=[12, 28, 20, 28],
        gap=8,
    )


def conversation(mode="idle"):
    return [
        box(
            "Conversation",
            [
                box(
                    "ReadingColumn",
                    [
                        row(
                            "ConversationDate",
                            text(
                                "Date",
                                "今天 · 项目对话",
                                "caption",
                                "muted",
                                width="fill",
                            ),
                            badge("EngineBadge", "ALVA · 团队网关", "Info"),
                        ),
                        box(
                            "UserMessage",
                            [
                                text("UserLabel", "你", "caption", "muted"),
                                text(
                                    "UserText",
                                    "我们先把界面规划好。动态内容可以放在哪里？",
                                    "body",
                                    width="fill",
                                ),
                            ],
                            width="fill",
                            padding=20,
                            gap=8,
                            fill="{color.selected}",
                            radius="{radius.lg}",
                        ),
                        box(
                            "AssistantMessage",
                            [
                                text("AssistantLabel", "Alva", "caption", "muted"),
                                text(
                                    "AssistantText",
                                    "可以。让完整对话区域负责消息布局和滚动，周边保留清晰的导航与工具入口。\n\n图表、表单与插件页面可以在内容区域展开；它们的操作仍然交给当前会话处理。",
                                    "body",
                                    width="fill",
                                ),
                                use(
                                    "CompletedTool",
                                    "ToolActivity",
                                    props={"State": "Done"},
                                    width="fill",
                                ),
                            ],
                            width="fill",
                            gap=16,
                        ),
                        card(
                            "DynamicCard",
                            [
                                row(
                                    "DynamicHeading",
                                    text(
                                        "DynamicTitle",
                                        "界面区域分工",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("DynamicLabel", "交互内容", "Info"),
                                    button("ExpandArtifact", "展开", "Ghost", width=72),
                                ),
                                row(
                                    "Diagram",
                                    box(
                                        "DiagramNav",
                                        [
                                            text(
                                                "DiagramNavTitle",
                                                "导航",
                                                "label",
                                                strong=True,
                                            ),
                                            text(
                                                "DiagramNavText",
                                                "项目与会话",
                                                "caption",
                                                "muted",
                                            ),
                                        ],
                                        width="fill",
                                        padding=20,
                                        gap=8,
                                        fill="{color.subtle}",
                                        radius="{radius.md}",
                                    ),
                                    box(
                                        "DiagramChat",
                                        [
                                            text(
                                                "DiagramChatTitle",
                                                "对话",
                                                "label",
                                                strong=True,
                                            ),
                                            text(
                                                "DiagramChatText",
                                                "消息与动态内容",
                                                "caption",
                                                "muted",
                                            ),
                                        ],
                                        width="fill",
                                        padding=20,
                                        gap=8,
                                        fill="{color.selected}",
                                        radius="{radius.md}",
                                    ),
                                    box(
                                        "DiagramDetails",
                                        [
                                            text(
                                                "DiagramDetailsTitle",
                                                "详情",
                                                "label",
                                                strong=True,
                                            ),
                                            text(
                                                "DiagramDetailsText",
                                                "按需打开",
                                                "caption",
                                                "muted",
                                            ),
                                        ],
                                        width="fill",
                                        padding=20,
                                        gap=8,
                                        fill="{color.subtle}",
                                        radius="{radius.md}",
                                    ),
                                ),
                                text(
                                    "DynamicCaption",
                                    "来源：当前回复 · 展开不会重新执行任务",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                        row(
                            "ReplyActions",
                            button("CopyReply", "复制", "Ghost", width=72),
                            button("OpenSources", "来源", "Ghost", width=72),
                            text(
                                "ReplyTime",
                                "已完成 · 刚刚",
                                "caption",
                                "muted",
                                width="fill",
                            ),
                        ),
                    ],
                    width="fill",
                    maxWidth="{size.content.max}",
                    height="hug",
                    gap=24,
                )
            ],
            width="fill",
            height="fill",
            align="center",
            padding=[24, 28],
            gap=20,
        ),
        composer(mode),
    ]


def settings_body(section, children):
    nav = []
    for name, label in [
        ("General", "外观与布局"),
        ("Routes", "模型与路由"),
        ("Capabilities", "工具与连接"),
        ("Rules", "规则与 Skills"),
        ("Storage", "资料与存储"),
        ("Security", "权限与安全"),
        ("Gateway", "Gateway 服务"),
        ("Advanced", "高级与诊断"),
    ]:
        nav.append(
            use(
                "Settings" + name,
                "NavigationItem",
                label,
                {"State": "Selected" if section == name else "Default"},
                width="fill",
                set={"Icon.visible": False, "Label.visible": True},
            )
        )
    return box(
        "SettingsLayout",
        [
            box(
                "SettingsNavigation",
                [
                    text("SettingsScope", "设置范围", "caption", "muted"),
                    badge("ScopeBadge", "应用 / 当前项目"),
                    *nav,
                ],
                width="{size.settingsNav}",
                height="fill",
                padding=[24, 12],
                gap=8,
                fill="{color.surface}",
            ),
            box(
                "SettingsContent",
                children,
                width="fill",
                height="fill",
                padding=28,
                gap=20,
            ),
        ],
        "row",
        width="fill",
        height="fill",
        gap=0,
    )


def pages(only=None):
    defs = []
    defs.append(
        shell(
            "Overview",
            [
                body(
                    [
                        heading(
                            "Welcome",
                            "把想法，变成下一步。",
                            "创建一个对话，或回到你正在推进的项目。",
                        ),
                        card(
                            "StartCard",
                            [
                                text(
                                    "StartHeading", "从这里开始", "heading", strong=True
                                ),
                                text(
                                    "StartDescription",
                                    "为一个项目工作，或者创建独立会话。启动不会发送模型请求。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "StartActions",
                                    button(
                                        "CreateProject",
                                        "打开项目",
                                        "Primary",
                                        width=128,
                                    ),
                                    button("CreatePrivate", "独立会话", width=128),
                                ),
                            ],
                        ),
                        heading("RecentHeading", "最近的工作"),
                        card(
                            "RecentCard",
                            [
                                row(
                                    "RecentProject",
                                    text(
                                        "RecentProjectName",
                                        "alva-agent",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("RecentStatus", "已完成", "Success"),
                                    button("OpenRecent", "继续对话", width=104),
                                ),
                                text(
                                    "RecentSummary",
                                    "整理 Agent 核心与插件边界 · 12 分钟前",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                        card(
                            "OverviewNotice",
                            [
                                text(
                                    "OverviewNoticeTitle",
                                    "工作区由你控制",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "OverviewNoticeText",
                                    "模型路由、工具和项目规则都可以按需配置。待处理的提问与审批会一直保持入口可见。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                button(
                                    "OverviewSettings", "查看设置", "Ghost", width=108
                                ),
                            ],
                        ),
                    ]
                )
            ],
            title="工作区",
            subtitle="欢迎回来",
            module="Workspace",
        )
    )
    defs.append(
        shell(
            "Conversation",
            conversation(),
            title="规划动态内容界面",
            actions=[
                button("CommandPalette", "⌘ K", "Ghost", width=64),
                button("OpenInspector", "详情", width=80),
                button("ConversationSettings", "会话设置", width=104),
            ],
            module="Chat",
            header_badge=badge("RunStatus", "已完成", "Success"),
        )
    )
    defs.append(
        shell(
            "WaitingQuestion",
            [
                box(
                    "PersistentAttention",
                    [
                        row(
                            "AttentionRow",
                            badge("AttentionStatus", "需要你的回答", "Warning"),
                            text(
                                "AttentionSummary",
                                "1 个问题等待回答，任务已暂停。",
                                "body",
                                width="fill",
                            ),
                            button("AnswerQuestion", "查看问题", "Primary", width=104),
                        )
                    ],
                    width="fill",
                    padding=[12, 24],
                    fill="{color.warningBg}",
                ),
                *conversation("waiting"),
            ],
            title="规划动态内容界面",
            actions=[button("StopWaiting", "停止任务", "Danger", width=104)],
            module="Chat",
            header_badge=badge("WaitingStatus", "等待回答", "Warning"),
        )
    )
    defs.append(
        shell(
            "Plugins",
            [
                body(
                    [
                        heading(
                            "PluginsHeading",
                            "插件与能力",
                            "按项目装配工具、服务与界面。更改在下一轮生效。",
                            [button("InstallPlugin", "添加插件", "Primary", width=112)],
                        ),
                        row(
                            "PluginTabs",
                            use("AllPlugins", "Tab", "全部", {"State": "Selected"}),
                            use(
                                "EnabledPlugins", "Tab", "已启用", {"State": "Default"}
                            ),
                            use("UIPlugins", "Tab", "界面扩展", {"State": "Default"}),
                        ),
                        card(
                            "PluginOne",
                            [
                                row(
                                    "PluginOneHeader",
                                    text(
                                        "PluginOneName",
                                        "工作区文件",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("PluginOneRuntime", "Go 内置"),
                                    badge("PluginOneState", "已启用", "Success"),
                                ),
                                text(
                                    "PluginOneDesc",
                                    "按授权范围读取、编辑项目文件。具体操作仍由当前权限规则检查。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "PluginOneFooter",
                                    text(
                                        "PluginOneScope",
                                        "当前项目 · 文件工具",
                                        "caption",
                                        "muted",
                                        width="fill",
                                    ),
                                    button("PluginOneConfigure", "配置", width=80),
                                ),
                            ],
                        ),
                        card(
                            "PluginTwo",
                            [
                                row(
                                    "PluginTwoHeader",
                                    text(
                                        "PluginTwoName",
                                        "数据图表",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("PluginTwoRuntime", "WASM"),
                                    badge("PluginTwoUI", "界面扩展", "Info"),
                                ),
                                text(
                                    "PluginTwoDesc",
                                    "将结构化结果展示为交互图表，可在回复中展开或打开到独立面板。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "PluginTwoFooter",
                                    text(
                                        "PluginTwoScope",
                                        "当前项目 · 默认关闭",
                                        "caption",
                                        "muted",
                                        width="fill",
                                    ),
                                    button("PluginTwoPreview", "预览界面", width=104),
                                    button("PluginTwoEnable", "检查并启用", width=120),
                                ),
                            ],
                        ),
                        card(
                            "PluginThree",
                            [
                                row(
                                    "PluginThreeHeader",
                                    text(
                                        "PluginThreeName",
                                        "自定义脚本",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("PluginThreeRuntime", "JS / Python"),
                                    badge("PluginThreeAccess", "宿主访问", "Warning"),
                                ),
                                text(
                                    "PluginThreeDesc",
                                    "运行前检查固定版本与请求的访问范围。更新后重新审阅。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                button(
                                    "PluginThreeReview", "查看详情", "Ghost", width=108
                                ),
                            ],
                        ),
                    ]
                )
            ],
            active="NavPlugins",
            title="插件与能力",
            subtitle="当前项目 · alva-agent",
            module="Capabilities",
        )
    )
    defs.append(
        shell(
            "Library",
            [
                body(
                    [
                        heading(
                            "LibraryHeading",
                            "资料与记忆",
                            "原始文件、处理结果和投递记录分别保留。",
                            [button("LibraryImport", "添加资料", "Primary", width=112)],
                        ),
                        row(
                            "LibraryTabs",
                            use(
                                "LibraryFiles", "Tab", "项目资料", {"State": "Selected"}
                            ),
                            use("LibraryMemory", "Tab", "记忆", {"State": "Default"}),
                            use(
                                "LibraryReceipts",
                                "Tab",
                                "投递记录",
                                {"State": "Default"},
                            ),
                        ),
                        card(
                            "LibraryFilesCard",
                            [
                                row(
                                    "FileA",
                                    text(
                                        "FileAName",
                                        "architecture-notes.md",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("FileAStatus", "仅保存"),
                                    button("PreviewMedia", "预览", width=80),
                                ),
                                text(
                                    "FileADesc",
                                    "原始文件 · 18 KB · 尚未投递给模型",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "FileB",
                                    text(
                                        "FileBName",
                                        "workspace-overview.png",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("FileBStatus", "已选择派生图", "Info"),
                                    button("FileBPreview", "来源与处理", width=112),
                                ),
                                text(
                                    "FileBDesc",
                                    "原件与派生内容独立保存 · 点击查看实际处理记录",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                        card(
                            "MemoryCard",
                            [
                                heading(
                                    "MemoryHeading",
                                    "项目记忆",
                                    "当前项目内共享，是否启用由项目配置决定。",
                                ),
                                setting(
                                    "MemoryToggle",
                                    "持久项目记忆",
                                    "启用不代表自动扫描、提取或向量上传。",
                                ),
                                button(
                                    "MemorySettings",
                                    "管理存储与范围",
                                    "Ghost",
                                    width=144,
                                ),
                            ],
                        ),
                    ]
                )
            ],
            active="NavLibrary",
            title="资料与记忆",
            module="Knowledge",
        )
    )
    defs.append(
        shell(
            "Team",
            [
                body(
                    [
                        heading(
                            "TeamHeading",
                            "协作",
                            "各成员拥有独立会话与任务队列。",
                            [button("AddWorker", "添加成员", "Primary", width=112)],
                        ),
                        card(
                            "LeadCard",
                            [
                                row(
                                    "LeadRow",
                                    text(
                                        "LeadName",
                                        "主 Agent",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("LeadRole", "协调者", "Info"),
                                ),
                                text(
                                    "LeadTask",
                                    "整理当前项目方案，汇总各成员的结果。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                        card(
                            "WorkerCard",
                            [
                                row(
                                    "WorkerRow",
                                    text(
                                        "WorkerName",
                                        "架构审阅",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("WorkerStatus", "已暂停", "Warning"),
                                    button("WorkerOpen", "查看会话", width=104),
                                ),
                                text(
                                    "WorkerTask",
                                    "队列：检查插件接口边界 · 独立账户与权限范围",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "WorkerActions",
                                    button(
                                        "WorkerResume", "继续队列", "Primary", width=104
                                    ),
                                    button("WorkerStop", "保持暂停", width=104),
                                ),
                            ],
                        ),
                        card(
                            "TeamInfo",
                            [
                                text(
                                    "TeamInfoTitle",
                                    "先入队，再明确执行",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "TeamInfoText",
                                    "创建成员和查看结果不会启动任务；继续执行时才使用所选模型路由。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ]
                )
            ],
            active="NavTeam",
            title="协作",
            module="Operations",
        )
    )
    defs.append(
        shell(
            "Automations",
            [
                body(
                    [
                        heading(
                            "AutomationHeading",
                            "自动化",
                            "新计划默认暂停。配置与执行分开。",
                            [
                                button(
                                    "CreateAutomation", "新建计划", "Primary", width=112
                                )
                            ],
                        ),
                        card(
                            "AutomationCard",
                            [
                                row(
                                    "AutomationRow",
                                    text(
                                        "AutomationName",
                                        "每周项目回顾",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("AutomationState", "已暂停", "Warning"),
                                    button("AutomationEdit", "编辑计划", width=104),
                                ),
                                text(
                                    "AutomationSchedule",
                                    "每周一 09:00 · 当前项目 · 指定模型路由",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "AutomationActions",
                                    button(
                                        "AutomationEnable",
                                        "审阅并启用",
                                        "Primary",
                                        width=128,
                                    ),
                                    button("AutomationHistory", "运行记录", width=104),
                                ),
                            ],
                        ),
                        card(
                            "AutomationEmpty",
                            [
                                text(
                                    "AutomationEmptyTitle",
                                    "每次运行都有清楚的归属",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "AutomationEmptyText",
                                    "时间、目标会话、账户、工具权限及成本范围在启用前一起审阅。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ]
                )
            ],
            active="NavAutomations",
            title="自动化",
            module="Operations",
        )
    )
    defs.append(
        shell(
            "Waits",
            [
                body(
                    [
                        heading(
                            "WaitHeading",
                            "等待任务",
                            "保存回答后仍保持暂停，只有明确继续才恢复执行。",
                        ),
                        card(
                            "WaitCard",
                            [
                                row(
                                    "WaitRow",
                                    text(
                                        "WaitName",
                                        "规划动态内容界面",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("WaitReason", "等待回答", "Warning"),
                                    button(
                                        "WaitOpenQuestion",
                                        "处理问题",
                                        "Primary",
                                        width=112,
                                    ),
                                ),
                                text(
                                    "WaitQuestion",
                                    "你更希望先实现哪种布局？",
                                    "body",
                                    width="fill",
                                ),
                                text(
                                    "WaitBinding",
                                    "当前项目 · 原会话 · 本次任务",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "WaitActions",
                                    button("WaitSave", "保存回答", width=104),
                                    button(
                                        "WaitContinue",
                                        "继续任务",
                                        "Primary",
                                        "Disabled",
                                        width=104,
                                    ),
                                    button(
                                        "WaitAbandon", "审阅放弃", "Ghost", width=104
                                    ),
                                ),
                            ],
                        ),
                        card(
                            "WaitHelp",
                            [
                                text(
                                    "WaitHelpTitle",
                                    "离开页面不会丢失等待",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "WaitHelpText",
                                    "待办保留入口。关闭查看窗口不会替你回答，也不会自动继续。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ]
                )
            ],
            active="NavWaits",
            title="等待任务",
            module="Operations",
            header_badge=badge("WaitCount", "1 个待处理", "Warning"),
        )
    )
    defs.append(
        shell(
            "General",
            [
                settings_body(
                    "General",
                    [
                        heading(
                            "GeneralHeading",
                            "外观与布局",
                            "应用偏好立即生效；窗口布局与会话执行状态分别保存。",
                        ),
                        card(
                            "AppearanceCard",
                            [
                                row(
                                    "AppearanceRow",
                                    text(
                                        "AppearanceTitle",
                                        "外观",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    button(
                                        "ChooseAppearance",
                                        "{label.appearance}",
                                        width=160,
                                    ),
                                ),
                                row(
                                    "DensityRow",
                                    text(
                                        "DensityTitle",
                                        "界面密度",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    button(
                                        "ChooseDensity", "{label.density}", width=160
                                    ),
                                ),
                            ],
                        ),
                        card(
                            "LayoutCard",
                            [
                                setting(
                                    "RememberLayout",
                                    "记住窗口布局",
                                    "仅保存导航、侧栏、面板宽度等显示偏好。",
                                    True,
                                ),
                                setting(
                                    "ShowDetails",
                                    "默认打开详情面板",
                                    "窄窗口改用抽屉，主对话保持可读。",
                                ),
                                setting(
                                    "ShowActivity",
                                    "显示运行详情",
                                    "可以折叠明细；运行状态与停止入口始终可达。",
                                    True,
                                ),
                                button("RestoreLayout", "恢复默认布局", width=136),
                            ],
                        ),
                        card(
                            "PreferencesScope",
                            [
                                text(
                                    "PreferencesTitle",
                                    "应用、项目、会话分开设置",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "PreferencesDescription",
                                    "每项配置显示其作用范围和来源；当前会话覆盖不会悄悄改写全局设置。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        )
    )
    defs.append(
        shell(
            "Routes",
            [
                settings_body(
                    "Routes",
                    [
                        heading(
                            "RoutesHeading",
                            "模型与路由",
                            "区分执行引擎、模型服务、账户与计费路由。",
                        ),
                        card(
                            "ActiveRoute",
                            [
                                row(
                                    "RouteHeading",
                                    text(
                                        "RouteTitle",
                                        "当前会话路由",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("RouteScope", "当前会话", "Info"),
                                ),
                                row(
                                    "RouteFields",
                                    field("EngineField", "执行引擎", "ALVA"),
                                    field("ConnectionField", "模型连接", "团队网关"),
                                ),
                                row(
                                    "ModelFields",
                                    field("ModelField", "模型", "主模型"),
                                    field("AccountField", "账户与计费", "项目开发账户"),
                                ),
                                text(
                                    "RouteWhen",
                                    "切换在当前任务结束后生效；保留上一条引擎分支。",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                                button(
                                    "ApplyRoute", "审阅并切换", "Primary", width=128
                                ),
                            ],
                        ),
                        card(
                            "Fallback",
                            [
                                setting(
                                    "FallbackToggle",
                                    "自动回退",
                                    "默认关闭。启用前明确选择目标引擎、账户和计费范围。",
                                ),
                                button("ConfigureFallback", "配置回退目标", width=128),
                            ],
                        ),
                        card(
                            "Connections",
                            [
                                row(
                                    "ConnectionsTitle",
                                    text(
                                        "ConnectionsHeading",
                                        "已配置连接",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    button("AddConnection", "添加连接", width=104),
                                ),
                                text(
                                    "ConnectionsText",
                                    "团队网关 · OpenAI / Anthropic / Gemini 协议",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        )
    )
    defs.append(
        shell(
            "Capabilities",
            [
                settings_body(
                    "Capabilities",
                    [
                        heading(
                            "CapabilitiesHeading",
                            "工具与连接",
                            "检查访问范围后启用，下轮生效。",
                        ),
                        card(
                            "ToolsConfig",
                            [
                                setting(
                                    "FileTools",
                                    "文件工具",
                                    "当前项目内的读取与编辑；具体写入仍检查权限。",
                                    True,
                                ),
                                setting(
                                    "ShellTools",
                                    "命令执行",
                                    "可启动真实进程，需要单独审阅宿主访问。",
                                ),
                                setting(
                                    "WebTools",
                                    "网络与浏览器",
                                    "目标域、浏览器配置与授权范围明确可见。",
                                ),
                                setting(
                                    "ChildAgents",
                                    "子 Agent",
                                    "与协作成员分开，按实际父级授权派生。",
                                ),
                            ],
                        ),
                        card(
                            "MCPCard",
                            [
                                row(
                                    "MCPHeading",
                                    text(
                                        "MCPTitle",
                                        "MCP 连接",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    button("AddMCP", "添加连接", width=104),
                                ),
                                row(
                                    "MCPRow",
                                    text(
                                        "MCPName", "项目文档服务", "body", width="fill"
                                    ),
                                    badge("MCPState", "已保存 · 未启用", "Warning"),
                                    button("MCPReview", "检查并启用", width=128),
                                ),
                                text(
                                    "MCPHint",
                                    "编辑连接后重新检查；保存不会自动启动服务。",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                        button("ManagePlugins", "打开插件管理", "Ghost", width=136),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        )
    )
    defs.append(
        shell(
            "Rules",
            [
                settings_body(
                    "Rules",
                    [
                        heading(
                            "RulesHeading",
                            "规则与 Skills",
                            "项目规则、兼容加载与可执行能力分别管理。",
                        ),
                        card(
                            "RulesCard",
                            [
                                setting(
                                    "NativeRules",
                                    "Alva 项目规则",
                                    "只从明确的项目范围加载。",
                                    True,
                                ),
                                setting(
                                    "CompatibilityRules",
                                    "兼容标准规则",
                                    "按需加载 AGENTS.md、CLAUDE.md 与 Agent Skills；默认关闭。",
                                ),
                                row(
                                    "RulesSource",
                                    text(
                                        "RulesPath",
                                        "来源：项目配置 · .alva/compatibility.json",
                                        "caption",
                                        "muted",
                                        width="fill",
                                    ),
                                    button("RulesPreview", "预览加载结果", width=128),
                                ),
                            ],
                        ),
                        card(
                            "SkillsCard",
                            [
                                row(
                                    "SkillHeading",
                                    text(
                                        "SkillsTitle",
                                        "项目 Skills",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    button("SkillAdd", "添加 Skill", width=104),
                                ),
                                row(
                                    "SkillItem",
                                    text("SkillName", "架构审阅", "body", width="fill"),
                                    badge("SkillState", "按需加载", "Info"),
                                    button("SkillEdit", "查看规则", width=104),
                                ),
                                text(
                                    "SkillDedup",
                                    "重复来源按规范身份去重，预览保留胜出来源与覆盖说明。",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        )
    )
    defs.append(
        shell(
            "Storage",
            [
                settings_body(
                    "Storage",
                    [
                        heading(
                            "StorageHeading",
                            "资料与存储",
                            "由应用装配存储组件；界面呈现能力、范围和状态。",
                        ),
                        card(
                            "StorageCard",
                            [
                                row(
                                    "StorageTitle",
                                    text(
                                        "StorageBackendTitle",
                                        "当前存储组件",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("StorageScope", "当前项目"),
                                ),
                                field("StorageBackend", "已绑定后端", "本地持久存储"),
                                text(
                                    "StorageNoSwitch",
                                    "切换后端需要明确迁移计划，不把修改显示名称当作迁移完成。",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "StorageActions",
                                    button(
                                        "StorageDetails", "查看能力与状态", width=152
                                    ),
                                    button("StorageExport", "导出项目数据", width=128),
                                ),
                            ],
                        ),
                        card(
                            "MemoryStorage",
                            [
                                setting(
                                    "MemoryPersistent",
                                    "持久项目记忆",
                                    "默认内存能力可由已安装的存储组件替换。",
                                ),
                                setting(
                                    "OriginalRetention",
                                    "保留原始资料",
                                    "派生处理与引用不会覆盖原始文件。",
                                    True,
                                ),
                            ],
                        ),
                        card(
                            "StorageFuture",
                            [
                                text(
                                    "StorageFutureTitle",
                                    "通过同一能力接口接入",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "StorageFutureText",
                                    "内存、云盘或其他存储由外层组件提供。不可用的能力明确显示，不静默退回另一后端。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                            ],
                        ),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        )
    )
    defs.append(
        shell(
            "Security",
            [
                settings_body(
                    "Security",
                    [
                        heading(
                            "SecurityHeading",
                            "权限与安全",
                            "回答问题、允许操作与配置能力是三种不同动作。",
                        ),
                        card(
                            "SecurityCard",
                            [
                                row(
                                    "SecurityMode",
                                    text(
                                        "SecurityModeTitle",
                                        "当前权限模式",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("SecurityModeBadge", "操作前询问", "Warning"),
                                ),
                                row(
                                    "KeepApprovalVisible",
                                    text(
                                        "ApprovalVisibilityRule",
                                        "未决请求保留待办入口",
                                        "body",
                                        width="fill",
                                    ),
                                    badge("AlwaysVisible", "始终显示", "Info"),
                                ),
                                row(
                                    "ShowScope",
                                    text(
                                        "ApprovalScopeRule",
                                        "操作前展示来源与访问范围",
                                        "body",
                                        width="fill",
                                    ),
                                    badge("AlwaysScoped", "始终显示", "Info"),
                                ),
                                button("ReviewGrants", "查看当前授权", width=128),
                            ],
                        ),
                        card(
                            "SecurityExample",
                            [
                                text(
                                    "SecurityExampleTitle",
                                    "本次操作：修改项目文件",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "SecurityExampleText",
                                    "从问题中选择 Other 不会授予文件写入、网络或进程权限。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                button(
                                    "OpenApprovalDemo",
                                    "查看审批样式",
                                    "Ghost",
                                    width=144,
                                ),
                            ],
                        ),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        )
    )
    for evaluation in [False, True]:
        name = "Evaluation" if evaluation else "IntegrationTests"
        defs.append(
            shell(
                name,
                [
                    body(
                        [
                            heading(
                                "DevHeading",
                                "开发者选项",
                                "接入测试验证契约，模型评估衡量指定任务集的表现。",
                            ),
                            row(
                                "DeveloperTabs",
                                use(
                                    "IntegrationTab",
                                    "Tab",
                                    "接入测试",
                                    {"State": "Default" if evaluation else "Selected"},
                                ),
                                use(
                                    "EvaluationTab",
                                    "Tab",
                                    "模型评估",
                                    {"State": "Selected" if evaluation else "Default"},
                                ),
                            ),
                            card(
                                "DevPlan",
                                [
                                    row(
                                        "DevPlanHeading",
                                        text(
                                            "DevPlanTitle",
                                            (
                                                "模型评估计划"
                                                if evaluation
                                                else "工具契约测试"
                                            ),
                                            "body",
                                            strong=True,
                                            width="fill",
                                        ),
                                        badge("DevScope", "单响应范围"),
                                    ),
                                    text(
                                        "DevPlanDescription",
                                        (
                                            "版本化任务集、重复试次与加权分数；执行错误单独报告。"
                                            if evaluation
                                            else "检查工具选择、参数与不调用工具的行为；不生成质量排名。"
                                        ),
                                        "body",
                                        "muted",
                                        width="fill",
                                    ),
                                    row(
                                        "DevInputs",
                                        field(
                                            "DevRoute", "模型路由", "团队网关 · 主模型"
                                        ),
                                        field(
                                            "DevDataset",
                                            "任务集" if evaluation else "工具名称",
                                            (
                                                "基础任务 v1"
                                                if evaluation
                                                else "ask_human"
                                            ),
                                        ),
                                    ),
                                    row(
                                        "DevBudget",
                                        field("DevRepeats", "重复次数", "3"),
                                        field("DevCalls", "调用上限", "24"),
                                        field("DevTimeout", "单次超时", "30 秒"),
                                    ),
                                    row(
                                        "DevButtons",
                                        button("PreviewPlan", "预览计划", width=112),
                                        button(
                                            (
                                                "RunEvaluation"
                                                if evaluation
                                                else "RunTests"
                                            ),
                                            "运行评估" if evaluation else "运行测试",
                                            "Primary",
                                            width=112,
                                        ),
                                    ),
                                    text(
                                        "DevCost",
                                        "运行使用已选模型账户，可能产生推理费用。预览不发送请求。",
                                        "caption",
                                        "muted",
                                        width="fill",
                                    ),
                                ],
                            ),
                            card(
                                "DevResult",
                                [
                                    text(
                                        "DevResultTitle",
                                        "还没有运行结果",
                                        "body",
                                        strong=True,
                                    ),
                                    text(
                                        "DevResultText",
                                        "查看页面不会开始执行。报告独立于聊天历史。",
                                        "body",
                                        "muted",
                                        width="fill",
                                    ),
                                ],
                            ),
                        ]
                    )
                ],
                active="NavDeveloper",
                title="开发者选项",
                subtitle="独立作业 · 无需聊天会话",
                module="Developer",
            )
        )
    for d in defs:
        if only is None or d["page"] == only:
            cli(
                "page",
                "draw",
                PRODUCT,
                intent=d,
                label="page-" + d["module"] + "-" + d["page"],
            )


def overlays(only=None):
    defs = []

    def overlay(name, title, subtitle, children, width=520, height=620):
        return dict(
            page=name,
            module="Overlay",
            platform="desktop",
            width=width,
            height=height,
            layout="column",
            fill="{color.surface}",
            padding=24,
            gap=20,
            children=[
                heading(
                    "OverlayHeading",
                    title,
                    subtitle,
                    [button("CloseOverlay", "关闭", "Ghost", width=72)],
                ),
                *children,
            ],
        )

    defs.append(
        overlay(
            "Question",
            "需要你的选择",
            "当前会话 · 问题 1 / 1",
            [
                text(
                    "QuestionText",
                    "你更希望先实现哪种布局？",
                    "heading",
                    strong=True,
                    width="fill",
                ),
                box(
                    "QuestionOptions",
                    [
                        button("OptionA", "A · 原生外壳与整块对话", width="fill"),
                        button("OptionB", "B · 主要工作区使用 HTML", width="fill"),
                        button("OptionOther", "Other · 自定义答案", width="fill"),
                    ],
                    gap=12,
                    width="fill",
                ),
                use(
                    "OtherText",
                    "TextField",
                    "输入你自己的偏好…",
                    {"State": "Default"},
                    width="fill",
                ),
                text(
                    "QuestionHelp",
                    "未预选任何答案。关闭面板会保留待办，任务仍然暂停。",
                    "body",
                    "muted",
                    width="fill",
                ),
                row(
                    "QuestionActions",
                    button("QuestionCancel", "稍后回答", width=112),
                    button("QuestionSubmit", "提交回答", "Primary", width=112),
                ),
            ],
            height=520,
        )
    )
    defs.append(
        overlay(
            "Approval",
            "允许本次操作？",
            "待审批 · 文件写入",
            [
                badge("ApprovalKind", "权限请求", "Warning"),
                text(
                    "ApprovalAction",
                    "修改项目文件",
                    "heading",
                    strong=True,
                    width="fill",
                ),
                card(
                    "ApprovalScope",
                    [
                        text(
                            "ApprovalTarget",
                            "src/agent.go",
                            "body",
                            strong=True,
                            width="fill",
                        ),
                        text(
                            "ApprovalReason",
                            "当前任务需要更新插件装配逻辑。",
                            "body",
                            "muted",
                            width="fill",
                        ),
                        text(
                            "ApprovalOwner",
                            "来源：当前会话 / Alva / 本次工具调用",
                            "caption",
                            "muted",
                            width="fill",
                        ),
                    ],
                ),
                text(
                    "ApprovalHint",
                    "只决定本次请求。关闭面板不代表允许。",
                    "body",
                    "muted",
                    width="fill",
                ),
                row(
                    "ApprovalActions",
                    button("ApprovalDeny", "拒绝本次", "Danger", width=120),
                    button("ApprovalAllow", "允许本次", "Primary", width=120),
                ),
            ],
            height=550,
        )
    )
    defs.append(
        overlay(
            "Inspector",
            "运行详情",
            "观察当前任务，不启动或重放执行",
            [
                row(
                    "InspectorTabs",
                    use("InspectorTimeline", "Tab", "时间线", {"State": "Selected"}),
                    use("InspectorRecords", "Tab", "记录", {"State": "Default"}),
                ),
                card(
                    "InspectorSummary",
                    [
                        text(
                            "InspectorTask",
                            "规划动态内容界面",
                            "body",
                            strong=True,
                            width="fill",
                        ),
                        badge("InspectorState", "已完成", "Success"),
                        text(
                            "InspectorOwner",
                            "当前会话 · ALVA · 当前分支",
                            "caption",
                            "muted",
                            width="fill",
                        ),
                    ],
                ),
                use(
                    "InspectorTool",
                    "ToolActivity",
                    props={"State": "Done"},
                    width="fill",
                ),
                card(
                    "InspectorReference",
                    [
                        text(
                            "InspectorReferenceTitle",
                            "结果以引用保留",
                            "body",
                            strong=True,
                        ),
                        text(
                            "InspectorReferenceBody",
                            "展开时按需读取；离开面板不会改变任务。",
                            "body",
                            "muted",
                            width="fill",
                        ),
                    ],
                ),
                button("DetachInspector", "在独立窗口查看", width="fill"),
            ],
            width=420,
            height=760,
        )
    )
    defs.append(
        overlay(
            "CommandPalette",
            "快速打开",
            "输入页面、命令或当前任务",
            [
                use(
                    "CommandSearch",
                    "TextField",
                    "搜索…",
                    {"State": "Focused"},
                    width="fill",
                ),
                box(
                    "Commands",
                    [
                        button("CommandGoChat", "返回当前对话", "Ghost", width="fill"),
                        button("CommandGoPlugins", "插件与能力", "Ghost", width="fill"),
                        button("CommandGoSettings", "打开设置", "Ghost", width="fill"),
                        button(
                            "CommandGoDeveloper", "开发者选项", "Ghost", width="fill"
                        ),
                    ],
                    gap=8,
                    width="fill",
                ),
                text(
                    "CommandHint",
                    "Esc 关闭 · 焦点返回原来的控件",
                    "caption",
                    "muted",
                    width="fill",
                ),
            ],
            width=620,
            height=420,
        )
    )
    defs.append(
        overlay(
            "NewSession",
            "开始新的对话",
            "项目对话或独立会话",
            [
                row(
                    "NewSessionTabs",
                    use("ProjectSession", "Tab", "项目对话", {"State": "Selected"}),
                    use("PrivateSession", "Tab", "独立会话", {"State": "Default"}),
                ),
                field("NewTitle", "对话名称", "新的项目讨论"),
                field("NewWorkspace", "工作区", "选择项目目录…"),
                field("NewRoute", "模型路由", "ALVA · 团队网关"),
                text(
                    "NewSessionHint",
                    "创建对话不会发送消息，也不会启动模型。",
                    "body",
                    "muted",
                    width="fill",
                ),
                row(
                    "NewSessionActions",
                    button("NewSessionCancel", "取消", width=104),
                    button("NewSessionCreate", "创建对话", "Primary", width=120),
                ),
            ],
            height=560,
        )
    )
    defs.append(
        overlay(
            "Artifact",
            "界面区域分工",
            "来自当前回复 · 交互内容",
            [
                card(
                    "ArtifactPreview",
                    [
                        text(
                            "ArtifactTitle",
                            "导航 → 对话 → 详情",
                            "heading",
                            strong=True,
                            width="fill",
                        ),
                        text(
                            "ArtifactDescription",
                            "同一份内容可在消息内预览，也可在这里展开。",
                            "body",
                            "muted",
                            width="fill",
                        ),
                        row(
                            "ArtifactRegions",
                            box(
                                "ArtifactA",
                                [
                                    text("ArtifactATitle", "导航", "body", strong=True),
                                    text("ArtifactAText", "可折叠", "caption", "muted"),
                                ],
                                width="fill",
                                padding=24,
                                gap=12,
                                fill="{color.subtle}",
                                radius="{radius.md}",
                            ),
                            box(
                                "ArtifactB",
                                [
                                    text("ArtifactBTitle", "对话", "body", strong=True),
                                    text(
                                        "ArtifactBText", "独立滚动", "caption", "muted"
                                    ),
                                ],
                                width="fill",
                                padding=24,
                                gap=12,
                                fill="{color.selected}",
                                radius="{radius.md}",
                            ),
                            box(
                                "ArtifactC",
                                [
                                    text("ArtifactCTitle", "详情", "body", strong=True),
                                    text(
                                        "ArtifactCText", "按需展开", "caption", "muted"
                                    ),
                                ],
                                width="fill",
                                padding=24,
                                gap=12,
                                fill="{color.subtle}",
                                radius="{radius.md}",
                            ),
                        ),
                    ],
                ),
                row(
                    "ArtifactActions",
                    button("ArtifactFullscreen", "全屏查看", width=112),
                    button("ArtifactDetach", "独立窗口", width=112),
                    button("ArtifactBack", "返回对话", "Primary", width=112),
                ),
            ],
            width=920,
            height=560,
        )
    )
    for d in defs:
        if only is None or d["page"] == only:
            cli("page", "draw", PRODUCT, intent=d, label="overlay-" + d["page"])


def links():
    from flows import connect

    connect()


if __name__ == "__main__":
    actions = {
        "tokens": tokens,
        "components": components,
        "pages": pages,
        "overlays": overlays,
        "links": links,
    }
    for stage in sys.argv[1:]:
        name, _, target = stage.partition(":")
        if target:
            actions[name](target)
        else:
            actions[name]()
