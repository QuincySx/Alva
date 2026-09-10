// INPUT:  React local state, ALVA UI facade, and Lucide icons
// OUTPUT: Automations, extensions, settings, and search product pages
// POS:    Deterministic Hub product surfaces above the shared UI component system

import { useMemo, useState } from "react";
import {
  Blocks,
  Bot,
  CalendarClock,
  Check,
  Command,
  Download,
  FlaskConical,
  Gauge,
  GitBranch,
  Plus,
  Search,
  Settings2,
  Sparkles,
  Workflow,
} from "lucide-react";
import {
  Badge,
  Button,
  CardGrid,
  DialogContent,
  DialogRoot,
  DialogTrigger,
  EmptyState,
  FieldFrame,
  NavItem,
  Notice,
  PageHeader,
  ScrollRegion,
  SelectField,
  SettingsGroup,
  SettingsRow,
  Switch,
  StatusListItem,
  TabsList,
  TabsRoot,
  TabsTrigger,
  TextField,
} from "../ui";
import type {
  EngineTarget,
  ModelRoute,
  ProviderDiagnosticResult,
  ProviderProfile,
  SaveProviderProfile,
} from "../bridge";
import { useI18n, type LanguagePreference } from "../i18n";
import { ProviderSettings } from "./provider-settings";

export type HubPage = "conversation" | "automations" | "extensions" | "settings" | "search";

interface AutomationItem {
  id: string;
  title: string;
  schedule: string;
  prompt: string;
  enabled: boolean;
}


export function AutomationsPage() {
  const { t, locale } = useI18n();
  // R20: 自动化调度后端尚未开放——为避免假成功，创建/启用入口禁用，
  // 模板仅作预览；页面诚实说明当前状态。
  const automationTemplates = [
    ["nightly", t("automation.templateNightly"), t("automation.daily"), t("automation.templateNightlyPrompt")],
    ["review", t("automation.templateReview"), t("automation.weekdays"), t("automation.templateReviewPrompt")],
    ["radar", t("automation.templateRadar"), t("automation.weekly"), t("automation.templateRadarPrompt")],
  ] as const;
  const [automations] = useState<AutomationItem[]>([]);

  return (
    <ScrollRegion className="hub-product-page">
      <PageHeader
        actions={
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Badge tone="warning">
              {locale === "zh-CN" ? "调度尚未开放" : "Scheduling not yet available"}
            </Badge>
            <Button
              disabled
              aria-label={locale === "zh-CN" ? "自动化功能尚未开放" : "Automations not yet available"}
              leadingIcon={<Plus aria-hidden="true" size={15} />}
              variant="primary"
            >
              {t("automation.new")}
            </Button>
          </div>
        }
        description={t("automation.description")}
        meta={
          automations.length > 0
            ? t("automation.configured", { count: automations.length })
            : locale === "zh-CN"
              ? "尚未开放：调度后端仍在开发中"
              : "Not yet available: scheduling backend is under development"
        }
        title={t("nav.automations")}
      />
      <EmptyState
        icon={<Workflow aria-hidden="true" size={18} />}
        title={locale === "zh-CN" ? "自动化尚未开放" : "Automations not yet available"}
        description={
          locale === "zh-CN"
            ? "定时调度后端仍在开发中；下方模板仅供预览，创建入口已禁用。"
            : "The scheduling backend is still in development; templates below are previews and creation is disabled."
        }
      />
      <section className="hub-product-section">
        <h3>
          {t("automation.templates")}{" "}
          <Badge tone="warning">
            {locale === "zh-CN" ? "预览（暂不可用）" : "Preview (unavailable)"}
          </Badge>
        </h3>
        <CardGrid>
          {automationTemplates.map((template) => (
            <StatusListItem
              className="hub-template-card"
              description={template[3]}
              heading={template[1]}
              key={template[0]}
              leading={<CalendarClock aria-hidden="true" size={18} />}
              trailing={template[2]}
            />
          ))}
        </CardGrid>
      </section>
    </ScrollRegion>
  );
}

