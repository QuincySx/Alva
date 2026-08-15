// INPUT:  React local state, ALVA UI facade, and Lucide icons
// OUTPUT: Automations, extensions, settings, and search product pages
// POS:    Deterministic Hub product surfaces above the shared UI component system

import { useMemo, useState } from "react";
import {
  Blocks,
  Bot,
  CalendarClock,
  Check,
  CircleHelp,
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
  ListRow,
  NavItem,
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
  TextArea,
  TextField,
} from "../ui";
import type {
  EngineTarget,
  ModelRoute,
  ProviderDiagnosticResult,
  ProviderProfile,
  SaveProviderProfile,
} from "../bridge";
import { useI18n, type LanguagePreference, type Translate } from "../i18n";
import { ProviderSettings } from "./provider-settings";

export type HubPage = "conversation" | "automations" | "extensions" | "settings" | "search";

interface AutomationItem {
  id: string;
  title: string;
  schedule: string;
  prompt: string;
  enabled: boolean;
}

/// Best-effort human description of a standard 5-field cron expression
/// (`min hour day-of-month month day-of-week`). Unknown shapes return the
/// raw expression unchanged rather than guessing wrong.
function describeCron(expr: string, t: Translate): string {
  const parts = expr.trim().split(/\s+/);
  if (parts.length !== 5) return t("automation.cronInvalid");
  const [minute, hour] = parts;
  if (minute === "*" && hour === "*") return t("automation.cronEveryMinute");
  if (minute === "0" && hour === "*") return t("automation.cronEveryHour");
  if (minute === "0" && /^\d+$/.test(hour)) return t("automation.cronDailyAt", { hour });
  return t("automation.cronCustom");
}

