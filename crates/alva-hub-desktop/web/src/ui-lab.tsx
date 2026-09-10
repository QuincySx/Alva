// INPUT:  ALVA UI facade, deterministic fixture content, and domain components
// OUTPUT: Development/test-only component and page state catalog
// POS:    Reproducible visual acceptance surface excluded from production navigation

import { useState } from "react";
import { AlertTriangle, Check, FolderTree, Plus, Search } from "lucide-react";
import {
  Badge,
  Button,
  CardGrid,
  Checkbox,
  DialogContent,
  DialogRoot,
  DialogTrigger,
  Disclosure,
  EmptyState,
  FieldFrame,
  ListRow,
  Notice,
  PageHeader,
  PanelSection,
  PopoverContent,
  PopoverRoot,
  PopoverTrigger,
  SelectField,
  SettingsGroup,
  SettingsRow,
  StatusListItem,
  Switch,
  TabsContent,
  TabsList,
  TabsRoot,
  TabsTrigger,
  TextArea,
  TextField,
  UIProvider,
} from "./ui";
import { ModelPicker, type HubModelRoute } from "./components/model-picker";

const routes: HubModelRoute[] = [
  { engineId: "alva", engineLabel: "ALVA", modelId: "workspace", modelLabel: "Workspace model", providerId: "workspace", accountSource: "Workspace configuration", billingRoute: "Workspace provider", description: "Uses the project model configuration.", capabilities: ["tools", "images"], reasoningEfforts: ["low", "medium", "high"], reasoningEffort: "high", supportsFastMode: false, fastMode: false },
  { engineId: "claude", engineLabel: "Claude Code", modelId: "sonnet", modelLabel: "Sonnet", providerId: "subscription", accountSource: "Claude Code subscription", billingRoute: "Subscription", description: "Balanced coding model.", capabilities: ["tools", "images", "thinking"], reasoningEfforts: ["low", "medium", "high", "xhigh", "max"], reasoningEffort: "high", supportsFastMode: false, fastMode: false },
  { engineId: "codex", engineLabel: "Codex", modelId: "gpt", modelLabel: "GPT Codex", providerId: "subscription", accountSource: "Codex subscription", billingRoute: "Subscription", description: "OpenAI coding route.", reasoningEfforts: ["minimal", "low", "medium", "high", "xhigh"], reasoningEffort: "high", supportsFastMode: true, fastMode: false, disabled: true, disabledReason: "Sign in to Codex before selecting this model." },
];

type TokenRow = { role: string; token: string; sample?: string };

const typeTokenRows: TokenRow[] = [
  { role: "Page title", token: "--font-size-page-title · 700", sample: "20px" },
  { role: "Title", token: "--font-size-title · 700", sample: "18px" },
  { role: "Section title", token: "--font-size-section-title · 650", sample: "15px" },
  { role: "Body", token: "--font-size-body · 400", sample: "14px" },
  { role: "Body small", token: "--font-size-body-sm · 400", sample: "13px" },
  { role: "Caption", token: "--font-size-caption · 400", sample: "12px" },
  { role: "Micro / auxiliary", token: "--font-size-micro · subtle", sample: "11px" },
  { role: "Overline", token: "--font-size-overline · caps 650", sample: "10px" },
];

const surfaceTokenRows: TokenRow[] = [
  { role: "Canvas", token: "--color-canvas" },
  { role: "Sidebar", token: "--color-sidebar" },
  { role: "Panel", token: "--color-surface" },
  { role: "Raised", token: "--color-surface-raised" },
  { role: "Sunken inset", token: "--color-surface-sunken" },
  { role: "Hover", token: "--color-surface-hover" },
  { role: "Border", token: "--color-border / strong" },
];

const statusTokenRows: TokenRow[] = [
  { role: "Success", token: "--color-success / -bg / -border" },
  { role: "Warning", token: "--color-warning / -bg / -border" },
  { role: "Danger", token: "--color-danger / -bg / -border" },
  { role: "Focus", token: "--color-focus" },
];

