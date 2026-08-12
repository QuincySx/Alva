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

export type HubPage = "conversation" | "automations" | "extensions" | "settings" | "search";

interface AutomationItem {
  id: string;
  title: string;
  schedule: string;
  prompt: string;
  enabled: boolean;
}

const automationTemplates = [
  ["nightly", "Nightly self-healing tests", "Every day · 02:00", "Run tests, fix failures in an isolated worktree, and report verification."],
  ["review", "Pull request gatekeeper", "Weekdays · 09:00", "Review new changes and report correctness, security, and maintainability risks."],
  ["radar", "Domain radar", "Every Monday", "Collect important ecosystem changes and summarize their impact on this project."],
] as const;

export function AutomationsPage() {
  const [automations, setAutomations] = useState<AutomationItem[]>([]);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [schedule, setSchedule] = useState("daily");
  const [prompt, setPrompt] = useState("");

  function useTemplate(template: (typeof automationTemplates)[number]) {
    setTitle(template[1]);
    setPrompt(template[3]);
    setOpen(true);
  }

  function createAutomation() {
    const normalizedTitle = title.trim();
    const normalizedPrompt = prompt.trim();
    if (!normalizedTitle || !normalizedPrompt) return;
    setAutomations((current) => [
      ...current,
      {
        id: `${Date.now()}-${normalizedTitle}`,
        title: normalizedTitle,
        schedule,
        prompt: normalizedPrompt,
        enabled: true,
      },
    ]);
    setOpen(false);
    setTitle("");
    setPrompt("");
  }

  return (
    <ScrollRegion className="hub-product-page">
      <PageHeader
        actions={
          <DialogRoot open={open} onOpenChange={setOpen}>
            <DialogTrigger asChild>
              <Button leadingIcon={<Plus aria-hidden="true" size={15} />} variant="primary">
                New automation
              </Button>
            </DialogTrigger>
            <DialogContent
              description="Run a prompt on a predictable schedule. Templates only prefill this form."
              size="wide"
              title="Create automation"
            >
              <div className="hub-form-stack">
                <FieldFrame label="Title">
                  <TextField aria-label="Automation title" value={title} onChange={(event) => setTitle(event.target.value)} />
                </FieldFrame>
                <FieldFrame label="Schedule">
                  <SelectField
                    ariaLabel="Automation schedule"
                    onValueChange={setSchedule}
                    options={[
                      { value: "daily", label: "Daily · 02:00" },
                      { value: "weekdays", label: "Weekdays · 09:00" },
                      { value: "weekly", label: "Every Monday" },
                    ]}
                    value={schedule}
                  />
                </FieldFrame>
                <FieldFrame label="Prompt">
                  <TextArea aria-label="Automation prompt" value={prompt} onChange={(event) => setPrompt(event.target.value)} />
                </FieldFrame>
                <div className="hub-dialog-actions">
                  <Button onClick={() => setOpen(false)} variant="ghost">Cancel</Button>
                  <Button disabled={!title.trim() || !prompt.trim()} onClick={createAutomation} variant="primary">Create</Button>
                </div>
              </div>
            </DialogContent>
          </DialogRoot>
        }
        description="Let an Agent run a prompt on a schedule without turning setup into a separate workflow."
        meta={`${automations.length} configured`}
        title="Automations"
      />
      {automations.length === 0 ? (
        <EmptyState
          icon={<Workflow aria-hidden="true" size={18} />}
          title="No automations yet"
          description="Choose a template below or create one from scratch. Nothing runs until you confirm the form."
        />
      ) : (
        <div className="hub-product-list">
          {automations.map((automation) => (
            <ListRow
              actions={
                <Switch
                  checked={automation.enabled}
                  label={`Enable ${automation.title}`}
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
              meta={automation.schedule}
            />
          ))}
        </div>
      )}
      <section className="hub-product-section">
        <h3>Start from a template</h3>
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

const extensions = [
  { id: "github", name: "GitHub", description: "Repositories, pull requests, and issues", icon: GitBranch, installed: true, kind: "plugin" },
  { id: "maker", name: "TapTap Maker", description: "Build and preview interactive projects", icon: Sparkles, installed: true, kind: "plugin" },
  { id: "review", name: "Review", description: "Review every change before landing", icon: Check, installed: true, kind: "skill" },
  { id: "design", name: "Design engineering", description: "Structure, motion, and visual review guidance", icon: Blocks, installed: false, kind: "skill" },
] as const;

export function ExtensionsPage({ onStartChat }: { onStartChat: () => void }) {
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
              <Button leadingIcon={<Plus aria-hidden="true" size={15} />}>Add extension</Button>
            </DialogTrigger>
            <DialogContent
              description="Register an extension source. Installation remains explicit after registration."
              title="Add extension"
            >
              <div className="hub-form-stack">
                <FieldFrame label="Name or source">
                  <TextField
                    aria-label="Extension name or source"
                    onChange={(event) => setExtensionName(event.target.value)}
                    placeholder="owner/repository or local path"
                    value={extensionName}
                  />
                </FieldFrame>
                <div className="hub-dialog-actions">
                  <Button onClick={() => setAddOpen(false)} variant="ghost">Cancel</Button>
                  <Button
                    disabled={!extensionName.trim()}
                    onClick={() => {
                      setExtensionName("");
                      setAddOpen(false);
                    }}
                    variant="primary"
                  >
                    Register
                  </Button>
                </div>
              </div>
            </DialogContent>
          </DialogRoot>
        }
        description="Add tools and instructions without changing the Hub shell."
        title="Plugins and skills"
      />
      <div className="hub-product-toolbar">
        <TabsRoot onValueChange={setTab} value={tab}>
          <TabsList aria-label="Extension type">
            <TabsTrigger value="plugin">Plugins</TabsTrigger>
            <TabsTrigger value="skill">Skills</TabsTrigger>
          </TabsList>
        </TabsRoot>
        <label className="hub-search-field">
          <Search aria-hidden="true" size={16} />
          <TextField aria-label="Search extensions" placeholder="Search extensions…" value={query} onChange={(event) => setQuery(event.target.value)} />
        </label>
      </div>
      {visible.length === 0 ? (
        <EmptyState title="No matching extensions" description="Try another name or description." />
      ) : (
        <CardGrid>
          {visible.map((extension) => {
            const Icon = extension.icon;
            const isInstalled = installed.has(extension.id);
            return (
              <article className="hub-extension-card" key={extension.id}>
                <div className="hub-extension-card__heading">
                  <span><Icon aria-hidden="true" size={18} /></span>
                  <div><strong>{extension.name}</strong><small>{extension.kind}</small></div>
                  <Badge tone={isInstalled ? "success" : "neutral"}>{isInstalled ? "Installed" : "Available"}</Badge>
                </div>
                <p>{extension.description}</p>
                <div className="hub-extension-card__actions">
                  {isInstalled ? (
                    <Button onClick={onStartChat} size="small" variant="ghost">Use in chat</Button>
                  ) : (
                    <Button
                      leadingIcon={<Download aria-hidden="true" size={14} />}
                      onClick={() => setInstalled((current) => new Set(current).add(extension.id))}
                      size="small"
                    >
                      Install
                    </Button>
                  )}
                  <Button onClick={() => setSelectedExtension(extension)} size="small" variant="ghost">Details</Button>
                </div>
              </article>
            );
          })}
        </CardGrid>
      )}
      <DialogRoot open={selectedExtension !== null} onOpenChange={(open) => { if (!open) setSelectedExtension(null); }}>
        <DialogContent
          description={selectedExtension?.description}
          title={selectedExtension?.name ?? "Extension details"}
        >
          {selectedExtension ? (
            <SettingsGroup heading="Availability">
              <SettingsRow
                control={<Badge tone={installed.has(selectedExtension.id) ? "success" : "neutral"}>{installed.has(selectedExtension.id) ? "Installed" : "Available"}</Badge>}
                description="Extensions are activated per Hub installation and surfaced to compatible agents."
                heading={selectedExtension.kind === "plugin" ? "Plugin" : "Skill"}
              />
            </SettingsGroup>
          ) : null}
        </DialogContent>
      </DialogRoot>
    </ScrollRegion>
  );
}

const settingsSections = ["General", "Models and engines", "Usage", "Tools", "Automations", "Experiments", "Help"] as const;

export function SettingsPage() {
  const [section, setSection] = useState<(typeof settingsSections)[number]>("General");
  const [compact, setCompact] = useState(false);
  const [inspector, setInspector] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);

  return (
    <div className="hub-settings-frame">
      <nav aria-label="Settings sections" className="hub-settings-nav">
        {settingsSections.map((candidate) => (
          <NavItem active={section === candidate} key={candidate} label={candidate} onClick={() => setSection(candidate)} />
        ))}
      </nav>
      <ScrollRegion className="hub-product-page hub-settings-page">
        <PageHeader description="Settings use the same controls and row rhythm as the rest of Hub." title={section} />
        {section === "General" ? (
          <>
            <SettingsGroup heading="Appearance" description="Dark is the current visual baseline; semantic tokens remain ready for light mode.">
              <SettingsRow heading="Theme" description="ALVA dark" control={<Badge tone="accent">Dark</Badge>} />
              <SettingsRow heading="Interface density" description="Compact affects navigation, menus, model lists, and Workbench tools." control={<Switch checked={compact} label="Use compact density" onCheckedChange={(checked) => setCompact(checked === true)} />} />
              <SettingsRow heading="Reduce motion" description="Remove displacement while preserving useful state feedback." control={<Switch checked={reducedMotion} label="Reduce motion" onCheckedChange={(checked) => setReducedMotion(checked === true)} />} />
            </SettingsGroup>
            <SettingsGroup heading="Language and copy">
              <SettingsRow heading="Interface language" description="Layouts are verified with English and Chinese copy." control={<SelectField ariaLabel="Interface language" defaultValue="system" options={[{ value: "system", label: "System default" }, { value: "en", label: "English" }, { value: "zh", label: "简体中文" }]} />} />
            </SettingsGroup>
          </>
        ) : section === "Experiments" ? (
          <SettingsGroup heading="Developer surfaces" description="Experimental switches only change visibility, not trace retention.">
            <SettingsRow heading="Agent Inspector" description="Show the Inspector tab for HubSession execution details." control={<Switch checked={inspector} label="Show Agent Inspector" onCheckedChange={(checked) => setInspector(checked === true)} />} />
            <SettingsRow heading="UI Lab" description="Available only in development and test builds." control={<Badge>Development only</Badge>} />
          </SettingsGroup>
        ) : (
          <SettingsGroup heading={section} description="This section uses the shared SettingsGroup and SettingsRow contract.">
            <SettingsRow heading="Status" description="No additional configuration is required for the current workspace." control={<Badge tone="success">Ready</Badge>} />
            <SettingsRow heading="Learn more" description="Open Help from the settings navigation for product guidance." control={<CircleHelp aria-hidden="true" size={16} />} />
          </SettingsGroup>
        )}
      </ScrollRegion>
    </div>
  );
}

