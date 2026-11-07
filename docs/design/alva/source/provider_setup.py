# INPUT: Reviewed SmallPen workspace, explicit connection/model setup brief and native provider endpoint contract
# OUTPUT: Provider manager, three-stage connection wizard and shared add/edit model form wireframes
# POS: Product configuration design only; does not discover credentials, query a provider or save runtime configuration.
import argparse
import hashlib
import json
from pathlib import Path

from author import PRODUCT, ROOT, cli
from workbench import CANVAS, action, box, row, t

PAGES = ["08 选择供应商", "09 配置连接", "10 选择模型", "11 模型配置"]


def field(name, label, value, hint=None, width="fill"):
    return box(name, [t(name+"Label", label, 12, muted=True),
        box(name+"Input", [t(name+"Value", value, 13)], width="fill", padding=[10,12],
            fill="#FFFFFF", stroke="#CFCFCF", radius=5),
        *([t(name+"Hint", hint, 11, muted=True)] if hint else []),
    ], width=width, gap=6)


def connection_item(key, name, hint, selected=False):
    return box(key, [t(key+"Title", name, 14), t(key+"Hint", hint, 11, muted=True)],
        width="fill", padding=[12,10], gap=6, radius=6,
        **({"fill":"#E7E7E7", "stroke":"#D2D2D2"} if selected else {}))


def model_row(key, name, detail, enabled=True, purpose="对话", selectable=False):
    return row(key, [
        *([t(key+"Check", "[x]" if enabled else "[ ]", 14, width=28)] if selectable else []),
        box(key+"Labels", [t(key+"Name", name, 14), t(key+"Detail", detail, 11, muted=True)], width="fill", gap=6),
        t(key+"Purpose", purpose, 11, muted=True, width=56),
        *([] if selectable else [action(key+"Settings", "设置", quiet=True),
            action(key+"Enabled", "已启用" if enabled else "未启用", quiet=not enabled)]),
    ], height=65, padding=[8,12], gap=8, stroke={"color":"#E3E3E3","width":[0,0,1,0]})


def settings_main():
    return box("SettingsMain", [
        row("SettingsPageHeader", [t("SettingsPageTitle", "模型与连接", 25), action("AddConnection", "+  添加供应商")]),
        t("SettingsPageDescription", "管理模型来源和可用模型。示例数据 · 应用设置", 13, muted=True),
        row("ProviderManager", [
            box("ConnectionSidebar", [
                t("ConnectionsTitle", "我的连接", 12, muted=True),
                connection_item("OpenAIConnection", "OpenAI · 主连接", "API Key · 就绪 · 2 个模型启用", True),
                connection_item("AnthropicConnection", "Anthropic · 工作连接", "API Key · 就绪 · 2 个模型启用"),
                connection_item("GeminiConnection", "Google Gemini", "尚未连接"),
                connection_item("LocalConnection", "本地模型服务", "尚未连接"),
                t("LocalAccountsTitle", "本机 Agent 账户", 12, muted=True),
                action("ConnectLocalAccount", "+  连接本机账户", width="fill"),
                t("LocalAccountHint", "主动检查登录后选择账户与 Agent", 11, muted=True),
                box("ConnectionListSpace", width="fill", height="fill"),
                action("AddProviderBottom", "+  添加供应商", width="fill"),
            ], width=220, height="fill", padding=16, gap=10, fill="#F6F6F6"),
            box("ProviderDetail", [
                row("ProviderDetailHeader", [box("SelectedProvider", [t("SelectedProviderName", "OpenAI · 主连接", 19),
                    t("SelectedProviderStatus", "API Key · 就绪 · ALVA", 12, muted=True)], width="fill", gap=7),
                    action("EditConnection", "连接设置")]),
                t("ProviderEndpointSummary", "Responses · 请求端点已配置 · 密钥由宿主管理", 11, muted=True),
                row("ModelsToolbar", [t("ModelsTitle", "模型 · 已启用 2 个", 15), action("RefreshModels", "刷新目录", quiet=True), action("AddModel", "+  添加模型")]),
                row("ModelsFilters", [action("FilterConversation", "对话"), action("FilterImageGeneration", "图像生成", quiet=True),
                    box("SearchModels", [t("SearchModelsText", "搜索模型…", 12, muted=True)], width="fill", padding=[9,12], stroke="#D5D5D5", radius=6)]),
                box("ProviderModels", [
                    model_row("ManagedGPT", "GPT", "目录模型 · 视觉输入 · 工具调用 · 示例"),
                    model_row("ManagedGPTMini", "GPT Mini", "目录模型 · 文本 · 工具调用 · 示例"),
                    model_row("ManagedCustom", "我的自定义模型", "手动添加 · 能力待确认", False),
                ], width="fill", gap=0),
                t("ModelAvailabilityHint", "启用后出现在对话选择器；是否可调用取决于账户、Agent 与连接状态。", 12, muted=True),
                t("ImagePurposeHint", "图像生成单独管理；支持视觉输入的对话模型仍列在对话中。", 12, muted=True),
                box("ProviderDetailSpace", width="fill", height="fill"),
                t("CatalogRefreshHint", "目录刷新保留启用选择与手动模型；无法获取目录时可手动添加。", 11, muted=True),
            ], width="fill", height="fill", padding=22, gap=16),
        ], height="fill", gap=0, stroke="#D5D5D5", radius=8),
        row("DefaultAgentModel", [t("DefaultAgentModelLabel", "应用默认模型与 Agent", 13),
            action("DefaultAgentModelValue", "GPT · ALVA v", width=240)]),
        t("DefaultsHint", "项目和会话可覆盖默认值；当前正在执行的轮次保持原配置。", 11, muted=True),
    ], width="fill", height="fill", padding=32, gap=16)