export default function UiLab() {
  const [selected, setSelected] = useState(routes[0]);
  return (
    <UIProvider>
      <main className="ui-lab" data-testid="ui-lab">
        <PageHeader description="Deterministic component fixtures for visual, keyboard, and motion review." meta="Development only" title="ALVA UI Lab" />
        <TabsRoot className="ui-lab__tabs" defaultValue="controls">
          <TabsList aria-label="UI Lab sections">
            <TabsTrigger value="controls">Controls</TabsTrigger>
            <TabsTrigger value="tokens">Tokens</TabsTrigger>
            <TabsTrigger value="surfaces">Surfaces</TabsTrigger>
            <TabsTrigger value="model">Model picker</TabsTrigger>
          </TabsList>
          <TabsContent value="tokens">
            <section className="ui-lab__section">
              <h2>Typography roles</h2>
              <div className="ui-token-table">
                {typeTokenRows.map((row) => (
                  <div className="ui-token-table__row" key={row.role}>
                    <span className="ui-token-table__sample" style={{ fontSize: `var(${row.token.split(" ")[0]})` }}>
                      {row.role} — 你好 world
                    </span>
                    <code>{row.token}</code>
                  </div>
                ))}
              </div>
            </section>
            <section className="ui-lab__section">
              <h2>Surface roles</h2>
              <div className="ui-token-table">
                {surfaceTokenRows.map((row) => (
                  <div className="ui-token-table__row" key={row.role}>
                    <span className="ui-token-table__swatch" style={{ background: `var(${row.token.split(" ")[0]})` }} />
                    <span className="ui-token-table__sample">{row.role}</span>
                    <code>{row.token}</code>
                  </div>
                ))}
              </div>
            </section>
            <section className="ui-lab__section">
              <h2>Status roles</h2>
              <div className="ui-token-table">
                {statusTokenRows.map((row) => (
                  <div className="ui-token-table__row" key={row.role}>
                    <span className="ui-token-table__sample">{row.role}</span>
                    <code>{row.token}</code>
                  </div>
                ))}
                <div className="ui-token-table__row ui-token-table__row--status">
                  <Badge tone="success">Success badge</Badge>
                  <Badge tone="warning">Warning badge</Badge>
                  <Badge tone="danger">Danger badge</Badge>
                </div>
              </div>
            </section>
          </TabsContent>
          <TabsContent value="controls">
            <section className="ui-lab__section">
              <h2>Actions and fields</h2>
              <div className="ui-lab__row">
                <Button variant="primary">Primary</Button><Button>Secondary</Button><Button variant="ghost">Ghost</Button><Button variant="danger">Danger</Button><Button loading>Loading</Button>
              </div>
              <div className="ui-lab__row"><Button density="compact">Compact</Button><Checkbox label="Authorize automatic use" defaultChecked /><Switch label="Show Inspector" defaultChecked /></div>
              <CardGrid>
                <FieldFrame label="Project name"><TextField defaultValue="alva-agent" /></FieldFrame>
                <FieldFrame label="Runtime"><SelectField ariaLabel="Runtime" defaultValue="alva" options={[{ value: "alva", label: "ALVA" }, { value: "codex", label: "Codex", disabled: true }]} /></FieldFrame>
              </CardGrid>
              <FieldFrame label="Prompt"><TextArea defaultValue="Review the component hierarchy and motion behavior." /></FieldFrame>
              <div className="ui-lab__row">
                <PopoverRoot><PopoverTrigger asChild><Button>Open popover</Button></PopoverTrigger><PopoverContent size="compact">Anchored content</PopoverContent></PopoverRoot>
                <DialogRoot><DialogTrigger asChild><Button>Open dialog</Button></DialogTrigger><DialogContent description="Dialogs share size and motion variants." size="compact" title="Confirm action"><Button variant="primary">Confirm</Button></DialogContent></DialogRoot>
              </div>
            </section>
          </TabsContent>
          <TabsContent value="surfaces">
            <section className="ui-lab__section">
              <h2>Regions and surfaces</h2>
              <Notice actions={<><Button size="small" variant="primary">Allow</Button><Button size="small" variant="ghost">Deny</Button></>} description="Two agents need access to src/queue.rs." heading="Allow bash?" tone="info"><small>worker-demo-1 · worker-demo-2</small></Notice>
              <CardGrid>
                <PanelSection heading="Readiness"><StatusListItem heading="ALVA" leading={<Check size={16} />} status={<Badge tone="success">Ready</Badge>} /><StatusListItem description="Sign in before use" heading="Codex" leading={<AlertTriangle size={16} />} status={<Badge tone="warning">Unavailable</Badge>} unavailable /></PanelSection>
                <SettingsGroup heading="Appearance"><SettingsRow control={<Switch label="Compact density" />} heading="Density" /><SettingsRow control={<Badge>Dark</Badge>} heading="Theme" /></SettingsGroup>
              </CardGrid>
              <Disclosure description="Account, provider, and billing" title="Advanced details"><ListRow heading="Provider" meta="Workspace" /></Disclosure>
              <EmptyState icon={<FolderTree size={18} />} title="No files yet" description="Changed files appear here." action={<Button leadingIcon={<Plus size={14} />}>Add</Button>} />
            </section>
          </TabsContent>
          <TabsContent value="model">
            <section className="ui-lab__section ui-lab__model">
              <h2>Composer model route</h2>
              <label className="hub-search-field"><Search size={16} /><TextField aria-label="Lab search" placeholder="Search models…" /></label>
              <ModelPicker onOptionsChange={(route, update) => setSelected({ ...route, reasoningEffort: update.reasoningEffort ?? route.reasoningEffort, fastMode: update.fastMode ?? route.fastMode })} onSelect={setSelected} routes={routes} selectedEngineId={selected.engineId} selectedModelId={selected.modelId} selectedProviderId={selected.providerId} />
            </section>
          </TabsContent>
        </TabsRoot>
      </main>
    </UIProvider>
  );
}
