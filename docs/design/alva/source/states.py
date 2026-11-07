# INPUT: author composition helpers and installed SmallPen typed operations
# OUTPUT: Macro layout states, remaining settings and question state prototypes
# POS: Product design states; no application runtime or canonical JSON mutation.
from author import (
    FOUNDATION,
    PRODUCT,
    badge,
    body,
    box,
    button,
    card,
    cli,
    conversation,
    field,
    heading,
    row,
    setting,
    settings_body,
    shell,
    text,
    token,
    use,
)


def foundation():
    cli(
        "token",
        "set",
        FOUNDATION,
        intent={
            "tokens": [
                token(
                    "view.navCompact",
                    "boolean",
                    False,
                    values={"Viewport/Narrow": True},
                ),
            ]
        },
        label="compact-navigation-token",
    )
    cli(
        "component",
        "define",
        FOUNDATION,
        intent={
            "name": "ChoiceOption",
            "properties": {
                "Kind": ["Single", "Multiple"],
                "State": ["Default", "Selected", "Focus", "Disabled"],
            },
            "base": box(
                "Choice",
                [
                    text("Selector", "○", "body", "muted", width=20, textAlign="center"),
                    text("Label", "选择一个选项", "body", width="fill"),
                ],
                "row",
                width=440,
                padding=16,
                gap=12,
                align="center",
                fill="{color.surface}",
                stroke="{color.line}",
                radius="{radius.md}",
            ),
            "variants": [
                {"when": {"Kind": "Multiple"}, "set": {"Selector.text": "□"}},
                {
                    "when": {"State": "Selected"},
                    "set": {
                        "fill": "{color.selected}",
                        "stroke": "{color.primary}",
                        "Label.color": "{color.selectedText}",
                        "Selector.text": "●",
                        "Selector.color": "{color.selectedText}",
                    },
                },
                {
                    "when": {"State": "Selected", "Kind": "Multiple"},
                    "set": {"Selector.text": "✓"},
                },
                {
                    "when": {"State": "Focus"},
                    "set": {"stroke": {"color": "{color.primary}", "width": 2}},
                },
                {
                    "when": {"State": "Disabled"},
                    "set": {"fill": "{color.subtle}", "Label.color": "{color.muted}"},
                },
            ],
        },
        label="component-ChoiceOption",
    )
    cli(
        "component",
        "define",
        FOUNDATION,
        intent={
            "name": "Feedback",
            "properties": {"State": ["Loading", "Empty", "Error", "Unsupported"]},
            "base": box(
                "Feedback",
                [
                    text("Title", "正在加载", "body", strong=True, width="fill"),
                    text(
                        "Description",
                        "读取当前会话的结果引用…",
                        "caption",
                        "muted",
                        width="fill",
                    ),
                ],
                width=440,
                padding=20,
                gap=8,
                fill="{color.subtle}",
                stroke="{color.line}",
                radius="{radius.md}",
            ),
            "variants": [
                {
                    "when": {"State": "Empty"},
                    "set": {
                        "Title.text": "还没有内容",
                        "Description.text": "执行后会显示结果；打开面板不会开始执行。",
                    },
                },
                {
                    "when": {"State": "Error"},
                    "set": {
                        "fill": "{color.dangerBg}",
                        "Title.text": "内容加载失败",
                        "Title.color": "{color.danger}",
                        "Description.text": "保留来源，可重试或查看原文。",
                    },
                },
                {
                    "when": {"State": "Unsupported"},
                    "set": {
                        "Title.text": "当前版本无法显示此模块",
                        "Description.text": "原文与来源仍然可用。",
                    },
                },
            ],
        },
        label="component-Feedback",
    )


