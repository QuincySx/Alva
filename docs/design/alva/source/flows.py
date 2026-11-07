# INPUT: Named Product pages and elements authored through the SmallPen CLI
# OUTPUT: Same-canvas navigation and explicit review flows, without live business actions
# POS: Prototype wiring; approvals and answers here are illustrative transitions only.
import hashlib

from author import PRODUCT, cli

PAGES = {
    "Overview": "Workspace / Overview",
    "Conversation": "Chat / Conversation",
    "WaitingQuestion": "Chat / WaitingQuestion",
    "Focus": "Chat / Focus",
    "Details": "Chat / Details",
    "Running": "Chat / Running",
    "ApprovalPending": "Chat / ApprovalPending",
    "Stopped": "Chat / Stopped",
    "Error": "Chat / Error",
    "Plugins": "Capabilities / Plugins",
    "Library": "Knowledge / Library",
    "Team": "Operations / Team",
    "Automations": "Operations / Automations",
    "Waits": "Operations / Waits",
    "AnsweredWaits": "Operations / AnsweredWaits",
    "General": "Settings / General",
    "Routes": "Settings / Routes",
    "Capabilities": "Settings / Capabilities",
    "Rules": "Settings / Rules",
    "Storage": "Settings / Storage",
    "Security": "Settings / Security",
    "Gateway": "Settings / Gateway",
    "Advanced": "Settings / Advanced",
    "IntegrationTests": "Developer / IntegrationTests",
    "Evaluation": "Developer / Evaluation",
}
OVERLAYS = [
    "Question",
    "QuestionSelected",
    "QuestionSelectedB",
    "QuestionOther",
    "Approval",
    "Inspector",
    "CommandPalette",
    "NewSession",
    "Artifact",
]


def definitions():
    links = []

    def add(source, element, target=None, action="navigate"):
        links.append(
            {
                "fromPage": PAGES.get(source, source),
                "element": element,
                "toPage": PAGES.get(target, target),
                "action": action,
                "on": "click",
            }
        )

    nav = {
        "NavConversation": "Conversation",
        "NavPlugins": "Plugins",
        "NavLibrary": "Library",
        "NavTeam": "Team",
        "NavAutomations": "Automations",
        "NavWaits": "Waits",
        "NavDeveloper": "IntegrationTests",
        "NavSettings": "General",
    }
    for name in PAGES:
        if name == "Focus":
            continue
        for element, target in nav.items():
            add(name, element, target)
        for element in ["NewConversation", "NewConversationIcon"]:
            add(name, element, "Overlay / NewSession", "overlay")
    settings = [
        "General",
        "Routes",
        "Capabilities",
        "Rules",
        "Storage",
        "Security",
        "Gateway",
        "Advanced",
    ]
    for name in settings:
        for target in settings:
            add(name, "Settings" + target, target)
    for name in ["IntegrationTests", "Evaluation"]:
        add(name, "IntegrationTab", "IntegrationTests")
        add(name, "EvaluationTab", "Evaluation")
    for element in ["CreateProject", "CreatePrivate"]:
        add("Overview", element, "Overlay / NewSession", "overlay")
    add("Overview", "OpenRecent", "Conversation")
    add("Overview", "OverviewSettings", "General")
    add("Conversation", "ToggleSidebar", "Focus")
    add("Focus", "ToggleSidebar", "Conversation")
    add("Focus", "ExitFocus", "Conversation")
    add("Details", "CloseInspector", "Conversation")
    for name in [
        "Conversation",
        "WaitingQuestion",
        "Focus",
        "Details",
        "Running",
        "ApprovalPending",
        "Stopped",
        "Error",
    ]:
        add(name, "ExpandArtifact", "Overlay / Artifact", "overlay")
    for name in ["Conversation", "Focus", "Details", "Running"]:
        add(name, "OpenInspector", "Overlay / Inspector", "overlay")
    add("Conversation", "CommandPalette", "Overlay / CommandPalette", "overlay")
    add("Conversation", "ConversationSettings", "Routes")
    add("Conversation", "Send", "Running")
    for source, element in [
        ("WaitingQuestion", "AnswerQuestion"),
        ("Waits", "WaitOpenQuestion"),
        ("Waits", "WaitSave"),
        ("AnsweredWaits", "EditAnswer"),
    ]:
        add(source, element, "Overlay / Question", "overlay")
    add("ApprovalPending", "ReviewApproval", "Overlay / Approval", "overlay")
    add("Security", "OpenApprovalDemo", "Overlay / Approval", "overlay")
    for source, element in [
        ("Running", "StopTask"),
        ("ApprovalPending", "StopTask"),
        ("WaitingQuestion", "StopWaiting"),
    ]:
        add(source, element, "Stopped")
    add("AnsweredWaits", "ResumeOriginal", "Running")
    add("Error", "RetryTask", "Running")
    add("Capabilities", "ManagePlugins", "Plugins")
    add("Plugins", "PluginTwoPreview", "Overlay / Artifact", "overlay")
    add("Library", "MemorySettings", "Storage")
    for name in OVERLAYS:
        add("Overlay / " + name, "CloseOverlay", action="back")
    for element, target in [
        ("CommandGoChat", "Conversation"),
        ("CommandGoPlugins", "Plugins"),
        ("CommandGoSettings", "General"),
        ("CommandGoDeveloper", "IntegrationTests"),
    ]:
        add("Overlay / CommandPalette", element, target)
    for name in ["Question", "QuestionSelected", "QuestionSelectedB", "QuestionOther"]:
        for element, state in [
            ("OptionA", "QuestionSelected"),
            ("OptionB", "QuestionSelectedB"),
            ("OptionOther", "QuestionOther"),
        ]:
            add("Overlay / " + name, element, "Overlay / " + state)
        add("Overlay / " + name, "QuestionCancel", action="back")
        if name != "Question":
            add("Overlay / " + name, "QuestionSubmit", "AnsweredWaits")
    add("Overlay / NewSession", "NewSessionCancel", action="back")
    add("Overlay / NewSession", "NewSessionCreate", "Conversation")
    add("Overlay / Artifact", "ArtifactBack", action="back")
    # These transitions illustrate different outcomes, not real authorization.
    add("Overlay / Approval", "ApprovalAllow", "Running")
    add("Overlay / Approval", "ApprovalDeny", "Stopped")
    return links


def connect(start=0):
    for link in definitions()[start:]:
        identity = hashlib.sha256(repr(link).encode()).hexdigest()[:24]
        args = [
            "flow",
            "link",
            PRODUCT,
            "--from",
            link["fromPage"] + " / " + link["element"],
            "--action",
            "navigate" if link["action"] == "overlay" else link["action"],
            "--on",
            link["on"],
            "--batch-id",
            "alva-prototype-flow-" + identity,
        ]
        if link["toPage"]:
            args += ["--to", link["toPage"]]
        cli(*args, label="prototype-link")
    cli(
        "flow",
        "start",
        PRODUCT,
        "--page",
        PAGES["Overview"],
        "--batch-id",
        "alva-workspace-start",
        label="prototype-start",
    )


if __name__ == "__main__":
    connect()