def wizard(index, body, footer):
    steps = ["1  选择供应商", "2  配置连接", "3  选择模型"]
    return dict(page=PAGES[index], module="选择器", canvas=CANVAS, platform="desktop",
        width=920, height=770, layout="column", padding=24, fill="#ECECEC", children=[
            box("ProviderWizard", [
                row("WizardHeader", [t("WizardTitle", "添加供应商", 21), action("WizardCancelTop", "x", quiet=True)]),
                row("WizardSteps", [box("WizardStep"+str(i), [t("WizardStep"+str(i)+"Text", v, 13,
                    muted=i!=index)], width="fill", padding=10, radius=5,
                    fill="#E7E7E7" if i==index else "#F7F7F7") for i,v in enumerate(steps)], gap=8),
                body,
                row("WizardFooter", footer, height=48, gap=10,
                    stroke={"color":"#DDDDDD","width":[1,0,0,0]}),
            ], width="fill", height="fill", padding=24, gap=18, fill="#FFFFFF", stroke="#C7C7C7", radius=10)
        ])


def select_provider():
    def choice(key, label, hint, tag):
        return row(key, [box(key+"Mark", [t(key+"MarkText", tag, 13, width="hug")], width=36, height=36,
            align="center", justify="center", stroke="#D1D1D1", radius=7),
            box(key+"Labels", [t(key+"Name", label, 14), t(key+"Hint", hint, 11, muted=True)], width="fill", gap=5),
            t(key+"Arrow", ">", 15, muted=True, width=12)], height=62, padding=[6,8],
            stroke={"color":"#E6E6E6","width":[0,0,1,0]})
    return wizard(0, box("ProviderChoices", [
        field("ProviderSearch", "选择模型来源", "搜索供应商…"),
        t("PresetProvidersTitle", "API 服务", 12, muted=True),
        choice("ChooseOpenAI", "OpenAI", "使用自己的 API Key", "O"),
        choice("ChooseAnthropic", "Anthropic", "使用自己的 API Key", "A"),
        choice("ChooseGemini", "Google Gemini", "使用自己的 API Key", "G"),
        choice("ChooseLocal", "本地模型服务", "连接已经运行的 HTTP 服务", "L"),
        choice("ChooseLocalAccount", "本机 Agent 账户", "显式检查登录 · 仅用于对应 Agent", "C"),
        box("ChooseCustom", [t("ChooseCustomTitle", "+  自定义端点", 15),
            t("ChooseCustomHint", "接入兼容 OpenAI、Anthropic 或 Gemini 协议的服务", 12, muted=True)],
            width="fill", padding=14, gap=7, fill="#F4F4F4", stroke="#D1D1D1", radius=7),
    ], width="fill", height="fill", gap=8), [t("WizardScope", "不会自动读取本机凭据或启动本地模型", 11, muted=True), action("WizardCancel", "取消")])