export function ExtensionsPage({ onStartChat }: { onStartChat: () => void }) {
  const { t, locale } = useI18n();
  // R22: 生产目录只反映真实 registry——当前没有任何已安装扩展；样例一律
  // installed: false，安装/注册入口禁用并说明。
  const extensions = [
    { id: "github", name: "GitHub", description: t("extension.githubDescription"), icon: GitBranch, installed: false, kind: "plugin" },
    { id: "maker", name: "TapTap Maker", description: t("extension.makerDescription"), icon: Sparkles, installed: false, kind: "plugin" },
    { id: "review", name: t("extension.review"), description: t("extension.reviewDescription"), icon: Check, installed: false, kind: "skill" },
    { id: "design", name: t("extension.design"), description: t("extension.designDescription"), icon: Blocks, installed: false, kind: "skill" },
  ] as const;
  const [tab, setTab] = useState("plugin");
  const [query, setQuery] = useState("");
  const [installed] = useState<Set<string>>(
    () => new Set(extensions.filter((extension) => extension.installed).map((extension) => extension.id)),
  );
  const [selectedExtension, setSelectedExtension] = useState<(typeof extensions)[number] | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [extensionName, setExtensionName] = useState("");
  const visible = extensions.filter(
    (extension) =>
      extension.kind === tab &&
      `${extension.name} ${extension.description}`.toLowerCase().includes(query.trim().toLowerCase()),
  );

  return (
    <ScrollRegion className="hub-product-page">
      <PageHeader
        actions={
          <DialogRoot open={addOpen} onOpenChange={setAddOpen}>
            <DialogTrigger asChild>
              <Button leadingIcon={<Plus aria-hidden="true" size={15} />}>{t("extension.add")}</Button>
            </DialogTrigger>
            <DialogContent
              description={
                locale === "zh-CN"
                  ? "扩展注册后端尚未开放：此处暂不能登记新扩展。"
                  : "Extension registration backend is not yet available; nothing can be registered here yet."
              }
              title={t("extension.add")}
            >
              <div className="hub-form-stack">
                <FieldFrame label={t("extension.nameOrSource")}>
                  <TextField
                    aria-label={t("extension.nameOrSourceAria")}
                    onChange={(event) => setExtensionName(event.target.value)}
                    placeholder={t("extension.sourcePlaceholder")}
                    value={extensionName}
                  />
                </FieldFrame>
                <div className="hub-dialog-actions">
                  <Button onClick={() => setAddOpen(false)} variant="ghost">{t("common.cancel")}</Button>
                  <Button disabled variant="primary">
                    {t("extension.register")}
                  </Button>
                </div>
              </div>
            </DialogContent>
          </DialogRoot>
        }
        description={t("extension.description")}
        title={t("extension.title")}
      />
      <div className="hub-product-toolbar">
        <TabsRoot onValueChange={setTab} value={tab}>
          <TabsList aria-label={t("extension.type")}>
            <TabsTrigger value="plugin">{t("extension.plugins")}</TabsTrigger>
            <TabsTrigger value="skill">{t("extension.skills")}</TabsTrigger>
          </TabsList>
        </TabsRoot>
        <label className="hub-search-field">
          <Search aria-hidden="true" size={16} />
          <TextField aria-label={t("extension.search")} placeholder={t("extension.searchPlaceholder")} value={query} onChange={(event) => setQuery(event.target.value)} />
        </label>
      </div>
      {visible.length === 0 ? (
        <EmptyState title={t("extension.noMatches")} description={t("extension.noMatchesDescription")} />
      ) : (
        <CardGrid>
          {visible.map((extension) => {
            const Icon = extension.icon;
            const isInstalled = installed.has(extension.id);
            return (
              <article className="hub-extension-card" key={extension.id}>
                <div className="hub-extension-card__heading">
                  <span><Icon aria-hidden="true" size={18} /></span>
                  <div><strong>{extension.name}</strong><small>{extension.kind === "plugin" ? t("extension.plugin") : t("extension.skill")}</small></div>
                  <Badge tone={isInstalled ? "success" : "neutral"}>{isInstalled ? t("common.installed") : t("common.available")}</Badge>
                </div>
                <p>{extension.description}</p>
                <div className="hub-extension-card__actions">
                  {isInstalled ? (
                    <Button onClick={onStartChat} size="small" variant="ghost">{t("extension.useInChat")}</Button>
                  ) : (
                    <Button
                      aria-label={
                        locale === "zh-CN"
                          ? "安装尚未支持：需要扩展注册表后端"
                          : "Install is not supported yet: requires an extension registry backend"
                      }
                      disabled
                      leadingIcon={<Download aria-hidden="true" size={14} />}
                      size="small"
                      title={
                        locale === "zh-CN"
                          ? "安装需要扩展注册表后端（尚未开放）"
                          : "Install requires an extension registry backend (not yet available)"
                      }
                    >
                      {t("extension.install")}
                    </Button>
                  )}
                  <Button onClick={() => setSelectedExtension(extension)} size="small" variant="ghost">{t("common.details")}</Button>
                </div>
              </article>
            );
          })}
        </CardGrid>
      )}
      <DialogRoot open={selectedExtension !== null} onOpenChange={(open) => { if (!open) setSelectedExtension(null); }}>
        <DialogContent
          description={selectedExtension?.description}
          title={selectedExtension?.name ?? t("extension.details")}
        >
          {selectedExtension ? (
            <SettingsGroup heading={t("extension.availability")}>
              <SettingsRow
                control={<Badge tone={installed.has(selectedExtension.id) ? "success" : "neutral"}>{installed.has(selectedExtension.id) ? t("common.installed") : t("common.available")}</Badge>}
                description={t("extension.activationDescription")}
                heading={selectedExtension.kind === "plugin" ? t("extension.plugin") : t("extension.skill")}
              />
            </SettingsGroup>
          ) : null}
        </DialogContent>
      </DialogRoot>
    </ScrollRegion>
  );
}

const settingsSections = [
  "general",
  "models",
  "tools",
  "remote",
  "im",
  "keybindings",
  "usage",
  "permissions",
  "experiments",
  "help",
] as const;