def panel():
    return box(
        "DockedInspector",
        [
            heading("DockedHeading", "运行详情", "观察当前会话"),
            row(
                "DetailTabs",
                use("DetailActivity", "Tab", "活动", {"State": "Selected"}),
                use("DetailContext", "Tab", "上下文", {"State": "Default"}),
            ),
            badge("DetailStatus", "已完成", "Success"),
            text("DetailTitle", "规划动态内容界面", "body", strong=True, width="fill"),
            use("DetailTool", "ToolActivity", props={"State": "Done"}, width="fill"),
            card(
                "DetailOwnership",
                [
                    text(
                        "DetailOwnerTitle",
                        "当前引擎分支",
                        "body",
                        strong=True,
                        width="fill",
                    ),
                    text(
                        "DetailOwner",
                        "ALVA · 当前会话",
                        "caption",
                        "muted",
                        width="fill",
                    ),
                    button("HandoffEvidence", "引擎与交接", width="fill"),
                ],
            ),
            button("DetachInspector", "在独立窗口查看", width="fill"),
            button("CloseInspector", "收起详情", "Ghost", width="fill"),
        ],
        width="{size.inspector}",
        height="fill",
        padding=20,
        gap=20,
        fill="{color.surface}",
        stroke={"color": "{color.line}", "width": [0, 0, 0, 1]},
        visible="{view.navDetails}",
    )


