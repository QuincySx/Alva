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
  const { t } = useI18n();
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
              {t("pages.automations.schedulingUnavailable")}
            </Badge>
            <Button
              disabled
              aria-label={t("pages.automations.notYetAria")}
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
            : t("pages.automations.notYetMeta")
        }
        title={t("nav.automations")}
      />
      <EmptyState
        icon={<Workflow aria-hidden="true" size={18} />}
        title={t("pages.automations.notYetTitle")}
        description={t("pages.automations.notYetDescription")}
      />
      <section className="hub-product-section">
        <h3>
          {t("automation.templates")}{" "}
          <Badge tone="warning">
            {t("pages.automations.previewUnavailable")}
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
  const { t } = useI18n();
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
              description={t("pages.extensions.registerUnavailable")}
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
                      aria-label={t("pages.extensions.installUnsupportedAria")}
                      disabled
                      leadingIcon={<Download aria-hidden="true" size={14} />}
                      size="small"
                      title={t("pages.extensions.installUnsupportedTitle")}
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
  const { preference, setPreference, t } = useI18n();
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

  // Tools toggles — R31/R32: 不可用的假开关保持禁用展示（值不再被消费）。
  const [toolFileIo, setToolFileIo] = useState(true);
  const [toolShell, setToolShell] = useState(true);
  const [toolWeb, setToolWeb] = useState(true);
  const [toolBrowser, setToolBrowser] = useState(true);
  const [toolGit, setToolGit] = useState(true);

  // R37: per-section copy comes from the typed dictionary — no inline locale
  // branches in this domain component.
  const sectionCopy: Record<(typeof settingsSections)[number], { nav: string; title: string; description: string }> = {
    general: { nav: t("pages.settings.navGeneral"), title: t("pages.settings.generalTitle"), description: t("pages.settings.generalDescription") },
    models: { nav: t("pages.settings.navModels"), title: t("pages.settings.modelsTitle"), description: t("pages.settings.modelsDescription") },
    tools: { nav: t("pages.settings.navTools"), title: t("pages.settings.toolsTitle"), description: t("pages.settings.toolsDescription") },
    remote: { nav: t("pages.settings.navRemote"), title: t("pages.settings.remoteTitle"), description: t("pages.settings.remoteDescription") },
    im: { nav: t("pages.settings.navIm"), title: t("pages.settings.imTitle"), description: t("pages.settings.imDescription") },
    keybindings: { nav: t("pages.settings.navKeybindings"), title: t("pages.settings.keybindingsTitle"), description: t("pages.settings.keybindingsDescription") },
    usage: { nav: t("pages.settings.navUsage"), title: t("pages.settings.usageTitle"), description: t("pages.settings.usageDescription") },
    permissions: { nav: t("pages.settings.navPermissions"), title: t("pages.settings.permissionsTitle"), description: t("pages.settings.permissionsDescription") },
    experiments: { nav: t("pages.settings.navExperiments"), title: t("pages.settings.experimentsTitle"), description: t("pages.settings.experimentsDescription") },
    help: { nav: t("pages.settings.navHelp"), title: t("pages.settings.helpTitle"), description: t("pages.settings.helpDescription") },
  };

  return (
    <div className="hub-settings-frame">
      <nav aria-label={t("settings.sections")} className="hub-settings-nav">
        {settingsSections.map((candidate) => (
          <NavItem
            active={section === candidate}
            key={candidate}
            label={sectionCopy[candidate].nav}
            onClick={() => setSection(candidate)}
          />
        ))}
      </nav>
      <ScrollRegion className="hub-product-page hub-settings-page">
        <PageHeader
          description={sectionCopy[section].description}
          title={sectionCopy[section].title}
        />

        {section === "general" ? (
          <>
            {/* R44: no sample identity, no fake Save/Cancel. Profile
                persistence is not wired up, so the entry is disabled and the
                card carries no invented user. */}
            <div className="hub-profile-card">
              <div className="hub-profile-card__left">
                <div className="hub-profile-avatar">·</div>
                <div className="hub-profile-details">
                  <h4>{t("pages.settings.profileLocalTitle")}</h4>
                  <p>{t("pages.settings.profileNotWired")}</p>
                </div>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
                <Button disabled size="small" variant="ghost">
                  {t("pages.settings.editProfile")}
                </Button>
              </div>
            </div>

            {/* Appearance Settings — R32: 浅色/系统主题、全局密度、减弱动效
                开关都没有真实实现；诚实禁用并说明真实行为。 */}
            <SettingsGroup heading={t("settings.appearance")} description={t("settings.appearanceDescription")}>
              <SettingsRow
                heading={t("settings.themeTitle")}
                description={t("settings.themeDescription")}
                control={
                  <div style={{ display: "flex", gap: "6px" }}>
                    <Button
                      disabled
                      size="small"
                      variant={themeMode === "light" ? "primary" : "ghost"}
                      onClick={() => setThemeMode("light")}
                    >
                      {t("pages.settings.themeLight")}
                    </Button>
                    <Button
                      size="small"
                      variant={themeMode === "dark" ? "primary" : "ghost"}
                      onClick={() => setThemeMode("dark")}
                    >
                      {t("pages.settings.themeDark")}
                    </Button>
                    <Button
                      disabled
                      size="small"
                      variant={themeMode === "system" ? "primary" : "ghost"}
                      onClick={() => setThemeMode("system")}
                    >
                      {t("pages.settings.themeSystem")}
                    </Button>
                  </div>
                }
              />
              <SettingsRow heading={t("settings.density")} description={t("settings.densityDescriptionTruthful")} control={<Switch checked={compact} disabled label={t("settings.useCompact")} onCheckedChange={(checked) => setCompact(checked === true)} />} />
              <SettingsRow heading={t("settings.reduceMotion")} description={t("settings.reduceMotionDescriptionTruthful")} control={<Switch checked={reducedMotion} disabled label={t("settings.reduceMotion")} onCheckedChange={(checked) => setReducedMotion(checked === true)} />} />
            </SettingsGroup>

            {/* Language Settings */}
            <SettingsGroup heading={t("settings.languageGroup")}>
              <SettingsRow heading={t("settings.language")} description={t("settings.languageDescription")} control={<SelectField ariaLabel={t("settings.language")} onValueChange={(value) => setPreference(value as LanguagePreference)} options={[{ value: "zh-CN", label: t("settings.chinese") }, { value: "en-US", label: t("settings.english") }, { value: "system", label: t("settings.systemLanguage") }]} value={preference} />} />
            </SettingsGroup>

            {/* Cindy Orca Orchestration & Worker Concurrency — R31: 这些
                开关没有连接任何引擎，真实 Worker 并发来自各引擎的
                TeamRun；诚实禁用而不是假装生效。 */}
            <SettingsGroup
              heading={t("settings.workerControlsTitle")}
              description={t("settings.workerControlsDescription")}
            >
              <SettingsRow
                heading={t("pages.settings.workerConcurrency")}
                description={t("settings.truthfulNotWired")}
                control={
                  <SelectField
                    ariaLabel={t("pages.settings.workerConcurrency")}
                    disabled
                    value={workerLimit}
                    onValueChange={setWorkerLimit}
                    options={[
                      { value: "2", label: t("pages.settings.workersEco") },
                      { value: "4", label: t("pages.settings.workersStandard") },
                      { value: "8", label: t("pages.settings.workersHigh") },
                    ]}
                  />
                }
              />
              <SettingsRow
                heading={t("pages.settings.resourceMode")}
                description={t("settings.truthfulNotWired")}
                control={
                  <SelectField
                    ariaLabel={t("pages.settings.resourceMode")}
                    disabled
                    value={resourceTier}
                    onValueChange={setResourceTier}
                    options={[
                      { value: "eco", label: t("pages.settings.resourceEco") },
                      { value: "balanced", label: t("pages.settings.resourceBalanced") },
                      { value: "turbo", label: t("pages.settings.resourceTurbo") },
                    ]}
                  />
                }
              />
              <SettingsRow
                heading={t("pages.settings.gitCheckpoint")}
                description={t("settings.truthfulNotWired")}
                control={<Switch checked={gitSnapshot} disabled label={t("pages.settings.gitCheckpoint")} onCheckedChange={(c) => setGitSnapshot(c === true)} />}
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
              heading={t("pages.settings.toolsGroup")}
              description={t("settings.toolTogglesDescription")}
            >
              <SettingsRow
                heading={t("pages.settings.toolFile")}
                description={t("settings.truthfulNotWired")}
                control={<Switch checked={toolFileIo} disabled label={t("pages.settings.toolFile")} onCheckedChange={(c) => setToolFileIo(c === true)} />}
              />
              <SettingsRow
                heading={t("pages.settings.toolShell")}
                description={t("settings.truthfulNotWired")}
                control={<Switch checked={toolShell} disabled label={t("pages.settings.toolShell")} onCheckedChange={(c) => setToolShell(c === true)} />}
              />
              <SettingsRow
                heading={t("pages.settings.toolWeb")}
                description={t("settings.truthfulNotWired")}
                control={<Switch checked={toolWeb} disabled label={t("pages.settings.toolWeb")} onCheckedChange={(c) => setToolWeb(c === true)} />}
              />
              <SettingsRow
                heading={t("pages.settings.toolBrowser")}
                description={t("settings.truthfulNotWired")}
                control={<Switch checked={toolBrowser} disabled label={t("pages.settings.toolBrowser")} onCheckedChange={(c) => setToolBrowser(c === true)} />}
              />
              <SettingsRow
                heading={t("pages.settings.toolGit")}
                description={t("settings.truthfulNotWired")}
                control={<Switch checked={toolGit} disabled label={t("pages.settings.toolGit")} onCheckedChange={(c) => setToolGit(c === true)} />}
              />
            </SettingsGroup>

            {/* R32: MCP 管理未接后端——诚实呈现未接入状态，不再展示可增删
                的样例列表（样例归属 UI Lab demo）。 */}
            <SettingsGroup
              heading={t("pages.settings.mcpGroup")}
              description={t("settings.mcpNotWired")}
            >
              <Notice
                description={t("settings.mcpNotWired")}
                heading={t("settings.notWiredTitle")}
                role="status"
                tone="neutral"
              />
            </SettingsGroup>
          </>
        ) : section === "remote" ? (
          <SettingsGroup
            heading={t("pages.settings.remoteTitle")}
            description={t("settings.sshNotWired")}
          >
            {/* R32: SSH 执行未实现——诚实空态，移除假主机样例与添加对话框。 */}
            <Notice
              description={t("settings.sshNotWired")}
              heading={t("settings.notWiredTitle")}
              role="status"
              tone="neutral"
            />
          </SettingsGroup>
        ) : section === "im" ? (
          <SettingsGroup
            heading={t("pages.settings.imGroupHeading")}
            description={t("pages.settings.imGroupDescription")}
          >
            <SettingsRow
              heading={t("pages.settings.imWeCom")}
              description="Webhook: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=..."
              control={<Badge tone="neutral">{t("pages.settings.inactive")}</Badge>}
            />
            <SettingsRow
              heading={t("pages.settings.imFeishu")}
              description="Open Platform AppID: cli_a1b2c3d4..."
              control={<Badge tone="neutral">{t("pages.settings.inactive")}</Badge>}
            />
            <SettingsRow
              heading={t("pages.settings.imDingTalk")}
              description="Webhook Gateway Endpoint"
              control={<Badge tone="neutral">{t("pages.settings.inactive")}</Badge>}
            />
          </SettingsGroup>
        ) : section === "keybindings" ? (
          <SettingsGroup
            heading={t("pages.settings.kbGroup")}
            description={t("pages.settings.kbGroupDescription")}
          >
            <SettingsRow heading={t("pages.settings.kbSearch")} description={t("pages.settings.kbSearchDescription")} control={<Badge tone="accent">⌘ K</Badge>} />
            <SettingsRow heading={t("pages.settings.kbNewSession")} description={t("pages.settings.kbNewSessionDescription")} control={<Badge>⌘ N</Badge>} />
            <SettingsRow heading={t("pages.settings.kbWorkbench")} description={t("pages.settings.kbWorkbenchDescription")} control={<Badge>⌘ B</Badge>} />
            <SettingsRow heading={t("pages.settings.kbSend")} description={t("pages.settings.kbSendDescription")} control={<Badge tone="accent">Enter / ⌘ ↵</Badge>} />
            <SettingsRow heading={t("pages.settings.kbStop")} description={t("pages.settings.kbStopDescription")} control={<Badge tone="danger">Esc</Badge>} />
          </SettingsGroup>
        ) : section === "permissions" ? (
          <SettingsGroup
            heading={t("pages.settings.permGroup")}
            description={t("pages.settings.permGroupDescription")}
          >
            <SettingsRow
              heading={t("pages.settings.permModeTitle")}
              description={t("settings.permissionSource")}
              control={
                <SelectField
                  ariaLabel={t("pages.settings.permModeTitle")}
                  disabled
                  value={permissionMode}
                  onValueChange={(v) => setPermissionMode(v as typeof permissionMode)}
                  options={[
                    { value: "ask", label: t("pages.settings.permAsk") },
                    { value: "accept_edits", label: t("pages.settings.permAutoEdits") },
                  ]}
                />
              }
            />
            <SettingsRow
              heading={t("settings.sensitivePathTitle")}
              description={t("settings.sensitivePathDescription")}
              control={<Badge tone="neutral">{t("settings.sensitivePathBadge")}</Badge>}
            />
          </SettingsGroup>
        ) : section === "usage" ? (
          // R32: 用量统计未收集——诚实呈现未接入，不虚构 token/成本/占比。
          <SettingsGroup
            heading={t("pages.settings.usageTitle")}
            description={t("settings.usageNotWired")}
          >
            <Notice
              description={t("settings.usageNotWired")}
              heading={t("settings.notWiredTitle")}
              role="status"
              tone="neutral"
            />
          </SettingsGroup>
        ) : section === "experiments" ? (
          <SettingsGroup heading={t("settings.developerSurfaces")} description={t("settings.developerDescription")}>
            <SettingsRow heading={t("settings.agentInspector")} description={t("settings.agentInspectorDescription")} control={<Switch checked={inspector} label={t("settings.showInspector")} onCheckedChange={(checked) => setInspector(checked === true)} />} />
            <SettingsRow heading={t("settings.uiLab")} description={t("settings.uiLabDescription")} control={<Badge>{t("settings.developmentOnly")}</Badge>} />
          </SettingsGroup>
        ) : (
          // R32: 版本/更新未接 IPC——不虚构版本号与“已是最新”结论。
          <SettingsGroup heading={t("pages.settings.aboutGroup")} description={t("settings.description")}>
            <SettingsRow
              heading="ALVA Hub Desktop Core"
              description={t("settings.versionNotWired")}
              control={<Badge tone="neutral">{t("settings.notWiredTitle")}</Badge>}
            />
          </SettingsGroup>
        )}
      </ScrollRegion>
    </div>
  );
}