def configure_connection():
    return wizard(1, box("ConnectionForm", [
        row("ConnectionIdentity", [field("ConnectionName", "连接名称", "我的模型服务"),
            field("ConnectionAuth", "认证方式", "API Key  v")]),
        field("ConnectionProtocol", "API 协议", "OpenAI Responses  v", "可选 Chat Completions / Responses / Anthropic / Gemini"),
        field("ConnectionEndpoint", "请求端点 · 完整 API 地址", "https://api.example.com/v1/responses",
              "使用完整请求路径；本地地址允许 HTTP，远程服务使用 HTTPS。"),
        field("ConnectionApiKey", "API Key", "••••••••••••••••", "密钥安全保存，不会出现在对话和日志中。"),
        action("ConnectionAdvanced", ">  高级设置 · 自定义请求头", quiet=True),
        box("ConnectionCheckStatus", [t("ConnectionCheckTitle", "等待测试", 13),
            t("ConnectionCheckHint", "测试通过不代表所有模型可用；下一步选择或手动添加模型。", 12, muted=True)],
            width="fill", padding=14, gap=6, fill="#F4F4F4", radius=6),
        box("ConnectionFormSpace", width="fill", height="fill"),
    ], width="fill", height="fill", gap=16), [action("WizardBack", "上一步", quiet=True),
        box("WizardFooterSpace", width="fill", height=1), action("WizardCancel", "取消"), action("WizardContinue", "测试并继续", primary=True)])


def select_models():
    return wizard(2, box("NewConnectionModels", [
        row("NewConnectionSummary", [box("NewConnectionLabels", [t("NewConnectionName", "我的模型服务", 16),
            t("NewConnectionProtocol", "Responses · 连接测试通过（示例状态）", 12, muted=True)], width="fill", gap=6),
            action("WizardRefreshCatalog", "刷新目录", quiet=True)]),
        row("WizardModelToolbar", [field("WizardModelSearch", "选择后启用", "搜索模型…"), action("WizardManualModel", "+  手动添加模型")]),
        row("WizardSelectionTools", [action("WizardSelectAll", "全选", quiet=True), action("WizardSelectedOnly", "仅已选 2", quiet=True)]),
        model_row("WizardModelOne", "GPT", "model-id-1 · 来自服务目录 · 示例", True, selectable=True),
        model_row("WizardModelTwo", "GPT Mini", "model-id-2 · 来自服务目录 · 示例", True, selectable=True),
        model_row("WizardModelThree", "自定义模型", "exact-model-id · 手动添加 · 能力待确认", False, selectable=True),
        t("ManualCatalogFallback", "服务不提供目录？按服务文档填写准确的模型 ID，一样可以继续。", 12, muted=True),
        box("WizardModelsSpace", width="fill", height="fill"),
        t("EnableModelsHint", "完成后保存连接并启用已选模型；取消保留原有配置。", 12, muted=True),
    ], width="fill", height="fill", gap=12), [action("WizardBack", "上一步", quiet=True),
        box("WizardFooterSpace", width="fill", height=1), action("WizardCancel", "取消"), action("WizardFinish", "完成并启用 2 个模型", primary=True)])


