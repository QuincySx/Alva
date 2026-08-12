# ALVA Hub UI Wireframe Specification

> Status: Accepted structure for redrawing the seven aggregate Cindy-derived
> wireframes. The 67 existing state lineworks remain unchanged as evidence.

## Drawing Rules

Every aggregate wireframe uses the same canvas family and labels:

- Region boundaries use solid outer frames.
- Surfaces use lighter inner frames.
- Controls are named but not visually styled beyond type and density.
- Scroll owners carry a vertical `SCROLL` annotation.
- Docked regions carry a `DOCK` annotation and edge.
- Overlay triggers connect to their Overlay using a directional line.
- Width ownership is labeled on the owning Region, never on each child.
- Responsive variants show relationship changes, not squeezed desktop content.
- User-visible content order matches the Cindy reference.
- ALVA-specific Agent and Inspector meanings follow the Hub glossary even when
  Cindy names differ.

Each wireframe includes desktop-docked and narrow-overlay frames where the
relationship changes.

## Shared Application Layer

```text
AppFrame
├─ Sidebar
├─ Main
│  ├─ PaneHeader
│  ├─ ScrollRegion [SCROLL]
│  └─ DockRegion [DOCK]
└─ Workbench [DOCK desktop | OVERLAY narrow]
```

`AppFrame` owns the three-region relationship. `ContentFrame` inside Main owns
the common horizontal axis used by timeline, notices, approvals, and Composer.

## 01 Chat Shell

```text
AppFrame
├─ HubSidebar
│  ├─ Brand
│  ├─ PrimaryNavigation
│  ├─ SessionSearch
│  ├─ ProjectGroup
│  │  └─ SessionItem[]
│  └─ WorkspaceFooter
├─ Main
│  ├─ HubTopbar
│  ├─ ScrollRegion [SCROLL]
│  │  └─ HubTimeline
│  │     ├─ UserMessage
│  │     ├─ AgentMessage
│  │     ├─ ActivityRow / Disclosure
│  │     ├─ PermissionNotice
│  │     └─ FallbackNotice
│  └─ DockRegion [DOCK]
│     ├─ PersistentNotice[]
│     └─ HubComposer
└─ Workbench (optional)
```

The chat uses one reading axis. Approval, fallback, and handoff state do not
form independent pages or unrelated card columns. Composer remains docked while
Timeline owns conversation scrolling.

## 02 New Conversation

```text
Main
├─ HubTopbar
├─ ScrollRegion
│  └─ EmptyConversation
│     ├─ Prompt
│     └─ QuickStart[]
└─ DockRegion
   └─ HubComposer
      ├─ ContextTrigger ──> ContextPopover
      ├─ PermissionTrigger ──> PermissionMenu
      ├─ AgentTrigger ──> AgentMenu
      └─ ModelTrigger ──> ModelPicker
```

All new-conversation choices are reachable from Composer without leaving the
conversation. ModelPicker owns Engine tabs, search, available/unavailable model
groups, selected route, and progressive details.

## 03 Right Workbench

```text
Workbench
├─ WorkbenchTabBar
│  ├─ OpenToolTab[]
│  │  ├─ ToolIcon
│  │  ├─ ToolLabel
│  │  └─ CloseTab
│  ├─ AddTab ──> ToolMenu
│  └─ CloseWorkbench
└─ WorkbenchPage [SCROLL]
   └─ ActiveTool
```

The Workbench is a resizable split Region, not a permanently expanded list of
tools. Only opened tools appear as tabs. `AddTab` opens a menu containing Files,
Text, Git, Agent, Inspector, Tasks, Usage, Chrome, Terminal, Engines, Fallback,
and Team. Selecting a menu item opens and activates that tab; tabs switch and
close independently. Agent shows child-agent relationships. Inspector shows
execution-debug detail. On narrow windows the whole Workbench becomes a Sheet;
tab pages do not become multiple floating cards.

## 04 Automations

```text
MainPage
├─ PageHeader
├─ PageTabs / Filters
├─ AutomationList or TemplateGrid [SCROLL]
│  ├─ AutomationRow[]
│  └─ TemplateCard[]
└─ AutomationDialog [OVERLAY]
   ├─ DialogHeader
   ├─ AutomationForm
   └─ DialogFooter
```

Templates prefill a form; they do not silently create an automation. Empty,
loading, disabled, and failure states use shared Surfaces.

## 05 Plugins and Skills

```text
MainPage
├─ PageHeader
├─ PageTabs (Plugins | Skills)
├─ SearchField
├─ ExtensionGrid [SCROLL]
│  └─ ExtensionCard[]
└─ ExtensionDetail [OVERLAY or WorkbenchPage]
   ├─ Summary
   ├─ Availability
   ├─ Permissions
   ├─ Settings
   └─ Actions
```

Plugins and Skills share the page skeleton and Surface vocabulary. Availability
and the reason for unavailability are visible before technical details.

## 06 Settings

```text
SettingsFrame
├─ SettingsNav
└─ SettingsPage [SCROLL]
   ├─ PageHeader
   └─ SettingsGroup[]
      └─ SettingsRow[]
```

Settings groups reuse Fields, Switches, StatusListItems, and PanelSections.
Models/Engines, Usage, Tools, Automations, Experiments, and Inspector visibility
do not create separate primitive styles.

## 07 Context Menus

```text
ProjectTrigger ──> ProjectMenu
SessionTrigger ──> SessionMenu
FolderTrigger ──> FolderPicker
DangerAction ──> DangerConfirmation
```

Each Overlay names its trigger, anchor side, width variant, item density,
keyboard path, destructive action, and close path. Context menus never depend
on a hard-coded screen coordinate.

## Global Search Surface

Search is a shared Overlay or Main surface used from Sidebar and keyboard
commands. It reuses TextField, ListRow, Badge, EmptyState, and ScrollRegion. It
does not introduce an eighth application skeleton and keyboard activation does
not add positional animation.

## Required Responsive Frames

| Viewport | Required relationship |
|---|---|
| ≥ 1360 px | Sidebar + Main + docked resizable Workbench |
| 1024–1359 px | Narrower Sidebar; docked Workbench adjusts Main as one Region |
| 760–1023 px | Workbench is an overlay Sheet; Main is not compressed twice |
| < 760 px | Sidebar and Workbench are overlays; Composer may use two control rows |

These are Region rules. Individual pages may not define competing breakpoints.