export function SettingsPage({
  defaultTarget,
  providers,
  providersLoading,
  onSaveProvider,
  onSetDefaultProvider,
  onDiscoverProviderModels,
  onTestProviderRoute,
  routes,
}: {
  defaultTarget: EngineTarget | null;
  providers: ProviderProfile[];
  providersLoading: boolean;
  onSaveProvider: (provider: SaveProviderProfile) => Promise<void>;
  onSetDefaultProvider: (target: EngineTarget) => Promise<void>;
  onDiscoverProviderModels: (providerId: string) => Promise<ProviderDiagnosticResult>;
  onTestProviderRoute: (target: EngineTarget) => Promise<ProviderDiagnosticResult>;
  routes: ModelRoute[];
}) {
  const { locale, preference, setPreference, t } = useI18n();
  const [section, setSection] = useState<(typeof settingsSections)[number]>("general");
  const [themeMode, setThemeMode] = useState<"dark" | "light" | "system">("dark");
  const [compact, setCompact] = useState(false);
  const [inspector, setInspector] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [workerLimit, setWorkerLimit] = useState("4");
  const [resourceTier, setResourceTier] = useState("balanced");
  const [gitSnapshot, setGitSnapshot] = useState(true);
  // R21: 只保留真实存在的权限档（Ask / Auto Edits）；"只读计划" 无等价
  // PermissionProfile，为避免假成功已移除。持久化接线开发中。
  const [permissionMode, setPermissionMode] = useState<"ask" | "accept_edits">("ask");

  // Profile Edit Dialog
  const [profileOpen, setProfileOpen] = useState(false);
  const [profileName, setProfileName] = useState("Wang (CN)");
  const [profileEmail, setProfileEmail] = useState("wang@alva.local");

  // Remote SSH Hosts state
  const [sshHosts, setSshHosts] = useState([
    { id: "aliyun-prod", name: "阿里云生产集群 (aliyun-prod)", host: "10.0.12.88", user: "deploy", port: 22, status: "connected" },
    { id: "gpu-dev", name: "本地 GPU 算力机 (rtx4090-dev)", host: "192.168.1.120", user: "dev", port: 22, status: "idle" },
  ]);
  const [addSshOpen, setAddSshOpen] = useState(false);
  const [newSshName, setNewSshName] = useState("");
  const [newSshHost, setNewSshHost] = useState("");
  const [newSshUser, setNewSshUser] = useState("root");

  // MCP Servers local state
  const [mcpServers, setMcpServers] = useState([
    { id: "chrome-devtools", name: "Chrome DevTools MCP", transport: "SSE", endpoint: "http://127.0.0.1:9222/sse", enabled: true },
    { id: "postgres-inspector", name: "PostgreSQL Studio", transport: "stdio", endpoint: "npx @modelcontextprotocol/server-postgres", enabled: false },
  ]);
  const [addMcpOpen, setAddMcpOpen] = useState(false);
  const [newMcpName, setNewMcpName] = useState("");
  const [newMcpUrl, setNewMcpUrl] = useState("");

  // Tools toggles
  const [toolFileIo, setToolFileIo] = useState(true);
  const [toolShell, setToolShell] = useState(true);
  const [toolWeb, setToolWeb] = useState(true);
  const [toolBrowser, setToolBrowser] = useState(true);
  const [toolGit, setToolGit] = useState(true);

  return (
    <div className="hub-settings-frame">
      <nav aria-label={t("settings.sections")} className="hub-settings-nav">
        {settingsSections.map((candidate) => (
          <NavItem
            active={section === candidate}
            key={candidate}
            label={
              candidate === "general"
                ? (locale === "zh-CN" ? "通用设置" : "General")
                : candidate === "models"
                  ? (locale === "zh-CN" ? "模型供应商" : "Models")
                  : candidate === "tools"
                    ? (locale === "zh-CN" ? "内置工具与 MCP" : "Tools & MCP")
                    : candidate === "remote"
                      ? (locale === "zh-CN" ? "远程 SSH 连接" : "Remote SSH")
                      : candidate === "im"
                        ? (locale === "zh-CN" ? "💬 IM 机器人" : "IM Gateways")
                        : candidate === "keybindings"
                          ? (locale === "zh-CN" ? "⌨️ 键盘快捷键" : "Keybindings")
                          : candidate === "usage"
                            ? (locale === "zh-CN" ? "💳 用量与计费" : "Usage & Quota")
                            : candidate === "permissions"
                              ? (locale === "zh-CN" ? "权限与安全" : "Permissions")
                              : candidate === "experiments"
                                ? (locale === "zh-CN" ? "🧪 实验性功能" : "Experiments")
                                : (locale === "zh-CN" ? "关于与更新" : "About & Help")
            }
            onClick={() => setSection(candidate)}
          />
        ))}
      </nav>
      <ScrollRegion className="hub-product-page hub-settings-page">
        <PageHeader
          description={
            section === "general"
              ? (locale === "zh-CN" ? "管理账户资料、界面外观与 Worker 协作策略" : "Manage account profile, appearance, and worker coordination")
              : section === "models"
                ? (locale === "zh-CN" ? "配置外部大模型供应商连接、API 密钥与默认路由" : "Configure external provider credentials and default routes")
                : section === "tools"
                  ? (locale === "zh-CN" ? "管理内置 Agent 工具箱与外部 MCP (Model Context Protocol) 服务" : "Manage built-in tools and MCP servers")
                  : section === "remote"
                    ? (locale === "zh-CN" ? "配置远程主机 SSH 免密登录与端侧执行沙箱" : "Manage remote SSH host connections and remote execution")
                    : section === "im"
                      ? (locale === "zh-CN" ? "配置微信、企业微信与飞书等即时通讯机器人连接" : "Configure instant messaging bots and webhook bridges")
                      : section === "keybindings"
                        ? (locale === "zh-CN" ? "查看与管理常用键盘快捷键" : "Inspect keyboard shortcuts and navigation hotkeys")
                        : section === "usage"
                          ? (locale === "zh-CN" ? "查看会话 Token 消耗、API 成本与用量统计" : "Inspect token usage, billing, and run statistics")
                          : section === "permissions"
                            ? (locale === "zh-CN" ? "管理 Agent 执行权限策略与敏感文件访问控制" : "Manage agent execution permissions and path filters")
                            : section === "experiments"
                              ? (locale === "zh-CN" ? "探索面向开发者和架构师的实验室功能" : "Explore developer diagnostics and experimental features")
                              : (locale === "zh-CN" ? "版本信息、更新检查与系统诊断报告" : "System version, update checks, and diagnostics")
          }
          title={
            section === "general"
              ? (locale === "zh-CN" ? "通用设置" : "General Settings")
              : section === "models"
                ? (locale === "zh-CN" ? "模型供应商与连接" : "Model Providers")
                : section === "tools"
                  ? (locale === "zh-CN" ? "内置工具与 MCP" : "Tools & MCP")
                  : section === "remote"
                    ? (locale === "zh-CN" ? "远程 SSH 主机" : "Remote SSH Connections")
                    : section === "im"
                      ? (locale === "zh-CN" ? "IM 机器人网关" : "IM Bot Bridges")
                      : section === "keybindings"
                        ? (locale === "zh-CN" ? "键盘快捷键" : "Keyboard Shortcuts")
                        : section === "usage"
                          ? (locale === "zh-CN" ? "用量与计费" : "Usage & Billing")
                          : section === "permissions"
                            ? (locale === "zh-CN" ? "权限与安全" : "Permissions & Security")
                            : section === "experiments"
                              ? (locale === "zh-CN" ? "实验功能" : "Experiments")
                              : (locale === "zh-CN" ? "关于 ALVA Hub" : "About ALVA Hub")
          }
        />

        {section === "general" ? (
          <>
            {/* Account Profile Card */}
            <div className="hub-profile-card">
              <div className="hub-profile-card__left">
                <div className="hub-profile-avatar">{profileName.trim().charAt(0).toUpperCase() || "A"}</div>
                <div className="hub-profile-details">
                  <h4>{profileName}</h4>
                  <p>{profileEmail}{locale === "zh-CN" ? " · 本地资料，未接入账号系统" : " · local profile, no account system"}</p>
                </div>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
                <DialogRoot open={profileOpen} onOpenChange={setProfileOpen}>
                  <DialogTrigger asChild>
                    <Button size="small" variant="ghost">
                      {locale === "zh-CN" ? "编辑资料" : "Edit Profile"}
                    </Button>
                  </DialogTrigger>
                  <DialogContent
                    title={locale === "zh-CN" ? "编辑个人资料" : "Edit Profile"}
                    description={locale === "zh-CN" ? "更新本地显示的账户名与联系邮箱" : "Update local user profile details"}
                  >
                    <div className="hub-form-stack" style={{ marginTop: "12px" }}>
                      <FieldFrame label={locale === "zh-CN" ? "显示名称" : "Display Name"}>
                        <TextField value={profileName} onChange={(e) => setProfileName(e.target.value)} />
                      </FieldFrame>
                      <FieldFrame label={locale === "zh-CN" ? "邮箱地址" : "Email Address"}>
                        <TextField value={profileEmail} onChange={(e) => setProfileEmail(e.target.value)} />
                      </FieldFrame>
                      <div className="hub-dialog-actions" style={{ marginTop: "16px" }}>
                        <Button variant="ghost" onClick={() => setProfileOpen(false)}>
                          {t("common.cancel")}
                        </Button>
                        <Button variant="primary" onClick={() => setProfileOpen(false)}>
                          {locale === "zh-CN" ? "保存" : "Save"}
                        </Button>
                      </div>
                    </div>
                  </DialogContent>
                </DialogRoot>
              </div>
            </div>

            {/* Appearance Settings */}
            <SettingsGroup heading={t("settings.appearance")} description={t("settings.appearanceDescription")}>
              <SettingsRow
                heading={t("settings.theme")}
                description={locale === "zh-CN" ? "选择界面的视觉主题基线" : "Select UI color theme"}
                control={
                  <div style={{ display: "flex", gap: "6px" }}>
                    <Button
                      size="small"
                      variant={themeMode === "light" ? "primary" : "ghost"}
                      onClick={() => setThemeMode("light")}
                    >
                      {locale === "zh-CN" ? "浅色" : "Light"}
                    </Button>
                    <Button
                      size="small"
                      variant={themeMode === "dark" ? "primary" : "ghost"}
                      onClick={() => setThemeMode("dark")}
                    >
                      {locale === "zh-CN" ? "深色" : "Dark"}
                    </Button>
                    <Button
                      size="small"
                      variant={themeMode === "system" ? "primary" : "ghost"}
                      onClick={() => setThemeMode("system")}
                    >
                      {locale === "zh-CN" ? "跟随系统" : "System"}
                    </Button>
                  </div>
                }
              />
              <SettingsRow heading={t("settings.density")} description={t("settings.densityDescription")} control={<Switch checked={compact} label={t("settings.useCompact")} onCheckedChange={(checked) => setCompact(checked === true)} />} />
              <SettingsRow heading={t("settings.reduceMotion")} description={t("settings.reduceMotionDescription")} control={<Switch checked={reducedMotion} label={t("settings.reduceMotion")} onCheckedChange={(checked) => setReducedMotion(checked === true)} />} />
            </SettingsGroup>

            {/* Language Settings */}
            <SettingsGroup heading={t("settings.languageGroup")}>
              <SettingsRow heading={t("settings.language")} description={t("settings.languageDescription")} control={<SelectField ariaLabel={t("settings.language")} onValueChange={(value) => setPreference(value as LanguagePreference)} options={[{ value: "zh-CN", label: t("settings.chinese") }, { value: "en-US", label: t("settings.english") }, { value: "system", label: t("settings.systemLanguage") }]} value={preference} />} />
            </SettingsGroup>

            {/* Cindy Orca Orchestration & Worker Concurrency */}
            <SettingsGroup
              heading={locale === "zh-CN" ? "Agent 协同与执行策略 (Orca Coordination)" : "Orca Agent Coordination"}
              description={locale === "zh-CN" ? "配置 Lead Agent 派发子任务的并发上限与 Git 保存点" : "Configure subagent worker limits and Git checkpointing"}
            >
              <SettingsRow
                heading={locale === "zh-CN" ? "子 Worker 最大并发数" : "Max Worker Concurrency"}
                description={locale === "zh-CN" ? "限制同时运行的后台 Worker 数量以防资源过载" : "Upper limit for parallel subagents"}
                control={
                  <SelectField
                    ariaLabel="Worker Limit"
                    value={workerLimit}
                    onValueChange={setWorkerLimit}
                    options={[
                      { value: "2", label: locale === "zh-CN" ? "2 个 Worker (节能)" : "2 Workers" },
                      { value: "4", label: locale === "zh-CN" ? "4 个 Worker (标准推荐)" : "4 Workers (Standard)" },
                      { value: "8", label: locale === "zh-CN" ? "8 个 Worker (高性能)" : "8 Workers (High)" },
                    ]}
                  />
                }
              />
              <SettingsRow
                heading={locale === "zh-CN" ? "Agent 资源消耗档位" : "Resource Mode"}
                description={locale === "zh-CN" ? "控制后台子任务分配的 CPU/内存资源优先级" : "Resource priority tier"}
                control={
                  <SelectField
                    ariaLabel="Resource Tier"
                    value={resourceTier}
                    onValueChange={setResourceTier}
                    options={[
                      { value: "eco", label: locale === "zh-CN" ? "节能模式 (Low Power)" : "Eco" },
                      { value: "balanced", label: locale === "zh-CN" ? "标准平衡 (Balanced)" : "Balanced" },
                      { value: "turbo", label: locale === "zh-CN" ? "极速高负荷 (Turbo)" : "Turbo" },
                    ]}
                  />
                }
              />
              <SettingsRow
                heading={locale === "zh-CN" ? "自动 Git 安全保存点" : "Git Auto Checkpoint"}
                description={locale === "zh-CN" ? "在执行危险批量代码编辑前自动创建 stash/commit 快照" : "Create safety commit before dangerous edits"}
                control={<Switch checked={gitSnapshot} label="Git Auto Snapshot" onCheckedChange={(c) => setGitSnapshot(c === true)} />}
              />
            </SettingsGroup>
          </>
        ) : section === "models" ? (
          <ProviderSettings
            defaultTarget={defaultTarget}
            loading={providersLoading}
            onDiscover={onDiscoverProviderModels}
            onSave={onSaveProvider}
            onSetDefault={onSetDefaultProvider}
            onTestRoute={onTestProviderRoute}
            providers={providers}
            routes={routes}
          />
        ) : section === "tools" ? (
          <>
            <SettingsGroup
              heading={locale === "zh-CN" ? "内置核心能力工具" : "Built-in Tool Primitives"}
              description={locale === "zh-CN" ? "控制 Agent 在会话中可直接调用的基础原子能力" : "Toggle built-in verbs for agents"}
            >
              <SettingsRow
                heading={locale === "zh-CN" ? "文件系统读写 (File I/O & Search)" : "File System"}
                description={locale === "zh-CN" ? "允许 Agent 查看、检索与编辑项目工作区文件" : "Read/write workspace files"}
                control={<Switch checked={toolFileIo} label="File IO" onCheckedChange={(c) => setToolFileIo(c === true)} />}
              />
              <SettingsRow
                heading={locale === "zh-CN" ? "终端 Shell 执行 (Terminal & Shell)" : "Terminal / Shell"}
                description={locale === "zh-CN" ? "允许 Agent 在受控沙箱或本地执行构建与测试命令" : "Execute shell commands"}
                control={<Switch checked={toolShell} label="Shell" onCheckedChange={(c) => setToolShell(c === true)} />}
              />
              <SettingsRow
                heading={locale === "zh-CN" ? "互联网搜索与抓取 (Web Search & Fetch)" : "Web Search"}
                description={locale === "zh-CN" ? "支持 Agent 查询实时在线文档与公开网页" : "Search and read URL contents"}
                control={<Switch checked={toolWeb} label="Web" onCheckedChange={(c) => setToolWeb(c === true)} />}
              />
              <SettingsRow
                heading={locale === "zh-CN" ? "沙箱浏览器自动化 (Chrome CDP)" : "Browser CDP"}
                description={locale === "zh-CN" ? "支持 Agent 启动无头浏览器进行页面审查与测试" : "Headless browser automation"}
                control={<Switch checked={toolBrowser} label="Browser" onCheckedChange={(c) => setToolBrowser(c === true)} />}
              />
              <SettingsRow
                heading={locale === "zh-CN" ? "Git 与 Worktree 隔离分支" : "Git & Worktree"}
                description={locale === "zh-CN" ? "支持 Worker 在独立 Git Worktree 分支中安全试错" : "Isolated branch execution"}
                control={<Switch checked={toolGit} label="Git" onCheckedChange={(c) => setToolGit(c === true)} />}
              />
            </SettingsGroup>

            <SettingsGroup
              heading={locale === "zh-CN" ? "外部 MCP 服务器 (Model Context Protocol)" : "MCP Servers"}
              description={locale === "zh-CN" ? "连接外部标准 MCP 协议服务，按需扩展专业工具" : "Connect external MCP servers"}
            >
              <Notice description={locale === "zh-CN" ? "以下为界面样例数据，尚未接入真实后端；添加/编辑暂不生效。" : "The entries below are interface samples not yet wired to a backend; adding or editing has no effect."} heading={locale === "zh-CN" ? "示例数据" : "Sample data"} role="status" tone="warning" />
              <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: "12px" }}>
                <DialogRoot open={addMcpOpen} onOpenChange={setAddMcpOpen}>
                  <DialogTrigger asChild>
                    <Button leadingIcon={<Plus size={14} />}>
                      {locale === "zh-CN" ? "添加 MCP 服务器" : "Add MCP Server"}
                    </Button>
                  </DialogTrigger>
                  <DialogContent
                    title={locale === "zh-CN" ? "添加 MCP 服务器" : "Add MCP Server"}
                    description={locale === "zh-CN" ? "配置标准 MCP 协议端点或 stdio 子进程命令" : "Configure MCP SSE endpoint or stdio command"}
                  >
                    <div className="hub-form-stack" style={{ marginTop: "12px" }}>
                      <FieldFrame label={locale === "zh-CN" ? "服务名称" : "Server Name"}>
                        <TextField
                          placeholder="e.g. My PostgreSQL MCP"
                          value={newMcpName}
                          onChange={(e) => setNewMcpName(e.target.value)}
                        />
                      </FieldFrame>
                      <FieldFrame label={locale === "zh-CN" ? "服务端点 / 启动命令" : "Endpoint / Command"}>
                        <TextField
                          placeholder="http://127.0.0.1:8080/sse or npx ..."
                          value={newMcpUrl}
                          onChange={(e) => setNewMcpUrl(e.target.value)}
                        />
                      </FieldFrame>
                      <div className="hub-dialog-actions" style={{ marginTop: "16px" }}>
                        <Button variant="ghost" onClick={() => setAddMcpOpen(false)}>
                          {t("common.cancel")}
                        </Button>
                        <Button
                          variant="primary"
                          disabled={!newMcpName.trim() || !newMcpUrl.trim()}
                          onClick={() => {
                            setMcpServers((curr) => [
                              ...curr,
                              {
                                id: `mcp-${Date.now()}`,
                                name: newMcpName.trim(),
                                transport: newMcpUrl.startsWith("http") ? "SSE" : "stdio",
                                endpoint: newMcpUrl.trim(),
                                enabled: true,
                              },
                            ]);
                            setNewMcpName("");
                            setNewMcpUrl("");
                            setAddMcpOpen(false);
                          }}
                        >
                          {locale === "zh-CN" ? "确认注册" : "Register Server"}
                        </Button>
                      </div>
                    </div>
                  </DialogContent>
                </DialogRoot>
              </div>

              {mcpServers.map((srv) => (
                <SettingsRow
                  key={srv.id}
                  heading={srv.name}
                  description={`${srv.transport.toUpperCase()} · ${srv.endpoint}`}
                  control={
                    <Switch
                      checked={srv.enabled}
                      label={srv.name}
                      onCheckedChange={(checked) =>
                        setMcpServers((curr) =>
                          curr.map((item) =>
                            item.id === srv.id ? { ...item, enabled: checked === true } : item,
                          ),
                        )
                      }
                    />
                  }
                />
              ))}
            </SettingsGroup>
          </>
        ) : section === "remote" ? (
          <SettingsGroup
            heading={locale === "zh-CN" ? "远程 SSH 主机列表" : "SSH Remote Hosts"}
            description={locale === "zh-CN" ? "配置云端服务器与集群，支持 Agent 在远程环境执行沙箱命令" : "Manage remote execution targets"}
          >
            <Notice description={locale === "zh-CN" ? "以下为界面样例数据，尚未接入真实后端；添加/编辑暂不生效。" : "The entries below are interface samples not yet wired to a backend; adding or editing has no effect."} heading={locale === "zh-CN" ? "示例数据" : "Sample data"} role="status" tone="warning" />
            <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: "12px" }}>
              <DialogRoot open={addSshOpen} onOpenChange={setAddSshOpen}>
                <DialogTrigger asChild>
                  <Button leadingIcon={<Plus size={14} />}>
                    {locale === "zh-CN" ? "添加 SSH 主机" : "Add SSH Host"}
                  </Button>
                </DialogTrigger>
                <DialogContent
                  title={locale === "zh-CN" ? "添加远程 SSH 主机" : "Add SSH Host"}
                  description={locale === "zh-CN" ? "配置远程机器 IP、登录用户名及 SSH 密钥连接" : "Configure remote host details"}
                >
                  <div className="hub-form-stack" style={{ marginTop: "12px" }}>
                    <FieldFrame label={locale === "zh-CN" ? "主机别名" : "Host Name"}>
                      <TextField placeholder="e.g. GPU Server 4090" value={newSshName} onChange={(e) => setNewSshName(e.target.value)} />
                    </FieldFrame>
                    <FieldFrame label={locale === "zh-CN" ? "IP 地址 / 域名" : "IP Address / Hostname"}>
                      <TextField placeholder="192.168.1.100" value={newSshHost} onChange={(e) => setNewSshHost(e.target.value)} />
                    </FieldFrame>
                    <FieldFrame label={locale === "zh-CN" ? "登录用户名" : "User"}>
                      <TextField placeholder="root / ubuntu" value={newSshUser} onChange={(e) => setNewSshUser(e.target.value)} />
                    </FieldFrame>
                    <div className="hub-dialog-actions" style={{ marginTop: "16px" }}>
                      <Button variant="ghost" onClick={() => setAddSshOpen(false)}>{t("common.cancel")}</Button>
                      <Button
                        variant="primary"
                        disabled={!newSshName.trim() || !newSshHost.trim()}
                        onClick={() => {
                          setSshHosts((curr) => [
                            ...curr,
                            {
                              id: `ssh-${Date.now()}`,
                              name: newSshName.trim(),
                              host: newSshHost.trim(),
                              user: newSshUser.trim() || "root",
                              port: 22,
                              status: "idle",
                            },
                          ]);
                          setNewSshName("");
                          setNewSshHost("");
                          setAddSshOpen(false);
                        }}
                      >
                        {locale === "zh-CN" ? "确认添加" : "Add Host"}
                      </Button>
                    </div>
                  </div>
                </DialogContent>
              </DialogRoot>
            </div>

            {sshHosts.map((h) => (
              <SettingsRow
                key={h.id}
                heading={h.name}
                description={`${h.user}@${h.host}:${h.port} · SSH 密钥免密登录`}
                control={
                  <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                    <Badge tone={h.status === "connected" ? "success" : "neutral"}>
                      {h.status === "connected" ? (locale === "zh-CN" ? "已连接" : "Connected") : (locale === "zh-CN" ? "就绪" : "Ready")}
                    </Badge>
                    <Button size="small" variant="ghost">{locale === "zh-CN" ? "测试连通" : "Test"}</Button>
                  </div>
                }
              />
            ))}
          </SettingsGroup>
        ) : section === "im" ? (
          <SettingsGroup
            heading={locale === "zh-CN" ? "即时通讯机器人网关" : "IM Bot Bridges"}
            description={locale === "zh-CN" ? "将 Alva Agent 挂接到团队即时通讯群组，支持 @机器人 触发任务" : "Connect agents to team chat channels"}
          >
            <SettingsRow
              heading={locale === "zh-CN" ? "企业微信机器人 (WeCom Bot)" : "WeCom Bot"}
              description="Webhook: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=..."
              control={<Badge tone="neutral">{locale === "zh-CN" ? "未配置" : "Inactive"}</Badge>}
            />
            <SettingsRow
              heading={locale === "zh-CN" ? "飞书自定义机器人 (Feishu Bot)" : "Feishu Bot"}
              description="Open Platform AppID: cli_a1b2c3d4..."
              control={<Badge tone="neutral">{locale === "zh-CN" ? "未配置" : "Inactive"}</Badge>}
            />
            <SettingsRow
              heading={locale === "zh-CN" ? "钉钉群机器人 (DingTalk Bot)" : "DingTalk Bot"}
              description="Webhook Gateway Endpoint"
              control={<Badge tone="neutral">{locale === "zh-CN" ? "未配置" : "Inactive"}</Badge>}
            />
          </SettingsGroup>
        ) : section === "keybindings" ? (
          <SettingsGroup
            heading={locale === "zh-CN" ? "系统常用键盘快捷键" : "Keyboard Shortcuts"}
            description={locale === "zh-CN" ? "掌握高效快捷键以加速代码协作与会话流转" : "Hub navigation and action hotkeys"}
          >
            <SettingsRow heading={locale === "zh-CN" ? "全局快速搜索" : "Global Search"} description="随时呼出全局命令面板与会话检索" control={<Badge tone="accent">⌘ K</Badge>} />
            <SettingsRow heading={locale === "zh-CN" ? "新建会话" : "New Session"} description="快速开始一个新的空白独立或项目会话" control={<Badge>⌘ N</Badge>} />
            <SettingsRow heading={locale === "zh-CN" ? "切换工作台面板" : "Toggle Workbench"} description="展开或折叠右侧多标签工作台" control={<Badge>⌘ B</Badge>} />
            <SettingsRow heading={locale === "zh-CN" ? "发送指令 / 提交" : "Send Prompt"} description="在输入框中向 Agent 提交当前指令" control={<Badge tone="accent">Enter / ⌘ ↵</Badge>} />
            <SettingsRow heading={locale === "zh-CN" ? "停止当前运行" : "Stop Execution"} description="中断当前正在执行的 Agent 循环或子任务" control={<Badge tone="danger">Esc</Badge>} />
          </SettingsGroup>
        ) : section === "permissions" ? (
          <SettingsGroup
            heading={locale === "zh-CN" ? "默认执行权限与安全控制" : "Security & Permissions"}
            description={locale === "zh-CN" ? "设定 Agent 访问本地文件与执行 Shell 时的安全拦截级别" : "Configure HITL permission modes"}
          >
            <SettingsRow
              heading={locale === "zh-CN" ? "默认审批模式（仅影响新会话）" : "Default Permission Mode (new sessions only)"}
              description={
                locale === "zh-CN"
                  ? "仅支持 Ask / Auto Edits 两档；'只读计划' 无等价权限档，为避免假成功已移除。持久化接线开发中，当前为组件内默认值。"
                  : "Only Ask / Auto Edits are supported; 'Plan Only' has no equivalent profile and was removed. Persistence wiring is in progress."
              }
              control={
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <Badge tone="warning">
                    {locale === "zh-CN" ? "持久化开发中" : "Persistence in progress"}
                  </Badge>
                  <SelectField
                    ariaLabel="Permission Mode"
                    value={permissionMode}
                    onValueChange={(v) => setPermissionMode(v as typeof permissionMode)}
                    options={[
                      { value: "ask", label: locale === "zh-CN" ? "逐项询问 (Ask - 推荐)" : "Ask (Recommended)" },
                      { value: "accept_edits", label: locale === "zh-CN" ? "自动代码编辑 (Auto Edits)" : "Auto Accept Edits" },
                    ]}
                  />
                </div>
              }
            />
            <SettingsRow
              heading={locale === "zh-CN" ? "敏感路径保护过滤" : "Sensitive Path Filtering"}
              description={locale === "zh-CN" ? "禁止 Agent 读取或写入 .env、私钥、id_rsa 及密码凭据文件" : "Block access to keys and credentials"}
              control={<Badge tone="success">{locale === "zh-CN" ? "严格保护中 (ON)" : "Protected"}</Badge>}
            />
          </SettingsGroup>
        ) : section === "usage" ? (
          <>
            <div className="hub-metric-grid" style={{ marginBottom: "20px" }}>
              <div>
                <small>{locale === "zh-CN" ? "本月总 Token 消耗" : "Total Tokens (Month)"}</small>
                <strong>1,420,500</strong>
              </div>
              <div>
                <small>{locale === "zh-CN" ? "预估 API 总支出" : "Estimated Cost"}</small>
                <strong style={{ color: "var(--color-success, #10b981)" }}>$14.28</strong>
              </div>
            </div>
            <SettingsGroup
              heading={locale === "zh-CN" ? "大模型消耗明细" : "Model Usage Breakdown"}
              description={locale === "zh-CN" ? "按厂商与模型统计调用次数与 Token 占比" : "Usage per provider and model"}
            >
              <SettingsRow heading="Codex · GPT-5.6-Sol" description="920,000 tokens · 64.8%" control={<Badge tone="accent">65%</Badge>} />
              <SettingsRow heading="Anthropic · Claude 3.7 Sonnet" description="350,000 tokens · 24.6%" control={<Badge>25%</Badge>} />
              <SettingsRow heading="SiliconFlow · DeepSeek-R1" description="150,500 tokens · 10.6%" control={<Badge>10%</Badge>} />
            </SettingsGroup>
          </>
        ) : section === "experiments" ? (
          <SettingsGroup heading={t("settings.developerSurfaces")} description={t("settings.developerDescription")}>
            <SettingsRow heading={t("settings.agentInspector")} description={t("settings.agentInspectorDescription")} control={<Switch checked={inspector} label={t("settings.showInspector")} onCheckedChange={(checked) => setInspector(checked === true)} />} />
            <SettingsRow heading={t("settings.uiLab")} description={t("settings.uiLabDescription")} control={<Badge>{t("settings.developmentOnly")}</Badge>} />
          </SettingsGroup>
        ) : (
          <SettingsGroup heading={locale === "zh-CN" ? "系统状态与关于" : "System & About"} description={t("settings.description")}>
            <SettingsRow heading="ALVA Hub Desktop Core" description="v0.1.45 (Stable Release · Architecture V1)" control={<Badge tone="success">v0.1.45</Badge>} />
            <SettingsRow heading={locale === "zh-CN" ? "检查软件更新" : "Check for Updates"} description={locale === "zh-CN" ? "当前已是最新版本" : "Already up to date"} control={<Button size="small" variant="ghost">{locale === "zh-CN" ? "检查更新" : "Check"}</Button>} />
          </SettingsGroup>
        )}
      </ScrollRegion>
    </div>
  );
}