def layouts(only=None):
    defs = [
        shell(
            "Focus",
            conversation(),
            title="规划动态内容界面",
            module="Chat",
            navigation=False,
            header_badge=badge("FocusStatus", "已完成", "Success"),
            actions=[
                button("OpenInspector", "详情", width=80),
                button("ExitFocus", "退出专注", width=104),
            ],
        ),
        shell(
            "Details",
            [
                box(
                    "SplitWorkspace",
                    [
                        box(
                            "ChatWorkspace", conversation(), width="fill", height="fill"
                        ),
                        panel(),
                    ],
                    "row",
                    width="fill",
                    height="fill",
                )
            ],
            title="规划动态内容界面",
            module="Chat",
            header_badge=badge("DetailsStatus", "已完成", "Success"),
            actions=[button("OpenInspector", "详情", width=80)],
        ),
    ]
    for page, waiting in [("Running", False), ("ApprovalPending", True)]:
        attention = box(
            "PersistentAttention",
            [
                row(
                    "AttentionRow",
                    badge(
                        "AttentionStatus",
                        "等待审批" if waiting else "正在执行",
                        "Warning" if waiting else "Info",
                    ),
                    text(
                        "AttentionSummary",
                        (
                            "文件写入需要你的决定。"
                            if waiting
                            else "正在处理当前任务，可查看活动或停止。"
                        ),
                        "body",
                        width="fill",
                    ),
                    button(
                        "ReviewApproval" if waiting else "OpenInspector",
                        "审阅操作" if waiting else "运行详情",
                        "Primary",
                        width=112,
                    ),
                )
            ],
            width="fill",
            padding=[12, 24],
            fill="{color.warningBg}" if waiting else "{color.selected}",
        )
        defs.append(
            shell(
                page,
                [attention, *conversation("waiting" if waiting else "running")],
                title="规划动态内容界面",
                module="Chat",
                header_badge=badge(
                    "ExecutionStatus",
                    "等待审批" if waiting else "执行中",
                    "Warning" if waiting else "Info",
                ),
                actions=[button("StopTask", "停止任务", "Danger", width=104)],
            )
        )
    defs.append(
        shell(
            "AnsweredWaits",
            [
                body(
                    [
                        heading(
                            "AnsweredHeading",
                            "回答已保存",
                            "任务仍然暂停。只有明确继续才恢复原任务。",
                        ),
                        card(
                            "AnsweredCard",
                            [
                                row(
                                    "AnsweredStatus",
                                    text(
                                        "AnsweredTask",
                                        "规划动态内容界面",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("SavedStatus", "已保存 · 未继续", "Warning"),
                                ),
                                text(
                                    "SavedAnswer",
                                    "已保存示例回答，待继续原任务。",
                                    "body",
                                    width="fill",
                                ),
                                row(
                                    "AnsweredActions",
                                    button("EditAnswer", "修改回答", width=104),
                                    button(
                                        "ResumeOriginal",
                                        "继续原任务",
                                        "Primary",
                                        width=120,
                                    ),
                                ),
                            ],
                        ),
                    ]
                )
            ],
            active="NavWaits",
            title="等待任务",
            module="Operations",
            header_badge=badge("SavedWaitCount", "1 个待继续", "Warning"),
        )
    )
    for name, failed in [("Stopped", False), ("Error", True)]:
        notice = box(
            "ExecutionFeedback",
            (
                [
                    text(
                        "ExecutionTitle",
                        "本轮执行失败" if failed else "本轮已停止",
                        "body",
                        "danger" if failed else "muted",
                        True,
                        width="fill",
                    ),
                    text(
                        "ExecutionDescription",
                        (
                            "连接失败。已有记录保留，你可以检查连接后重试。"
                            if failed
                            else "已有记录保留。可以编辑输入后开始新的任务。"
                        ),
                        "caption",
                        "muted",
                        width="fill",
                    ),
                    button("RetryTask", "重试本轮", "Primary", width=112),
                ]
                if failed
                else [
                    text(
                        "ExecutionTitle",
                        "本轮已停止",
                        "body",
                        strong=True,
                        width="fill",
                    ),
                    text(
                        "ExecutionDescription",
                        "已有记录保留。可以编辑输入后开始新的任务。",
                        "caption",
                        "muted",
                        width="fill",
                    ),
                ]
            ),
            width="fill",
            padding=[12, 24],
            gap=8,
            fill="{color.dangerBg}" if failed else "{color.subtle}",
        )
        if failed:
            notice = box(
                "ExecutionFeedback",
                [
                    row(
                        "ErrorSummary",
                        text(
                            "ExecutionDescription",
                            "连接失败。已有记录保留，检查连接后可重试。",
                            "body",
                            "danger",
                            width="fill",
                        ),
                        button("RetryTask", "重试本轮", "Primary", width=112),
                    )
                ],
                width="fill",
                padding=[8, 24],
                fill="{color.dangerBg}",
            )
        defs.append(
            shell(
                name,
                [notice, *conversation()],
                title="规划动态内容界面",
                module="Chat",
                header_badge=badge(
                    "FinalStatus",
                    "执行失败" if failed else "已停止",
                    "Danger" if failed else "Neutral",
                ),
            )
        )
    for d in defs:
        if only is None or d["page"] == only:
            cli("page", "draw", PRODUCT, intent=d, label="state-" + d["page"])


def settings():
    defs = [
        shell(
            "Gateway",
            [
                settings_body(
                    "Gateway",
                    [
                        heading(
                            "GatewayHeading",
                            "Gateway 服务",
                            "本机对外 API 服务；与当前会话使用的模型连接分开管理。",
                        ),
                        card(
                            "GatewayService",
                            [
                                row(
                                    "GatewayStatusRow",
                                    text(
                                        "GatewayServiceName",
                                        "本机 Gateway",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("GatewayStatus", "未启动"),
                                ),
                                row(
                                    "GatewayBindings",
                                    field("GatewayAddress", "监听地址", "127.0.0.1"),
                                    field("GatewayPort", "端口", "8080"),
                                ),
                                field("GatewayRoute", "上游路由", "项目默认连接"),
                                text(
                                    "GatewayProtocols",
                                    "协议：OpenAI Chat / Responses、Anthropic、Gemini",
                                    "caption",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "GatewayActions",
                                    button("SaveGateway", "保存配置", width=112),
                                    button(
                                        "StartGateway",
                                        "审阅并启动",
                                        "Primary",
                                        width=128,
                                    ),
                                ),
                            ],
                        ),
                        card(
                            "GatewayAccess",
                            [
                                text(
                                    "GatewayCredentials",
                                    "访问凭据",
                                    "body",
                                    strong=True,
                                ),
                                text(
                                    "GatewaySecret",
                                    "仅展示凭据引用和状态。新建时一次性提供密钥，已有密钥不显示原文。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                button("GatewayLogs", "查看请求记录", width=128),
                            ],
                        ),
                    ],
                )
            ],
            active="NavSettings",
            title="设置",
            module="Settings",
        ),
        shell(
            "Advanced",
            [
                settings_body(
                    "Advanced",
                    [
                        heading(
                            "AdvancedHeading",
                            "高级与诊断",
                            "按服务查看状态、访问范围、日志和错误。",
                        ),
                        card(
                            "RuntimeServices",
                            [
                                setting(
                                    "MediaProcessing",
                                    "媒体处理",
                                    "显式选择派生内容；原件、处理和投递记录分别保留。",
                                ),
                                setting(
                                    "Hooks",
                                    "项目 Hooks",
                                    "执行命令前展示来源和范围，保存不会立即执行。",
                                ),
                                row(
                                    "BrowserEnvironment",
                                    text(
                                        "BrowserLabel",
                                        "浏览器环境",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("BrowserAvailable", "按需配置"),
                                    button("BrowserInspect", "检查配置", width=104),
                                ),
                                row(
                                    "ScriptRuntime",
                                    text(
                                        "ScriptLabel",
                                        "AEP 脚本运行时",
                                        "body",
                                        strong=True,
                                        width="fill",
                                    ),
                                    badge("ScriptAccess", "宿主访问", "Warning"),
                                    button("ScriptInspect", "查看范围", width=104),
                                ),
                            ],
                        ),
                        card(
                            "Diagnostics",
                            [
                                text("DiagnosticTitle", "诊断", "body", strong=True),
                                text(
                                    "DiagnosticInfo",
                                    "仅检索当前范围的脱敏记录；导出前显示包含的服务和会话。",
                                    "body",
                                    "muted",
                                    width="fill",
                                ),
                                row(
                                    "DiagnosticActions",
                                    button("InspectLogs", "查看诊断", width=112),
                                    button("ExportDiagnostic", "审阅并导出", width=128),
                                ),
                                use(
                                    "DiagnosticEmpty",
                                    "Feedback",
                                    props={"State": "Empty"},
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
        ),
    ]
    for d in defs:
        cli("page", "draw", PRODUCT, intent=d, label="page-Settings-" + d["page"])


def questions(only=None):
    for state in ["Question", "QuestionSelected", "QuestionSelectedB", "QuestionOther"]:
        if only is not None and state != only:
            continue
        selected = state != "Question"
        other = state == "QuestionOther"
        d = dict(
            page=state,
            module="Overlay",
            platform="desktop",
            width=560,
            height=590,
            layout="column",
            fill="{color.surface}",
            padding=24,
            gap=20,
            children=[
                heading(
                    "OverlayHeading",
                    "需要你的选择",
                    "当前会话 · 单选 · 问题 1 / 1",
                    [button("CloseOverlay", "关闭", "Ghost", width=72)],
                ),
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
                        use(
                            "OptionA",
                            "ChoiceOption",
                            "原生外壳与完整 HTML 工作区",
                            {
                                "Kind": "Single",
                                "State": (
                                    "Selected"
                                    if state == "QuestionSelected"
                                    else "Default"
                                ),
                            },
                            width="fill",
                        ),
                        use(
                            "OptionB",
                            "ChoiceOption",
                            "先保持完整自绘界面",
                            {
                                "Kind": "Single",
                                "State": (
                                    "Selected"
                                    if state == "QuestionSelectedB"
                                    else "Default"
                                ),
                            },
                            width="fill",
                        ),
                        use(
                            "OptionOther",
                            "ChoiceOption",
                            "Other · 自定义答案",
                            {
                                "Kind": "Single",
                                "State": "Selected" if other else "Default",
                            },
                            width="fill",
                        ),
                    ],
                    gap=12,
                    width="fill",
                ),
                use(
                    "OtherText",
                    "TextField",
                    "先实现对话与配置页面。" if other else "选择 Other 后填写",
                    {"State": "Focused" if other else "Disabled"},
                    width="fill",
                ),
                text(
                    "QuestionHelp",
                    "提交只保存回答，任务仍暂停。关闭面板会保留待办。",
                    "body",
                    "muted",
                    width="fill",
                ),
                row(
                    "QuestionActions",
                    button("QuestionCancel", "稍后回答", width=112),
                    button(
                        "QuestionSubmit",
                        "保存回答",
                        "Primary",
                        "Default" if selected else "Disabled",
                        width=112,
                    ),
                ),
            ],
        )
        cli("page", "draw", PRODUCT, intent=d, label="question-state-" + state)


if __name__ == "__main__":
    import sys

    actions = {
        "foundation": foundation,
        "layouts": layouts,
        "settings": settings,
        "questions": questions,
    }
    for stage in sys.argv[1:]:
        name, _, target = stage.partition(":")
        if target:
            actions[name](target)
        else:
            actions[name]()
