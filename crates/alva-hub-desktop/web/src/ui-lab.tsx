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
  { engineId: "alva", engineLabel: "ALVA", modelId: "workspace", modelLabel: "Workspace model", providerId: "workspace", accountSource: "Workspace configuration", billingRoute: "Workspace provider", description: "Uses the project model configuration.", capabilities: ["tools", "images"] },
  { engineId: "claude", engineLabel: "Claude Code", modelId: "sonnet", modelLabel: "Sonnet", providerId: "subscription", accountSource: "Claude Code subscription", billingRoute: "Subscription", description: "Balanced coding model.", capabilities: ["tools", "images", "thinking"] },
  { engineId: "codex", engineLabel: "Codex", modelId: "gpt", modelLabel: "GPT Codex", providerId: "subscription", accountSource: "Codex subscription", billingRoute: "Subscription", description: "OpenAI coding route.", disabled: true, disabledReason: "Sign in to Codex before selecting this model." },
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
            <TabsTrigger value="surfaces">Surfaces</TabsTrigger>
            <TabsTrigger value="model">Model picker</TabsTrigger>
          </TabsList>
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
              <ModelPicker onSelect={setSelected} routes={routes} selectedEngineId={selected.engineId} selectedModelId={selected.modelId} />
            </section>
          </TabsContent>
        </TabsRoot>
      </main>
    </UIProvider>
  );
}