export function SearchPage({ sessions, onOpenSession, onNavigate, onOpenWorkbench }: { sessions: Array<{ id: string; title: string; description: string }>; onOpenSession: (id: string) => void; onNavigate: (page: HubPage) => void; onOpenWorkbench: (tool: "inspector" | "usage") => void }) {
  const [query, setQuery] = useState("");
  const { t } = useI18n();
  const results = useMemo(() => sessions.filter((session) => `${session.title} ${session.description}`.toLowerCase().includes(query.trim().toLowerCase())), [query, sessions]);
  return (
    <ScrollRegion className="hub-product-page hub-search-page">
      <PageHeader description={t("search.description")} title={t("nav.search")} />
      <label className="hub-search-field hub-search-field--large">
        <Search aria-hidden="true" size={18} />
        <TextField autoFocus aria-label={t("search.hub")} placeholder={t("search.hubPlaceholder")} value={query} onChange={(event) => setQuery(event.target.value)} />
        <kbd>⌘ K</kbd>
      </label>
      <section className="hub-product-section">
        <h3>{query ? t("search.results") : t("search.quickActions")}</h3>
        <div className="hub-product-list">
          {query ? results.map((session) => (
            <StatusListItem
              className="hub-search-result"
              description={session.description}
              heading={session.title}
              key={session.id}
              leading={<Bot aria-hidden="true" size={17} />}
              onClick={() => onOpenSession(session.id)}
            />
          )) : (
            <>
              <StatusListItem heading={t("search.openSettings")} description={t("search.openSettingsDescription")} leading={<Settings2 aria-hidden="true" size={17} />} onClick={() => onNavigate("settings")} trailing={<Command aria-hidden="true" size={14} />} />
              <StatusListItem heading={t("search.inspectSession")} description={t("search.inspectSessionDescription")} leading={<FlaskConical aria-hidden="true" size={17} />} onClick={() => onOpenWorkbench("inspector")} />
              <StatusListItem heading={t("search.reviewUsage")} description={t("search.reviewUsageDescription")} leading={<Gauge aria-hidden="true" size={17} />} onClick={() => onOpenWorkbench("usage")} />
            </>
          )}
        </div>
      </section>
    </ScrollRegion>
  );
}