export function SearchPage({ sessions, onOpenSession, onNavigate, onOpenWorkbench }: { sessions: Array<{ id: string; title: string; description: string }>; onOpenSession: (id: string) => void; onNavigate: (page: HubPage) => void; onOpenWorkbench: (tool: "inspector" | "usage") => void }) {
  const [query, setQuery] = useState("");
  const { t } = useI18n();
  const { locale } = useI18n();
  // R59: the search scope now matches the claim. Actions are first-class
  // indexed entries — matching one navigates for real — and an empty result
  // set says so instead of rendering a blank section.
  const actions = useMemo(() => [
    {
      id: "action:settings",
      title: t("search.openSettings"),
      description: t("search.openSettingsDescription"),
      icon: <Settings2 aria-hidden="true" size={17} />,
      run: () => onNavigate("settings"),
    },
    {
      id: "action:inspector",
      title: t("search.inspectSession"),
      description: t("search.inspectSessionDescription"),
      icon: <FlaskConical aria-hidden="true" size={17} />,
      run: () => onOpenWorkbench("inspector"),
    },
    {
      id: "action:usage",
      title: t("search.reviewUsage"),
      description: t("search.reviewUsageDescription"),
      icon: <Gauge aria-hidden="true" size={17} />,
      run: () => onOpenWorkbench("usage"),
    },
  ], [locale, onNavigate, onOpenWorkbench, t]);
  const needle = query.trim().toLowerCase();
  const sessionResults = useMemo(
    () => sessions.filter((session) => `${session.title} ${session.description}`.toLowerCase().includes(needle)),
    [needle, sessions],
  );
  const actionResults = useMemo(
    () => actions.filter((action) => `${action.title} ${action.description}`.toLowerCase().includes(needle)),
    [actions, needle],
  );
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
          {!query ? (
            <>
              {actions.map((action) => (
                <StatusListItem heading={action.title} description={action.description} key={action.id} leading={action.icon} onClick={action.run} trailing={<Command aria-hidden="true" size={14} />} />
              ))}
            </>
          ) : sessionResults.length === 0 && actionResults.length === 0 ? (
            <EmptyState title={t("search.noResults")} description={t("search.noResultsDescription")} />
          ) : (
            <>
              {sessionResults.length > 0 ? <h4 className="hub-search-group">{t("search.sectionSessions")}</h4> : null}
              {sessionResults.map((session) => (
                <StatusListItem
                  className="hub-search-result"
                  description={session.description}
                  heading={session.title}
                  key={session.id}
                  leading={<Bot aria-hidden="true" size={17} />}
                  onClick={() => onOpenSession(session.id)}
                />
              ))}
              {actionResults.length > 0 ? <h4 className="hub-search-group">{t("search.sectionActions")}</h4> : null}
              {actionResults.map((action) => (
                <StatusListItem heading={action.title} description={action.description} key={action.id} leading={action.icon} onClick={action.run} trailing={<Command aria-hidden="true" size={14} />} />
              ))}
            </>
          )}
        </div>
      </section>
    </ScrollRegion>
  );
}