export function SearchPage({ sessions, onOpenSession, onNavigate, onOpenWorkbench }: { sessions: Array<{ id: string; title: string; description: string }>; onOpenSession: (id: string) => void; onNavigate: (page: HubPage) => void; onOpenWorkbench: (tool: "inspector" | "usage") => void }) {
  const [query, setQuery] = useState("");
  const results = useMemo(() => sessions.filter((session) => `${session.title} ${session.description}`.toLowerCase().includes(query.trim().toLowerCase())), [query, sessions]);
  return (
    <ScrollRegion className="hub-product-page hub-search-page">
      <PageHeader description="Search sessions, projects, settings, and commands from one keyboard-first surface." title="Search" />
      <label className="hub-search-field hub-search-field--large">
        <Search aria-hidden="true" size={18} />
        <TextField autoFocus aria-label="Search Hub" placeholder="Search Hub…" value={query} onChange={(event) => setQuery(event.target.value)} />
        <kbd>⌘ K</kbd>
      </label>
      <section className="hub-product-section">
        <h3>{query ? "Results" : "Quick actions"}</h3>
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
              <StatusListItem heading="Open settings" description="Models, tools, experiments, and appearance" leading={<Settings2 aria-hidden="true" size={17} />} onClick={() => onNavigate("settings")} trailing={<Command aria-hidden="true" size={14} />} />
              <StatusListItem heading="Inspect current session" description="Open execution details in the right Workbench" leading={<FlaskConical aria-hidden="true" size={17} />} onClick={() => onOpenWorkbench("inspector")} />
              <StatusListItem heading="Review usage" description="See token, cost, and runtime summaries" leading={<Gauge aria-hidden="true" size={17} />} onClick={() => onOpenWorkbench("usage")} />
            </>
          )}
        </div>
      </section>
    </ScrollRegion>
  );
}