export function AutomationsPage() {
  const { t } = useI18n();
  const automationTemplates = [
    ["nightly", t("automation.templateNightly"), t("automation.daily"), t("automation.templateNightlyPrompt")],
    ["review", t("automation.templateReview"), t("automation.weekdays"), t("automation.templateReviewPrompt")],
    ["radar", t("automation.templateRadar"), t("automation.weekly"), t("automation.templateRadarPrompt")],
  ] as const;
  const [automations, setAutomations] = useState<AutomationItem[]>([]);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [schedule, setSchedule] = useState("daily");
  const [cron, setCron] = useState("0 9 * * *");
  const [prompt, setPrompt] = useState("");

  function useTemplate(template: (typeof automationTemplates)[number]) {
    setTitle(template[1]);
    setPrompt(template[3]);
    setOpen(true);
    // A template's daily/weekdays/weekly schedule is a selectable preset, not
    // a cron expression; the custom cron field stays independent.
    if (template[0] === "review") setSchedule("weekdays");
    else if (template[0] === "radar") setSchedule("weekly");
    else setSchedule("daily");
  }

  function effectiveSchedule(): string {
    // A custom cron overrides the passive preset; otherwise the preset label
    // doubles as the stored schedule.
    return schedule === "cron" ? `cron:${cron}` : schedule;
  }

  function createAutomation() {
    const normalizedTitle = title.trim();
    const normalizedPrompt = prompt.trim();
    if (!normalizedTitle || !normalizedPrompt) return;
    if (schedule === "cron" && !cron.trim()) return;
    setAutomations((current) => [
      ...current,
      {
        id: `${Date.now()}-${normalizedTitle}`,
        title: normalizedTitle,
        schedule: effectiveSchedule(),
        prompt: normalizedPrompt,
        enabled: true,
      },
    ]);
    setOpen(false);
    setTitle("");
    setPrompt("");
    setSchedule("daily");
    setCron("0 9 * * *");
  }

  return (
    <ScrollRegion className="hub-product-page">
      <PageHeader
        actions={
          <DialogRoot open={open} onOpenChange={setOpen}>
            <DialogTrigger asChild>
              <Button leadingIcon={<Plus aria-hidden="true" size={15} />} variant="primary">
                {t("automation.new")}
              </Button>
            </DialogTrigger>
            <DialogContent
              description={t("automation.dialogDescription")}
              size="wide"
              title={t("automation.create")}
            >
              <div className="hub-form-stack">
                <FieldFrame label={t("automation.title")}>
                  <TextField aria-label={t("automation.titleAria")} value={title} onChange={(event) => setTitle(event.target.value)} />
                </FieldFrame>
                <FieldFrame label={t("automation.schedule")}>
                  <SelectField
                    ariaLabel={t("automation.scheduleAria")}
                    onValueChange={setSchedule}
                    options={[
                      { value: "daily", label: t("automation.daily") },
                      { value: "weekdays", label: t("automation.weekdays") },
                      { value: "weekly", label: t("automation.weekly") },
                      { value: "cron", label: t("automation.cron") },
                    ]}
                    value={schedule}
                  />
                </FieldFrame>
                {schedule === "cron" ? (
                  <FieldFrame label={t("automation.cronExpression")} description={describeCron(cron, t)}>
                    <TextField aria-label={t("automation.cronExpressionAria")} value={cron} onChange={(event) => setCron(event.target.value)} placeholder="0 9 * * *" />
                  </FieldFrame>
                ) : null}
                <FieldFrame label={t("automation.prompt")}>
                  <TextArea aria-label={t("automation.promptAria")} value={prompt} onChange={(event) => setPrompt(event.target.value)} />
                </FieldFrame>
                <div className="hub-dialog-actions">
                  <Button onClick={() => setOpen(false)} variant="ghost">{t("common.cancel")}</Button>
                  <Button disabled={!title.trim() || !prompt.trim()} onClick={createAutomation} variant="primary">{t("common.create")}</Button>
                </div>
              </div>
            </DialogContent>
          </DialogRoot>
        }
        description={t("automation.description")}
        meta={t("automation.configured", { count: automations.length })}
        title={t("nav.automations")}
      />
      {automations.length === 0 ? (
        <EmptyState
          icon={<Workflow aria-hidden="true" size={18} />}
          title={t("automation.empty")}
          description={t("automation.emptyDescription")}
        />
      ) : (
        <div className="hub-product-list">
          {automations.map((automation) => (
            <ListRow
              actions={
                <Switch
                  checked={automation.enabled}
                  label={t("automation.enable", { title: automation.title })}
                  onCheckedChange={(checked) =>
                    setAutomations((current) =>
                      current.map((candidate) =>
                        candidate.id === automation.id
                          ? { ...candidate, enabled: checked === true }
                          : candidate,
                      ),
                    )
                  }
                />
              }
              description={automation.prompt}
              heading={automation.title}
              key={automation.id}
              leading={<CalendarClock aria-hidden="true" size={17} />}
              meta={automation.schedule.startsWith("cron:") ? t("automation.cronLabel", { expr: automation.schedule.slice(5) }) : automation.schedule === "daily" ? t("automation.daily") : automation.schedule === "weekdays" ? t("automation.weekdays") : t("automation.weekly")}
            />
          ))}
        </div>
      )}
      <section className="hub-product-section">
        <h3>{t("automation.templates")}</h3>
        <CardGrid>
          {automationTemplates.map((template) => (
            <StatusListItem
              className="hub-template-card"
              description={template[3]}
              heading={template[1]}
              key={template[0]}
              leading={<CalendarClock aria-hidden="true" size={18} />}
              onClick={() => useTemplate(template)}
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
  const extensions = [
    { id: "github", name: "GitHub", description: t("extension.githubDescription"), icon: GitBranch, installed: true, kind: "plugin" },
    { id: "maker", name: "TapTap Maker", description: t("extension.makerDescription"), icon: Sparkles, installed: true, kind: "plugin" },
    { id: "review", name: t("extension.review"), description: t("extension.reviewDescription"), icon: Check, installed: true, kind: "skill" },
    { id: "design", name: t("extension.design"), description: t("extension.designDescription"), icon: Blocks, installed: false, kind: "skill" },
  ] as const;
  const [tab, setTab] = useState("plugin");
  const [query, setQuery] = useState("");
  const [installed, setInstalled] = useState<Set<string>>(
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
              description={t("extension.addDescription")}
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
                  <Button
                    disabled={!extensionName.trim()}
                    onClick={() => {
                      setExtensionName("");
                      setAddOpen(false);
                    }}
                    variant="primary"
                  >
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
                      leadingIcon={<Download aria-hidden="true" size={14} />}
                      onClick={() => setInstalled((current) => new Set(current).add(extension.id))}
                      size="small"
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

const settingsSections = ["general", "models", "usage", "tools", "automations", "experiments", "help"] as const;

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
  const [compact, setCompact] = useState(false);
  const [inspector, setInspector] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);

  return (
    <div className="hub-settings-frame">
      <nav aria-label={t("settings.sections")} className="hub-settings-nav">
        {settingsSections.map((candidate) => (
          <NavItem active={section === candidate} key={candidate} label={t(`settings.${candidate}`)} onClick={() => setSection(candidate)} />
        ))}
      </nav>
      <ScrollRegion className="hub-product-page hub-settings-page">
        <PageHeader description={t("settings.description")} title={t(`settings.${section}`)} />
        {section === "general" ? (
          <>
            <SettingsGroup heading={t("settings.appearance")} description={t("settings.appearanceDescription")}>
              <SettingsRow heading={t("settings.theme")} description={t("settings.alvaDark")} control={<Badge tone="accent">{t("settings.dark")}</Badge>} />
              <SettingsRow heading={t("settings.density")} description={t("settings.densityDescription")} control={<Switch checked={compact} label={t("settings.useCompact")} onCheckedChange={(checked) => setCompact(checked === true)} />} />
              <SettingsRow heading={t("settings.reduceMotion")} description={t("settings.reduceMotionDescription")} control={<Switch checked={reducedMotion} label={t("settings.reduceMotion")} onCheckedChange={(checked) => setReducedMotion(checked === true)} />} />
            </SettingsGroup>
            <SettingsGroup heading={t("settings.languageGroup")}>
              <SettingsRow heading={t("settings.language")} description={t("settings.languageDescription")} control={<SelectField ariaLabel={t("settings.language")} onValueChange={(value) => setPreference(value as LanguagePreference)} options={[{ value: "zh-CN", label: t("settings.chinese") }, { value: "en-US", label: t("settings.english") }, { value: "system", label: t("settings.systemLanguage") }]} value={preference} />} />
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
        ) : section === "experiments" ? (
          <SettingsGroup heading={t("settings.developerSurfaces")} description={t("settings.developerDescription")}>
            <SettingsRow heading={t("settings.agentInspector")} description={t("settings.agentInspectorDescription")} control={<Switch checked={inspector} label={t("settings.showInspector")} onCheckedChange={(checked) => setInspector(checked === true)} />} />
            <SettingsRow heading={t("settings.uiLab")} description={t("settings.uiLabDescription")} control={<Badge>{t("settings.developmentOnly")}</Badge>} />
          </SettingsGroup>
        ) : (
          <SettingsGroup heading={t(`settings.${section}`)} description={t("settings.description")}>
            <SettingsRow heading={t("settings.status")} description={t("settings.noAdditionalConfig")} control={<Badge tone="success">{t("status.ready")}</Badge>} />
            <SettingsRow heading={t("settings.learnMore")} description={t("settings.learnMoreDescription")} control={<CircleHelp aria-hidden="true" size={16} />} />
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