def model_form(prefix, editing):
    def f(key, *args, **kwargs): return field(prefix+key, *args, **kwargs)
    return box(prefix+"ModelForm", [
        row(prefix+"Header", [t(prefix+"Title", "模型设置" if editing else "手动添加模型", 20), action(prefix+"Close", "x", quiet=True)]),
        t(prefix+"Origin", "OpenAI · 主连接" if editing else "添加到当前连接", 12, muted=True),
        f("ModelID", "模型 ID"+(" · 只读" if editing else " · 必填"), "model-id-1" if editing else "填写服务要求的准确 ID"),
        f("DisplayName", "显示名称", "GPT" if editing else "我的模型"),
        row(prefix+"PurposeRow", [f("Purpose", "模型用途", "对话  v"), f("Context", "Context 上限 · token", "128000" if editing else "未声明")]),
        f("Capabilities", "能力声明", "[x] 视觉输入    [x] 工具调用" if editing else "[ ] 视觉输入    [ ] 工具调用"),
        row(prefix+"EffortRow", [f("Reasoning", "思考等级", "跟随模型默认  v"), f("Visibility", "对话列表", "启用  v")]),
        t(prefix+"MetadataHint", "能力与上限按服务说明填写；修改声明不会让模型获得新能力。", 11, muted=True),
        t(prefix+"PricingHint", "价格：服务未提供；不估算、不展示虚构费用。", 11, muted=True),
        box(prefix+"Space", width="fill", height="fill"),
        row(prefix+"Footer", [action(prefix+"Cancel", "取消", quiet=True), box(prefix+"FooterSpace", width="fill", height=1),
            action(prefix+"Save", "保存设置" if editing else "添加到连接", primary=True)]),
    ], width="fill", height="fill", padding=22, gap=14, fill="#FFFFFF", stroke="#CFCFCF", radius=9)


def model_configuration():
    return dict(page=PAGES[3], module="选择器", canvas=CANVAS, platform="desktop", width=1080, height=740,
        layout="column", padding=20, gap=12, fill="#ECECEC", children=[
            t("ModelFormVariantsTitle", "同一模型表单 · 手动添加 / 已有模型设置", 14, muted=True),
            row("ModelFormVariants", [model_form("Add", False), model_form("Edit", True)], height="fill", gap=18),
        ])


def definitions():
    return [select_provider(), configure_connection(), select_models(), model_configuration()]


def connect():
    snapshot=cli("flow", "list", PRODUCT, label="provider-setup-read-links")
    if snapshot.get("resultFile"): snapshot=json.loads(Path(snapshot["resultFile"]["path"]).read_text())
    existing=snapshot.get("text", "")
    manager="框架 / 04 模型与连接"
    p=["选择器 / "+x for x in PAGES]
    links=[(manager,"AddConnection",p[0]),(manager,"AddProviderBottom",p[0]),
        (manager,"AddModel",p[3]),(manager,"ManagedGPTSettings",p[3]),
        (manager,"EditConnection",p[1]),(manager,"DefaultAgentModelValue","选择器 / 05 Agent 与模型"),
        (manager,"BackToWorkspace","框架 / 02 对话进行中"),(manager,"ActiveTask","框架 / 02 对话进行中"),
        (p[0],"ChooseCustom",p[1]),(p[0],"ChooseOpenAI",p[1]),
        (p[1],"WizardBack",p[0]),(p[1],"WizardContinue",p[2]),
        (p[2],"WizardBack",p[1]),(p[2],"WizardFinish",manager),
        *[(page,control,manager) for page in p[:3] for control in ("WizardCancel","WizardCancelTop")],
        *[(p[3],prefix+control,manager) for prefix in ("Add","Edit") for control in ("Close","Cancel","Save")]]
    for i,(source,element,target) in enumerate(links):
        if f"{source} (desktop) / {element} —click→ {target}" in existing: continue
        identity=hashlib.sha256((source+" / "+element+" -> "+target).encode()).hexdigest()[:16]
        cli("flow","link",PRODUCT,"--from",source+" / "+element,"--to",target,"--on","click",
            "--batch-id",f"alva-provider-setup-link-{identity}-v1",label="provider-setup-link")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--links",action="store_true")
    parser.add_argument("--since")
    args=parser.parse_args()
    if args.write or args.links:
        if not args.since: parser.error("Writing requires --since REVIEWED_REVISION")
        changes=cli("changes",PRODUCT,"--since",args.since,label="provider-setup-read-app-changes")
        if changes.get("changeCount",0): raise SystemExit("Inspect App changes before authoring.")
    if args.write:
        from workbench import screens
        for d in [screens()[3],*definitions()]:
            cli("page","draw",PRODUCT,intent=d,label="provider-setup-"+d["page"][:2])
    elif args.links: connect()
    else:
        for d in definitions():
            (ROOT/"source"/"intents"/("provider-setup-"+d["page"][:2]+".json")).write_text(json.dumps(d,ensure_ascii=False,indent=2)+"\n")
        print("Prepared provider setup definitions; no package changes.")
