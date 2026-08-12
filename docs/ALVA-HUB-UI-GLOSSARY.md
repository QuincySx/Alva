# ALVA Hub UI Domain Glossary

> Status: Accepted vocabulary for the ALVA Hub Tauri UI system.
>
> This document names visual and interaction ownership. Product-domain terms
> such as HubSession, EngineBranch, Agent, and Inspector remain defined by
> `ALVA-HUB-GLOSSARY.md`.

## Reference Artifacts

### Reference screenshot

One captured Cindy interaction state. It is evidence for information hierarchy,
spatial proportion, trigger relationship, overlay direction, and interaction
path. It is not an instruction to copy Cindy branding pixel for pixel.

### State linework

A simplified drawing derived from one reference screenshot. It preserves the
state's controls, content order, anchoring, and closing path while removing
incidental visual detail.

There are 67 retained state lineworks.

### Aggregate wireframe

One normalized page-family drawing that combines related state lineworks into
a stable region hierarchy. It annotates component boundaries, width ownership,
docking, scroll ownership, and overlay anchors.

The seven page families are Chat Shell, New Conversation, Right Workbench,
Automations, Plugins and Skills, Settings, and Context Menus. Global search is
a shared surface rather than an eighth page skeleton.

### Reference triple

The review set for one state: Cindy reference screenshot, state linework, and
current Hub screenshot. Review uses all three because any one alone loses
either visual detail, structural intent, or implementation truth.

## Component Layers

### Token

A semantic value shared across the UI system: color, typography, spacing,
radius, shadow, z-layer, size, density, duration, or easing. A Token is not a
component-specific magic number.

### Control

The smallest public interactive or status component, such as Button, Field,
Tabs, Tooltip, Menu, Popover, Dialog, Disclosure, Badge, or Switch. A Control
owns its primitive states, accessibility contract, and motion.

### Region

A structural component that owns a meaningful part of the application frame,
including size relationship, docking, responsive mode, or scrolling. Examples
are AppFrame, Sidebar, ContentFrame, ScrollRegion, DockRegion, Workbench, and
WorkbenchPage.

### Surface

A reusable visual grouping inside a Region, such as PanelSection, Notice,
StatusListItem, SettingsGroup, ListRow, CardGrid, or EmptyState. A Surface owns
grouping and repeated visual rhythm but not page-level geometry.

### Domain component

A Hub-specific composition that binds product data and actions to Controls,
Regions, and Surfaces. Examples are HubComposer, ModelPicker, HubTimeline,
EngineReadinessPanel, FallbackPolicyPanel, and TeamPanel.

### Page

A routable or major product state that arranges public Domain components and
Regions. A Page does not own primitive interaction styling.

### UI facade

The public `ui/` module consumed by business components. It wraps Radix and
native controls so focus, portals, keyboard behavior, collision handling,
visual states, and motion remain consistent.

### Slot

A documented composition location such as header, body, action, meta, footer,
or details. Slots allow content variation without exposing primitive internals.

### Variant

A finite typed choice that changes an intentional component behavior or visual
role, such as tone, size, density, or overlay width. A Variant is not arbitrary
CSS injection.

### Default density

The control and spacing scale used by pages, forms, and primary content.

### Compact density

The control and spacing scale used by Sidebar navigation, menus, model lists,
and Workbench tools. Compact does not mean page-specific pixel overrides.

## Layout Ownership

### AppFrame

The Region that owns the Sidebar/Main/Workbench relationship and global
breakpoints. Child pages do not negotiate those widths independently.

### ContentFrame

The Region that owns the main reading width and horizontal gutter shared by
timeline content, notices, and Composer.

### Scroll owner

The single element responsible for scrolling one Region. A Region with a scroll
owner must not contain another competing page-level `overflow: auto` container.

### DockRegion

The persistent area attached to the bottom of Main for Composer, approval, and
status content that must stay available while conversation content scrolls.

### Workbench

The resizable right-side Tool Workspace Region. It is docked on wide desktop
windows and becomes an overlay Sheet on narrower windows.

### Overlay

A portal-based temporary surface positioned relative to a trigger, viewport,
or application frame. Menus, Popovers, Dialogs, and Sheets are Overlay classes.
The Overlay component owns collision, focus, z-layer, size variants, and close
behavior.

## Motion

### Motion purpose

The reason an element moves: feedback, spatial relationship, state explanation,
or avoiding a jarring transition. Motion without one of these purposes is
decorative and is removed from normal productivity flows.

### High-frequency interaction

An interaction users may perform many times per session, such as navigation,
tab selection, list selection, or search. High-frequency interactions use
color and focus feedback rather than repeated translation or scaling.

### Interruptible transition

A transition that can reverse immediately from its current visual state when
the user changes direction. Popovers, Dialogs, Disclosures, and Workbench
opening and closing must be interruptible.

### Reduced motion

The UI behavior under `prefers-reduced-motion: reduce`. Positional movement is
removed or made instant while useful color and opacity state changes remain.

## Development and Acceptance

### UI Lab

A development/test-only Hub route or entry that renders Controls, Regions,
Domain components, and aggregate page states inside the real application web
environment. It is not production navigation.

### Fixture state

A deterministic UI Lab data set representing one component or product state.
It provides stable copy, loading, errors, permissions, engine availability,
and interaction outcomes without depending on a live backend.

### Live scenario

A selected UI Lab or product scenario connected to real stores and Tauri
commands. Live scenarios prove integration after deterministic visual states
are accepted.

### Visual baseline

A fixed-viewport screenshot produced from a named Fixture state. Baselines are
compared by machine-generated differences and human review; they are not strict
pixel clones of Cindy.

### Component state matrix

The set of applicable default, hover, active, focus, disabled, loading, open,
close, error, empty, compact, default-density, dark-theme, long-copy, and
reduced-motion states required for a component.

### Page complete

A page whose mapped Fixture states pass visual and interaction review and whose
real product flow passes store/Tauri integration. Visual similarity alone does
not satisfy completion.
