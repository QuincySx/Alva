// INPUT:  React local state, appearance/i18n preferences, product callbacks, ALVA UI facade, and Lucide icons
// OUTPUT: Real Automations, included ALVA plugins/MCP, workspace Skills, settings and search pages
// POS:    Secondary Hub product surfaces above the shared UI component system

import { useMemo, useState } from "react";
import {
  Bot,
  Command,
  FlaskConical,
  Gauge,
  Search,
  Settings2,
} from "lucide-react";
import {
  Badge,
  Button,
  EmptyState,
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
  CreateProviderProfile,
} from "../bridge";
import { useI18n, type LanguagePreference } from "../i18n";
import { ProviderSettings } from "./provider-settings";
import { useAppearance } from "../appearance";
import { WorkspaceSkills } from "./workspace-skills";
import { ToolSettings } from "./tool-settings";

export type HubPage = "conversation" | "automations" | "extensions" | "settings" | "search";

export { AutomationsPage } from "./automations";

export function ExtensionsPage({ onStartChat, sessionId = null }: { onStartChat: () => void; sessionId?: string | null }) {
  const { t } = useI18n();
  const [tab, setTab] = useState("plugin");
  const [query, setQuery] = useState("");
  return (
    <ScrollRegion className="hub-product-page">
      <PageHeader title={t("extension.title")} description={t("plugins.description")} actions={<Button onClick={onStartChat} variant="ghost">{t("extension.useInChat")}</Button>} />
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
      {tab === "skill" ? <WorkspaceSkills sessionId={sessionId} query={query} /> : <>
        <Notice tone="info" heading={t("plugins.title")} description={t("plugins.description")} />
        <ToolSettings key={sessionId ?? "no-session"} sessionId={sessionId} query={query} />
      </>}
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
  sessionId = null,
  defaultTarget,
  providers,
  providersLoading,
  onSaveProvider,
  onCreateProvider,
  onDeleteProvider,
  onSetDefaultProvider,
  onDiscoverProviderModels,
  onTestProviderRoute,
  onOpenInspector,
  routes,
}: {
  sessionId?: string | null;
  defaultTarget: EngineTarget | null;
  providers: ProviderProfile[];
  providersLoading: boolean;
  onSaveProvider: (provider: SaveProviderProfile) => Promise<void>;
  onCreateProvider: (provider: CreateProviderProfile) => Promise<ProviderProfile>;
  onDeleteProvider: (id: string) => Promise<void>;
  onSetDefaultProvider: (target: EngineTarget) => Promise<void>;
  onDiscoverProviderModels: (providerId: string) => Promise<ProviderDiagnosticResult>;
  onTestProviderRoute: (target: EngineTarget) => Promise<ProviderDiagnosticResult>;
  onOpenInspector?: () => void;
  routes: ModelRoute[];
}) {
  const { preference, setPreference, t } = useI18n();
  const [section, setSection] = useState<(typeof settingsSections)[number]>("general");
  const { theme: themeMode, compact, reduceMotion, persistenceFailed, update: updateAppearance } = useAppearance();
  const [workerLimit, setWorkerLimit] = useState("4");
  const [resourceTier, setResourceTier] = useState("balanced");
  const [gitSnapshot, setGitSnapshot] = useState(true);
  // R21: 只保留真实存在的权限档（Ask / Auto Edits）；"只读计划" 无等价
  // PermissionProfile，为避免假成功已移除。持久化接线开发中。
  const [permissionMode, setPermissionMode] = useState<"ask" | "accept_edits">("ask");

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

            {persistenceFailed ? <Notice tone="warning" role="status" heading={t("settings.appearanceSaveFailed")} /> : null}
            <SettingsGroup heading={t("settings.appearance")} description={t("settings.appearanceDescription")}>
              <SettingsRow
                heading={t("settings.themeTitle")}
                description={t("settings.themeDescription")}
                control={
                  <div style={{ display: "flex", gap: "6px" }}>
                    <Button
                      aria-pressed={themeMode === "light"}
                      size="small"
                      variant={themeMode === "light" ? "primary" : "ghost"}
                      onClick={() => updateAppearance({ theme: "light" })}
                    >
                      {t("pages.settings.themeLight")}
                    </Button>
                    <Button
                      aria-pressed={themeMode === "dark"}
                      size="small"
                      variant={themeMode === "dark" ? "primary" : "ghost"}
                      onClick={() => updateAppearance({ theme: "dark" })}
                    >
                      {t("pages.settings.themeDark")}
                    </Button>
                    <Button
                      aria-pressed={themeMode === "system"}
                      size="small"
                      variant={themeMode === "system" ? "primary" : "ghost"}
                      onClick={() => updateAppearance({ theme: "system" })}
                    >
                      {t("pages.settings.themeSystem")}
                    </Button>
                  </div>
                }
              />
              <SettingsRow heading={t("settings.density")} description={t("settings.densityDescriptionTruthful")} control={<Switch checked={compact} label={t("settings.useCompact")} onCheckedChange={(checked) => updateAppearance({ compact: checked === true })} />} />
              <SettingsRow heading={t("settings.reduceMotion")} description={t("settings.reduceMotionDescriptionTruthful")} control={<Switch checked={reduceMotion} label={t("settings.reduceMotion")} onCheckedChange={(checked) => updateAppearance({ reduceMotion: checked === true })} />} />
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
            onCreate={onCreateProvider}
            onDelete={onDeleteProvider}
            onSetDefault={onSetDefaultProvider}
            onTestRoute={onTestProviderRoute}
            providers={providers}
            routes={routes}
          />
        ) : section === "tools" ? (
          <ToolSettings key={sessionId ?? "no-session"} sessionId={sessionId} />
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
            <SettingsRow heading={t("settings.agentInspector")} description={t("settings.agentInspectorDescription")} control={<Button disabled={!onOpenInspector} onClick={onOpenInspector}>{t("settings.openInspector")}</Button>} />
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
